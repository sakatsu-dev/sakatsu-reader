//! 屠つ (Sakatsu) desktop shell.
//!
//! A thin adapter: it exposes `sakatsu-core` to the web UI through Tauri
//! commands and (later) owns capture, the hotkey, and sidecar lifecycles. All
//! real logic lives in the core crate so it can be reused by a future mobile app.
//!
//! **Tests for this crate live in `tests/`, not in `#[cfg(test)]` modules here.**
//! Cargo can only attach link args to integration-test targets, so a unit-test
//! harness for this lib cannot get the Common-Controls 6 manifest that Tauri's
//! window and webview code paths need: the moment a unit test links them it dies
//! at startup with STATUS_ENTRYPOINT_NOT_FOUND. See `build.rs`.

pub mod analyzer_probe;
pub mod binding;
#[cfg(windows)]
pub mod bits;
pub mod capture;
pub mod checkpoint;
pub mod clipboard;
pub mod commands;
#[cfg(debug_assertions)]
pub mod fault;
pub mod lifecycle;
pub mod live;
pub mod memory;
pub mod route;
pub mod runtime_status;
pub mod setup;
pub mod setup_manager;
pub mod vlm_setup;
// Windows only, and it has to be: the module calls `window.hwnd()` and imports the `windows`
// crate, which is a `[target.'cfg(windows)'.dependencies]` entry. Compiled unconditionally it
// takes the whole shell down off Windows with "unresolved import `windows`" — invisible here,
// and invisible in CI, which builds `src-tauri` on Windows alone.
#[cfg(windows)]
pub mod titlebar;
pub mod update;
pub mod warmup;
pub mod work;

use std::path::PathBuf;

use commands::{AnalyzerState, AudioState, DictionaryState, OcrState, StoreState, TranslatorState};
use live::Live;
use route::{Readiness, Route};
use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer, Remembering};
use sakatsu_core::dict::{Dictionary, TagGlossary, TagInfo};
use sakatsu_core::model::AnalyzerPolicy;
use sakatsu_core::ocr::{MangaOcr, PaddleOcr};
use sakatsu_core::store::{AnalyzerChoice, Store};
use sakatsu_core::IchiranAnalyzer;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The store opens first because the analyzer choice is one of the things it holds and
    // `choose_analyzer` needs it before anything is managed — and now for a second reason:
    // the log's switch is in there too, and startup is only loggable if the file is open
    // before startup happens.
    let store = open_store(&store_dir());
    let settings = store.settings();
    // Copied out because `setup` runs after the builder has finished with `settings`.
    let preferred_analyzer = settings.analyzer;
    let built_in = built_in_analyzer();
    apply_logging(&settings, "starting");
    // Before `choose_analyzer`, which loads whatever was kept the moment it is built.
    words_kept_at(&store);

    // **What an interrupted install left behind, said out loud before anything reads a
    // dictionary.** A recovery record protects the generation a failed transaction may still need,
    // and protection nobody is told about is a directory that quietly stops being reclaimed and an
    // install that refuses for reasons the reader cannot see. Reported, never acted on: which of
    // two half-finished states somebody wanted is not a thing to guess at startup.

    let dictionaries = discover_dictionaries(&dictionaries_dir());
    // Loaded once at startup, like the dictionaries themselves: a frequency list is tens of
    // thousands of rows and re-reading it per search would put a file read in the path of
    // every keystroke's worth of results.
    let frequencies = commands::FrequencyState::load(
        dictionaries_dir(),
        &settings.frequency_source,
        settings.frequency_direction,
    );
    sakatsu_core::log_info!(
        "session",
        "frequency: {}",
        frequencies
            .0
            .read()
            .ok()
            .and_then(|held| held
                .as_ref()
                .map(|f| format!("{} ({} words)", f.name, f.len())))
            .unwrap_or_else(|| "none installed".into())
    );
    sakatsu_core::log_info!(
        "session",
        "dictionaries: {}",
        if dictionaries.0.now().dictionaries.is_empty() {
            "none imported".to_string()
        } else {
            dictionaries
                .0
                .now()
                .dictionaries
                .iter()
                .map(|dict| dict.meta().title.clone())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    sakatsu_core::log_info!(
        "session",
        "models: manga-ocr {}, pp-ocrv6 {}",
        present(MangaOcr::is_present(&model_dir())),
        present(PaddleOcr::is_present(&paddle_model_dir()))
    );

    tauri::Builder::default()
        // **The reader window is the app's lifetime.**
        //
        // Tauri's default is to exit once the last window closes, and that default was wrong here
        // because the capture frame counts as a window. Closing the reader with the frame up left
        // the process running with nothing to drive it: the frame is `decorations(false)` so it
        // has no close button, `skip_taskbar(true)` so it is not in the taskbar or Alt-Tab, and
        // `always_on_top(true)` so it sits over everything regardless. No binding reopens the
        // reader either — `Capture`, `Overlay` and `Switch` are the three, and none of them is
        // "come back". The chords went on working, which is what made it read as a window that had
        // merely been hidden, and Task Manager was the only way out.
        //
        // On the window event rather than in `commands::close_window`, because that command is
        // only the caption button. Alt+F4 and Close on the system menu — which `titlebar.rs`
        // deliberately offers — never reach it.
        .on_window_event(|window, event| {
            use tauri::Manager;
            if window.label() == commands::MAIN_LABEL
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                sakatsu_core::log_info!("session", "the reader was closed — ending the session");
                // **The local vision server is this process's child, so it goes when we do.**
                // Nothing else would stop it: it is not a service and has no tray icon to close,
                // and left running it would hold a port, the weights and a gigabyte of video
                // memory until the machine restarted — then lose the port to its own orphan on
                // the next launch.
                crate::vlm_setup::stop_managed_runtime();
                // The same reasoning for the analyzer sidecar, which is a child of this process
                // whenever the persistent session is on. Closing our end of its stdin is enough
                // to end its read loop, but only if something closes it — and under WSL the
                // process outlives a parent that simply exits.
                sakatsu_core::analyzer::ichiran::server::shutdown();
                window.app_handle().exit(0);
            }
        })
        // **First, ahead of every other plugin, and that ordering is the point.**
        //
        // A second copy of the reader is turned away while this plugin initialises, which
        // happens before `.setup()` and therefore before `apply_saved_settings` — so the
        // doomed instance exits without ever reaching the global chords. Registered after
        // `binding::plugin()` it would still exit, but only once both had raced for the same
        // hotkeys, and the loser is whichever asked second.
        //
        // That race is not hypothetical: it is what left the switch binding unregistered
        // while the settings went on displaying the chord it was set to, and it looked
        // intermittent because it depended on which copy started first. `cargo tauri dev`
        // over a still-running build is the everyday way to cause it.
        //
        // What it does *not* prevent is the doomed instance running everything above this
        // line — opening the store, loading the dictionaries and the frequency list — before
        // it finds out it is not wanted. That is a few seconds of wasted work and two lines in
        // a shared log, and it is the price of the check living in a plugin. It writes no
        // settings: the only path that does is in `.setup()`, which it never reaches.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Raise the copy that is already running, so launching again reads as "here it
            // is" rather than as nothing happening at all.
            sakatsu_core::log_info!("session", "a second copy was launched — raising this one");
            use tauri::Manager;
            if let Some(window) = app.get_webview_window(crate::commands::MAIN_LABEL) {
                // All three: minimised and hidden are different states, and `set_focus` alone
                // rescues neither.
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(binding::plugin())
        .plugin(tauri_plugin_dialog::init())
        // **One built-in analyzer**, shared: it is what Reference search asks for by name and
        // what the route falls back to, and neither needs its own copy.
        .manage(analyzer_to_open_with(
            preferred_analyzer,
            built_in.0.clone(),
        ))
        .manage(built_in)
        .manage(dictionaries)
        .manage(store)
        // Conventional OCR stays lazy; a remembered PaddleOCR-VL selection is warmed after the
        // window appears in `.setup()` below. If a required resource is absent, the stub says so
        // rather than failing silently.
        .manage(OcrState::new(model_dir(), paddle_model_dir(), &settings))
        .manage(AudioState::default())
        // Empty until scripts/fetch-wordnet.ps1 has run, which is a supported
        // state: without it the structural cues still work and particle senses
        // simply abstain more often.
        .manage(commands::ParticleState::load(
            wordnet_dir().join("categories.tsv"),
        ))
        // Likewise optional: without it the reference page says how to fetch it rather
        // than showing an empty section.
        .manage(commands::KanjiState::load(kanji_dir().join("kanjidic.tsv")))
        .manage(frequencies)
        .manage(commands::FetchState::default())
        .manage(setup_manager::SetupState::load())
        .manage(warmup::WarmupState::default())
        // Also lazy: ~300 MB, and only used when the learner opens the disclosure.
        .manage(TranslatorState::new(translation_model_dir()))
        .setup(move |app| {
            update::initialize(app.handle())?;
            apply_saved_settings(app.handle());
            // **Only the accepted instance reaches setup, and that is why recovery lives here.**
            //
            // Both of these were briefly run before the builder, which put them ahead of the
            // single-instance plugin — so a second launch would have inspected an installation
            // still running in the first process, and the sweep would have deleted its `payload`
            // and `prepared` directories out from under it. The comment on that plugin already
            // states the invariant this broke: the doomed instance writes nothing, because the
            // only path that writes is this one.
            recover_owned_resources(app.handle());
            // Only the accepted instance reaches setup. Cleanup runs after restart, when old
            // handles are gone, and protects every actual override/fallback path this run uses.
            //
            // **Owned before any of those paths are resolved.** They all answer from the
            // device-data location, and this runs on a background thread beside a session the
            // reader can start an install in — so working out what to protect and *then* claiming
            // would leave this deleting against a picture of where the resources used to be.
            std::thread::spawn(move || {
                let outcome = owning([sakatsu_core::setup::claim::Claim::Everything], |owned| {
                    let target = device_data_dir();
                    let protected = vec![
                        dictionaries_dir(),
                        kanji_dir(),
                        wordnet_dir(),
                        model_dir(),
                        paddle_model_dir(),
                        translation_model_dir(),
                        audio_dir(),
                    ];
                    sakatsu_core::setup::resources::reclaim_relocated(owned, &target, &protected)
                });
                match outcome {
                    Ok(notices) => {
                        for notice in notices {
                            sakatsu_core::log_warn!("setup", "{notice}");
                        }
                    }
                    Err(busy) => sakatsu_core::log_warn!(
                        "setup",
                        "old resource copies kept for now: {busy} They will be removed at the \
                         next start."
                    ),
                }
            });
            // **Ask for the foreground rather than assuming it.** A window declared in
            // `tauri.conf.json` is created and shown, which is not the same as being raised:
            // the report was that the reader opens behind everything until you find it in the
            // taskbar. The single-instance handler has always done this for a *second* launch
            // — the first had nothing at all.
            //
            // Windows grants this only while the app still holds the right its launcher handed
            // over, which is why `analyzer_to_open_with` exists: asking is worth nothing if the
            // app spent ten seconds first.
            if let Some(window) = tauri::Manager::get_webview_window(app, commands::MAIN_LABEL) {
                let _ = window.show();
                let _ = window.set_focus();
            }
            // Now that there is something on screen, go and find out whether the analyzer the
            // reader actually asked for is available.
            upgrade_analyzer_in_the_background(app.handle().clone(), preferred_analyzer);
            // Likewise for the reader whose remembered engine is PaddleOCR-VL: a choice made in
            // a previous session is still a choice, and without this the first capture of every
            // session pays the twenty seconds again. Also after the window is up — the point is
            // to spend the wait somewhere nobody is watching, which is not true before it.
            commands::warm_ocr_in_the_background(app.handle().clone());
            // After the window exists, because it needs the HWND. This is what keeps the
            // snap-layout flyout working now that the window draws its own caption — see
            // `titlebar.rs` for why one integer from a child window is the whole mechanism.
            #[cfg(windows)]
            titlebar::install(app.handle());
            Ok(())
        })
        .invoke_handler(reader_commands!())
        .run(tauri::generate_context!())
        .expect("error while running 屠つ");
}

/// Pick the analyzer to run on: Ichiran if it answers, the built-in one otherwise.
///
/// Ichiran is preferred because it is genuinely better — full conjugation chains,
/// compound words, and its own senses (`project notes` §4). But it wants WSL, PostgreSQL
/// and about 4.7 GB, so it is **optional**: Lindera with an embedded
/// dictionary ships inside the binary and needs no setup at all.
///
/// The check is a real analysis rather than a look for the binary, because a sidecar
/// that is installed but cannot reach its database fails in exactly the same way as a
/// missing one, and falling back is right in both cases.
///
/// The saved setting decides, and `SAKATSU_ANALYZER` overrides it — an environment
/// variable is for forcing a choice while debugging, so it should win over a preference
/// somebody set weeks ago and forgot.
/// Every command the reader can invoke, in one place both the app and its tests use.
///
/// # Why this is a macro rather than a list
///
/// `tauri::generate_handler!` expands to a closure over the exact command functions named in it, so
/// the registration cannot be handed around as a value. Before this existed, `run()` had the list
/// and each IPC test wrote its own — which meant a test could establish that `setup_snapshot`
/// binds its arguments and behaves correctly while production had stopped registering it at all.
/// The test would stay green and the command would be unreachable in the app.
///
/// Sharing it makes that impossible: the harness invokes through the same expansion the app does,
/// so a command removed here fails the tests that use it. `src-tauri/tests/setup_ipc.rs`
/// demonstrates that rather than asserting it — see `a_command_missing_from_production_is_missing
/// _from_the_harness`.
///
/// **Registration is not the same as state.** A command whose managed state the harness does not
/// hold still registers; it fails when invoked, which is the harness's business rather than this
/// list's.
#[macro_export]
macro_rules! reader_commands {
    () => {
        tauri::generate_handler![
            $crate::commands::analyze,
            $crate::update::update_status,
            $crate::update::update_check,
            $crate::update::update_download,
            $crate::update::update_install,
            $crate::commands::study,
            $crate::commands::lookup,
            $crate::commands::lookup_in_context,
            $crate::commands::search_words,
            $crate::commands::lookup_kanji,
            $crate::commands::prefetch_reference,
            $crate::commands::prefetch_analysis,
            $crate::commands::warmup_diagnostics,
            $crate::commands::runtime_status,
            $crate::commands::diagnostic_report,
            $crate::commands::unload_ocr,
            $crate::commands::unload_translator,
            $crate::commands::kanji_diagram,
            $crate::commands::kanji_related,
            $crate::commands::kanji_compounds,
            $crate::commands::gloss_index_ready,
            $crate::commands::dictionary_upkeep,
            $crate::commands::build_gloss_index,
            $crate::commands::particle_senses,
            $crate::commands::expression_offer,
            $crate::commands::word_status,
            $crate::commands::dictionaries,
            $crate::commands::explain_tag,
            $crate::commands::explain_pattern,
            $crate::commands::search_grammar,
            $crate::commands::search_particles,
            $crate::commands::set_word_status,
            $crate::commands::word_counts,
            $crate::commands::settings,
            $crate::commands::save_settings,
            $crate::commands::minimize_window,
            $crate::commands::toggle_maximize,
            $crate::commands::close_window,
            $crate::commands::window_maximized,
            $crate::commands::titlebar_metrics,
            $crate::commands::window_menu,
            $crate::commands::restart_app,
            $crate::commands::reload_frequency,
            $crate::commands::reload_analyzer,
            $crate::commands::analyzer_engine,
            $crate::commands::notices_available,
            $crate::commands::open_notices,
            $crate::commands::open_creator_link,
            $crate::commands::store_error,
            $crate::commands::recovery_report,
            $crate::commands::recovery_keep_current,
            $crate::commands::recovery_restore_previous,
            $crate::commands::log_file,
            $crate::commands::reveal_log,
            $crate::commands::corpus_file,
            $crate::commands::reveal_corpus,
            $crate::commands::copy_text,
            $crate::commands::open_frame,
            $crate::commands::close_frame,
            $crate::commands::pin_window,
            $crate::commands::frame_clickthrough,
            $crate::commands::toggle_frame,
            $crate::commands::place_frame,
            $crate::commands::frame_rect,
            $crate::commands::frame_limits,
            $crate::commands::read_frame,
            $crate::commands::set_capture_bind,
            $crate::commands::set_overlay_bind,
            $crate::commands::set_switch_bind,
            $crate::commands::ocr_engine,
            $crate::commands::recognize_image,
            $crate::commands::recognize_clipboard,
            $crate::commands::translate,
            $crate::commands::translate_sentences,
            $crate::commands::translate_variants,
            $crate::commands::audio_known,
            $crate::commands::audio_availability,
            $crate::commands::anki_decks,
            $crate::commands::anki_deck_info,
            $crate::commands::anki_create_deck,
            $crate::commands::anki_duplicate_deck,
            $crate::commands::anki_rename_deck,
            $crate::commands::anki_delete_deck,
            $crate::commands::add_to_anki,
            $crate::commands::word_audio,
            $crate::commands::explain_term,
            $crate::commands::explain_tags,
            $crate::commands::search_terms,
            $crate::commands::all_terms,
            $crate::commands::patterns_for_term,
            $crate::commands::readiness,
            $crate::commands::catalog,
            $crate::commands::start_ichiran_setup,
            $crate::vlm_setup::vlm_setup_status,
            $crate::vlm_setup::start_vlm_runtime,
            $crate::commands::credits,
            $crate::commands::fetch_component,
            $crate::commands::cancel_fetch,
            $crate::commands::cancel_request,
            $crate::commands::import_dictionary,
            $crate::setup_manager::setup_snapshot,
            $crate::setup_manager::setup_preflight,
            $crate::setup_manager::setup_start,
            $crate::setup_manager::setup_pause,
            $crate::setup_manager::setup_use_text_only,
            $crate::setup_manager::setup_finish_analyzer,
            $crate::setup_manager::setup_repair,
            $crate::setup_manager::setup_choose_storage,
            $crate::setup_manager::setup_move_resources,
            $crate::setup_manager::setup_restart_artifact,
            $crate::setup_manager::setup_import_offline,
        ]
    };
}

/// Take ownership of the resources a piece of work writes, **and only then** work out where they
/// are.
///
/// # The ordering this exists to enforce
///
/// Claiming is not enough on its own. Every one of these operations begins by resolving a resource
/// directory — `dictionaries_dir()`, `destination(id)`, the relocation mappings — and those
/// functions answer from the device-data location, which a move changes. Resolve first and claim
/// second and this is reachable:
///
/// ```text
/// resolve the old location → a move completes → take the claim → write to the abandoned location
/// ```
///
/// No two operations overlap while holding claims, and the write still lands somewhere nothing will
/// look again. So the claim is taken here, the paths are resolved **inside** `work`, and the witness
/// is held until `work` returns — which is after publication *and* after the session's handles have
/// been rebuilt, because those handles are part of what the work changed.
///
/// **This function is where the ordering lives.** The `Claimed` witness the writing operations take
/// proves only that a claim is held when they are called; it cannot see whether the caller resolved
/// a path first. Resolving inside this closure is what makes the order right, which makes it a
/// convention with one enforcement point — checked by asking the commands, in
/// `src-tauri/tests/writer_ownership.rs`.
///
/// The refusal is the sentence naming what is already running, ready to show a reader.
pub fn owning<T>(
    claims: impl IntoIterator<Item = sakatsu_core::setup::claim::Claim>,
    work: impl FnOnce(&sakatsu_core::setup::claim::Claimed) -> T,
) -> Result<T, String> {
    let _lifecycle = lifecycle::gates()
        .enter(lifecycle::Gate::Publication)
        .map_err(|e| e.to_string())?;
    let owned = sakatsu_core::setup::claim::claim(claims)?;
    Ok(work(&owned))
}

/// Where looked-up words are kept between sessions.
///
/// **Unconditional now, and that is a deliberate loss of a switch.** It used to be gated on
/// `search_history_size`, on the reasoning that a file of words somebody looked up *is* a record
/// of what they looked up, so it should answer to the setting governing that record. Sound, but
/// the setting has gone with the reference page's Recent list — the tabs do that job — and the
/// coupling was never visible where it mattered: the control said "Remember searches" and said
/// nothing about a cache, so nobody turning it off knew this was what they were also turning off.
/// Its default was 10, so this file was being written for everyone anyway.
///
/// If it should be optional again it wants a switch of its own, honestly labelled, rather than
/// borrowing one from a neighbour.
///
/// Only words reach the file; `analyzer::remembering` filters passages out at the write. This says
/// *where*, and no longer whether — which is why it stopped taking the settings.
fn words_kept_at(store: &StoreState) {
    // `analysis-cache.json`, and the name matters more than it looks. This first shipped as
    // `words.json`, which is the store's *own* file for saved word statuses — so the cache
    // was renamed over a learner's known/seen marks, and the store then refused to load
    // or save because it found a map where it expects a list. A cache that destroys real data
    // is worse than no cache. `store::tests` now proves the two cannot collide.
    let beside_the_settings = store
        .store
        .as_ref()
        .map(|open| open.root().join("analysis-cache.json"));
    sakatsu_core::analyzer::remembering::set_persistence(beside_the_settings);
}

/// The route to open with, chosen without asking anything slow.
///
/// **There is no window yet when this is called.** Every `.manage` argument is evaluated before
/// `run()` creates one, so work here is time the app spends with nothing on screen — and probing
/// Ichiran is a `wsl -e ichiran-cli` spawn loading a large Lisp image. Measured from a reader's own
/// log, three launches: 6.2s, 6.7s, 8.7s. So the app opens on the built-in analyzer, **recorded as
/// standing in while Ichiran is checked**, and `upgrade_analyzer_in_the_background` probes once the
/// window is up.
fn analyzer_to_open_with(
    saved: AnalyzerChoice,
    built_in: std::sync::Arc<dyn Analyzer>,
) -> AnalyzerState {
    let policy = route::effective_policy(saved);
    #[cfg(debug_assertions)]
    if let Some(mode) = fault::mode() {
        if policy != AnalyzerPolicy::BuiltIn {
            return AnalyzerState::new(fault::route(policy, built_in, mode));
        }
    }
    AnalyzerState::new(Route {
        policy,
        primary: built_in,
        fallback: None,
        readiness: if policy == AnalyzerPolicy::BuiltIn {
            Readiness::Ready
        } else {
            Readiness::Probing
        },
        generation: 0,
    })
}

/// Whether the built-in analyzer is the embedded parser the Ichiran path takes its advice from
/// (`sakatsu_core::analyzer::lindera::shared`), rather than a second instance of it.
///
/// Set by measurement: one instance means one
/// tokenizer lock for Reference searches and passage advice alike, and that is a latency question.
/// Measured: no added search or passage latency under contention, and 92 MB less at first use.
pub const UNIFY_LINDERA: bool = true;

/// The built-in analyzer, held for the whole session beside whichever one is preferred.
///
/// `search_words` asks this one specifically for single-word Reference queries, so it has to
/// survive `upgrade_analyzer_in_the_background` swapping Ichiran into `AnalyzerState`. The route
/// holds a clone of the same `Arc` as its primary or fallback, so this is one analyzer however many
/// states hold it.
///
/// **Built here, in `run`, before the window**, either way. Unified, this is where the shared
/// parser is first loaded; separate, it is a second instance with its own copy of the embedded
/// dictionary beside the one the advice loads on first use.
/// Public so `tests/ipc.rs` registers the *same* state the app does. A harness that built its
/// own would drift from this one silently, and the drift would look like a passing test.
pub fn built_in_analyzer() -> commands::BuiltInAnalyzerState {
    if UNIFY_LINDERA {
        return match sakatsu_core::analyzer::lindera::shared_analyzer() {
            Some(analyzer) => commands::BuiltInAnalyzerState(analyzer),
            None => {
                eprintln!("the built-in analyzer failed to load");
                commands::BuiltInAnalyzerState(std::sync::Arc::new(
                    sakatsu_core::analyzer::StubAnalyzer,
                ))
            }
        };
    }
    match LinderaAnalyzer::new() {
        Ok(analyzer) => commands::BuiltInAnalyzerState(std::sync::Arc::new(analyzer)),
        Err(error) => {
            eprintln!("the built-in analyzer failed to load: {error}");
            commands::BuiltInAnalyzerState(std::sync::Arc::new(
                sakatsu_core::analyzer::StubAnalyzer,
            ))
        }
    }
}

/// Probe for the preferred analyzer off the startup path, and publish what it finds.
///
/// **By generation, not by engine name.** The generation is read before the probe; if anything was
/// published while it ran — the reader's own selection above all — the probe's answer is dropped.
/// Comparing engine names could not see a selection of the same engine, or a choice and a change
/// back. A probe that finds Ichiran unavailable publishes that too, so the passages that follow
/// record why the built-in analyzer is standing in.
fn upgrade_analyzer_in_the_background<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    saved: AnalyzerChoice,
) {
    if route::effective_policy(saved) == AnalyzerPolicy::BuiltIn {
        return;
    }
    #[cfg(debug_assertions)]
    if fault::mode().is_some() {
        return;
    }
    std::thread::spawn(move || probe_and_publish(&app));
}

/// Probe the preferred analyzer again if the route says it is due, at most one probe at a time.
///
/// Called where the reader signals a read is coming — the capture frame opening, a capture
/// starting — so an Ichiran that failed at startup (WSL was not up yet, say) is found again
/// without a restart, and a failure is never "unavailable for the whole session". Rate-limited by
/// [`route::REPROBE_AFTER`], and published by the same generation check as the startup probe.
/// Interrupted startup is retried on the next intent; the probe owner prevents duplicate work.
pub fn reprobe_if_due<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::Manager;
    static PROBING: AtomicBool = AtomicBool::new(false);
    let Some(state) = app.try_state::<AnalyzerState>() else {
        return;
    };
    if !state.route().reprobe_due(std::time::Instant::now()) {
        return;
    }
    if PROBING.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        probe_and_publish(&app);
        PROBING.store(false, Ordering::Release);
    });
}

/// One probe, published only if nothing else was published meanwhile.
#[doc(hidden)]
pub fn probe_and_publish<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(probe) = analyzer_probe::Probe::start() else {
        return;
    };
    use tauri::{Emitter, Manager};
    let (Some(state), Some(built_in)) = (
        app.try_state::<AnalyzerState>(),
        app.try_state::<commands::BuiltInAnalyzerState>(),
    ) else {
        return;
    };
    let started = state.route();
    let Some(found) = probe_route_with(started.policy, built_in.0.clone(), |ichiran| {
        let result = ichiran.check_available(probe.cancellation());
        if probe
            .cancellation()
            .load(std::sync::atomic::Ordering::Acquire)
        {
            None
        } else {
            Some(result.map(|_| ()))
        }
    }) else {
        return;
    };
    let engine = found.primary.engine().to_string();
    let Some(published) = probe.publish(|| state.publish_if(started.generation, found)) else {
        return;
    };
    if !published {
        sakatsu_core::log_info!(
            "analyzer",
            "leaving the route alone: a selection was published while the probe ran"
        );
        return;
    }
    if engine != started.primary.engine() {
        sakatsu_core::log_info!(
            "analyzer",
            "{engine} answered; swapping it in for {}",
            started.primary.engine()
        );
        if let Some(warmup) = app.try_state::<warmup::WarmupState>() {
            warmup.invalidate();
        }
    }
    // The settings row reads the engine once, on load. Without this it goes on naming the one
    // the app opened with until it is reopened.
    let _ = app.emit(commands::RELOADED, "analyzer");
}

/// Ask for the analyzer `policy` prefers, and describe the route that results.
///
/// **Slow**: it runs 水 through Ichiran, a WSL spawn loading a large Lisp image. Called off every
/// thread anybody waits on — the background probe, and `reload_analyzer`, which is `async`.
pub(crate) fn probe_route(policy: AnalyzerPolicy, built_in: std::sync::Arc<dyn Analyzer>) -> Route {
    probe_route_with(policy, built_in, |ichiran| {
        Some(ichiran.analyze("\u{6c34}").map(|_| ()))
    })
    .expect("an explicit readiness check cannot be cancelled")
}

fn probe_route_with(
    policy: AnalyzerPolicy,
    built_in: std::sync::Arc<dyn Analyzer>,
    check: impl FnOnce(&IchiranAnalyzer) -> Option<Result<(), sakatsu_core::analyzer::AnalyzeError>>,
) -> Option<Route> {
    let built_in_route = |readiness| Route {
        policy,
        primary: built_in.clone(),
        fallback: None,
        readiness,
        generation: 0,
    };
    if policy == AnalyzerPolicy::BuiltIn {
        return Some(built_in_route(Readiness::Ready));
    }
    #[cfg(debug_assertions)]
    if let Some(mode) = fault::mode() {
        return Some(fault::route(policy, built_in.clone(), mode));
    }
    let ichiran = configured_ichiran();
    Some(match check(&ichiran)? {
        // Remembered: every one-shot call is a spawn, and the same text is asked for far more
        // often than it looks — a Japanese reference search analyses its query, and `readiness`
        // analyses 水 as a health check every time it runs.
        Ok(_) => Route {
            policy,
            primary: std::sync::Arc::new(Remembering::new(ichiran)),
            fallback: Some(built_in.clone()),
            readiness: Readiness::Ready,
            generation: 0,
        },
        Err(error) => {
            if policy == AnalyzerPolicy::Ichiran {
                // Asked for by name and not working: every passage will say so.
                sakatsu_core::log_error!(
                    "analyzer",
                    "Ichiran was asked for and is not answering: {error}"
                );
            } else {
                sakatsu_core::log_warn!(
                    "analyzer",
                    "the built-in analyzer stands in; Ichiran said: {error}"
                );
            }
            built_in_route(Readiness::Unavailable {
                reason: error.to_string(),
                since: std::time::Instant::now(),
            })
        }
    })
}

#[derive(serde::Deserialize)]
struct SavedIchiranCommand {
    program: String,
    prefix_args: Vec<String>,
}

/// Build the local analyzer from the launcher's app-owned record when one exists.
///
/// The environment variable remains the explicit developer override and therefore wins.
/// The setup script also writes it for command-line users, but an in-app restart inherits
/// the old process environment on Windows. Reading this tiny file is what makes the
/// visible "Restart to use Ichiran" button sufficient instead of requiring sign-out.
fn configured_ichiran() -> IchiranAnalyzer {
    if std::env::var_os(sakatsu_core::analyzer::ichiran::COMMAND_ENV).is_some() {
        return IchiranAnalyzer::new();
    }

    let path = app_data_dir().join("ichiran-command.json");
    match std::fs::read_to_string(&path) {
        Ok(json) => match parse_ichiran_command(&json) {
            Some((program, prefix_args)) => IchiranAnalyzer::with_command(program, prefix_args),
            None => {
                sakatsu_core::log_warn!(
                    "analyzer",
                    "ignoring an invalid Ichiran launcher at {}",
                    path.display()
                );
                IchiranAnalyzer::new()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => IchiranAnalyzer::new(),
        Err(error) => {
            sakatsu_core::log_warn!(
                "analyzer",
                "could not read the Ichiran launcher at {}: {error}",
                path.display()
            );
            IchiranAnalyzer::new()
        }
    }
}

fn parse_ichiran_command(json: &str) -> Option<(String, Vec<String>)> {
    let mut saved: SavedIchiranCommand = serde_json::from_str(json).ok()?;
    if saved.program.trim().is_empty() {
        return None;
    }
    if !saved.prefix_args.iter().any(|argument| argument == "-f") {
        saved.prefix_args.push("-f".into());
    }
    Some((saved.program, saved.prefix_args))
}

/// Push the saved settings out to the places that cannot ask for them.
///
/// The binding layer, the clipboard watcher and the analyzer's console flag are all bare
/// functions and statics with no route to the store, so startup hands each of them its
/// value — the same job `save_settings` does whenever one changes afterwards.
///
/// Binding failure is tolerated: a chord another application already owns cannot be taken,
/// and that must not stop the app from starting, since the buttons still work. Falling
/// back to the default is worth trying, because the saved binding may have been fine on
/// the machine it was saved on and be taken on this one.
fn apply_saved_settings<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;

    let saved = app
        .state::<StoreState>()
        .store
        .as_ref()
        .map(sakatsu_core::store::Store::settings)
        .unwrap_or_default();

    // Before anything else visible: this decides the window's own theme and the colour it paints
    // in the moment before the webview draws, so doing it late is a flash of the wrong palette.
    commands::apply_window_theme(app, &saved.theme);

    binding::set_hold(saved.hold_ms);
    binding::set_lock_tap(saved.lock_tap());
    binding::set_overlay_enabled(saved.overlay_enabled);
    // At startup as well as on change, or the setting would be remembered and not applied
    // until something else saved.
    commands::set_always_on_top(app, saved.always_on_top);
    sakatsu_core::analyzer::ichiran::set_quiet(saved.quiet);
    apply_collecting(&saved);
    // The session itself is started lazily by the first analysis that wants it, so this only
    // records the intent — opening the app with it on costs nothing until something is read.
    sakatsu_core::analyzer::ichiran::server::set_persistent(saved.ichiran_server);

    // **Opening the reader starts no model service.** This used to warm the vision model here,
    // so the first capture would not wait on it — which meant launching a reading app put an
    // AI runtime in the tray before anybody had asked for a read, and closing that runtime by
    // hand did not keep it closed.
    //
    // The trade it was making is also worse than it looks: the wait it saved is now explained
    // rather than hidden, because a cold capture says "Loading PaddleOCR-VL…" instead of
    // sitting silent. A capture still starts the service, because a capture is the moment
    // somebody actually asks for one.

    // The thread runs for the life of the app and parks when the setting is off, so this
    // starts it unconditionally and lets the flag decide whether it does anything.
    clipboard::watch(app.clone());
    clipboard::set_watching(saved.read_clipboard);

    if let Err(refusal) = binding::apply(
        app,
        &saved.capture_bind,
        &saved.overlay_bind,
        saved.switch_bind.as_ref(),
    ) {
        eprintln!("capture binding unavailable: {refusal}");
        sakatsu_core::log_warn!("binding", "saved binding unavailable: {refusal}");

        let defaults = sakatsu_core::store::Settings::default();
        let (capture, overlay, switch) = after_refusal(&saved, &defaults, &refusal.actions);
        if let Err(refusal) = binding::apply(app, capture, overlay, switch) {
            eprintln!("the default bindings are unavailable too: {refusal}");
            sakatsu_core::log_error!(
                "binding",
                "the default bindings are unavailable too: {refusal}"
            );
        }
    }
}

/// Which bindings to try on the second attempt, given what the first one could not take.
///
/// **Only what was refused goes back to a default.** This used to re-apply all three, so one
/// unavailable chord silently replaced two working ones — and since the default for the switch
/// hotkey is *nothing*, a refusal anywhere left it unbound while the settings page went on
/// displaying the chord someone had set. A second instance of the app, or any program holding
/// one of the chords, was enough to trigger it, which is why it looked intermittent.
///
/// A refused switch binding is dropped rather than replaced, because there is no default to
/// replace it with. That is the honest outcome and it is in the log with its own reason.
fn after_refusal<'a>(
    saved: &'a sakatsu_core::store::Settings,
    defaults: &'a sakatsu_core::store::Settings,
    refused: &[binding::Action],
) -> (
    &'a sakatsu_core::store::Binding,
    &'a sakatsu_core::store::Binding,
    Option<&'a sakatsu_core::store::Binding>,
) {
    let refused = |action| refused.contains(&action);
    (
        if refused(binding::Action::Capture) {
            &defaults.capture_bind
        } else {
            &saved.capture_bind
        },
        if refused(binding::Action::Overlay) {
            &defaults.overlay_bind
        } else {
            &saved.overlay_bind
        },
        if refused(binding::Action::Switch) {
            None
        } else {
            saved.switch_bind.as_ref()
        },
    )
}

/// For log lines that report whether something is installed.
fn present(yes: bool) -> &'static str {
    if yes {
        "installed"
    } else {
        "missing"
    }
}

/// Where the optional diagnostic log is written.
///
/// The shell decides this, not `sakatsu_core::log` — that module deliberately knows
/// nothing about the OS, the same reason `model_dir()` lives here.
pub fn log_path() -> PathBuf {
    app_data_dir().join("logs").join("sakatsu.log")
}

/// Where captured Japanese is collected, beside the logs rather than inside them.
///
/// One passage per line, which is the format `SAKATSU_ANALYZER_SENTENCES` reads — so the file can
/// be handed straight to `analyzer_corpus.rs` without anything in between. See
/// `sakatsu_core::corpus` for why that is worth a second file.
pub fn corpus_path() -> PathBuf {
    app_data_dir().join("corpus").join("captured.txt")
}

/// Start or stop collecting captured Japanese, to match the settings.
///
/// **Independent of `apply_logging`, deliberately.** Collecting a corpus has nothing to do with
/// diagnosing a fault, and requiring the log to be on to collect would tie a reading aid to a
/// troubleshooting switch. The two share a settings field because one answer covers both — keep
/// what I read, or do not — and nothing else.
pub fn apply_collecting(settings: &sakatsu_core::store::Settings) {
    if !settings.collect_sentences {
        sakatsu_core::corpus::disable();
        return;
    }
    let path = corpus_path();
    if let Err(error) = sakatsu_core::corpus::enable(&path) {
        sakatsu_core::log_warn!(
            "corpus",
            "could not open {} for collecting: {error}",
            path.display()
        );
        return;
    }
    if let Some((path, held)) = sakatsu_core::corpus::status() {
        sakatsu_core::log_info!(
            "corpus",
            "collecting captured Japanese into {} ({held} already held)",
            path.display()
        );
    }
}

/// Turn the log file on or off to match the settings, and say hello when turning it on.
///
/// The header is the highest-value thing in the file. A report that begins "capture stopped
/// working" is answerable when the top of the log says which analyzer was chosen and why,
/// which recognition engine is installed and which dictionaries opened — and unanswerable
/// without it.
pub fn apply_logging(settings: &sakatsu_core::store::Settings, detail: &str) {
    if !settings.log {
        sakatsu_core::log::disable();
        return;
    }

    let path = log_path();
    if let Err(error) = sakatsu_core::log::enable(&path, settings.collect_sentences) {
        // Not fatal, and not silent either: the one place this can be reported is the
        // console, which is exactly what a packaged build lacks — so it is also the one
        // failure the log cannot log.
        eprintln!("could not open the log at {}: {error}", path.display());
        return;
    }

    sakatsu_core::log_info!(
        "session",
        "屠つ {} — {} {}, {} build",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    sakatsu_core::log_info!("session", "{detail}");
    sakatsu_core::log_info!(
        "session",
        "settings: ocr={:?} analyzer={:?} preprocess(t={} c={} u={}) clipboard={} quiet={}",
        settings.ocr,
        settings.analyzer,
        settings.preprocess.tighten,
        settings.preprocess.contrast,
        settings.preprocess.upscale,
        settings.read_clipboard,
        settings.quiet
    );
    if settings.collect_sentences {
        sakatsu_core::log_info!(
            "session",
            "keeping what is read — recognized text goes into this file and into the corpus"
        );
    }
}

/// Resolve a device resource from the new local-data root, then the legacy roaming root.
///
/// The store is intentionally excluded: changing that path would orphan learning state.
/// Environment overrides remain authoritative for development and portable installations.
///
/// **Fetches resolve through here too**, which is why every `*_install_dir` is an alias for its
/// matching `*_dir` rather than "always the local root". The two would differ in exactly one
/// situation — the legacy root holds usable content and the local one does not — and writing to
/// local there splits a resource across two directories while the loader reads only one. For a
/// dictionary that means the ones already imported vanish from lookups; for a frequency list,
/// whose directory never satisfies the dictionary predicate below, it means the download
/// succeeds and ranking silently never turns on.
///
/// A fresh install still lands in the local root, because an unusable legacy root resolves
/// there. Nothing is migrated: content stays where it is, and new content joins it.
fn resource_dir(
    override_name: &str,
    relative: &str,
    usable: impl Fn(&std::path::Path) -> bool,
) -> PathBuf {
    if let Some(dir) = std::env::var_os(override_name) {
        return PathBuf::from(dir);
    }
    let local = device_data_dir().join(relative);
    let legacy = app_data_dir().join(relative);
    if usable(&local) || !usable(&legacy) {
        local
    } else {
        legacy
    }
}

/// Where scripts/fetch-kanjidic.ps1 puts the kanji reading table.
pub fn kanji_dir() -> PathBuf {
    resource_dir("SAKATSU_KANJI_DIR", "kanji", |dir| {
        dir.join("kanjidic.tsv").is_file()
    })
}

pub fn kanji_install_dir() -> PathBuf {
    kanji_dir()
}

/// Where the optional, openly licensed pronunciation pack is indexed.
pub fn audio_dir() -> PathBuf {
    resource_dir("SAKATSU_AUDIO_DIR", "audio/tofugu-vocabulary", |dir| {
        dir.join("index.tsv").is_file()
    })
}

pub fn audio_install_dir() -> PathBuf {
    audio_dir()
}

/// Where scripts/fetch-wordnet.ps1 puts the noun category table.
pub fn wordnet_dir() -> PathBuf {
    resource_dir("SAKATSU_WORDNET_DIR", "wordnet", |dir| {
        dir.join("categories.tsv").is_file()
    })
}

pub fn wordnet_install_dir() -> PathBuf {
    wordnet_dir()
}

/// Where scripts/fetch-ocr-model.ps1 puts the recognition model.
pub fn model_dir() -> PathBuf {
    resource_dir("SAKATSU_MODEL_DIR", "models/manga-ocr", |dir| {
        MangaOcr::is_present(dir)
    })
}

pub fn model_install_dir() -> PathBuf {
    model_dir()
}

/// Where scripts/fetch-paddle-ocr.ps1 puts the PP-OCRv6 models.
///
/// `models/ppocrv6`, not the old `models/ppocrv5`: the two exports share file names (`det.onnx`,
/// `rec.onnx`), and a half-replaced directory could pair a v5 detector with a v6 recogniser
/// that no check would notice. A v5 install left behind is simply not found.
pub fn paddle_model_dir() -> PathBuf {
    resource_dir("SAKATSU_PADDLE_DIR", "models/ppocrv6", |dir| {
        PaddleOcr::is_present(dir)
    })
}

pub fn paddle_model_install_dir() -> PathBuf {
    paddle_model_dir()
}

/// Where the vision reader lives: a llama.cpp build and the two GGUF files it serves.
///
/// One directory for both because they are installed, verified and thrown away together — the
/// runtime is useless without the weights and the weights unreadable without the runtime, and
/// splitting them is how a half-install becomes two half-installs.
///
/// Under the app's own data, never on the PATH and never registered with the system. That is
/// the difference this whole component exists to make: it replaced an Ollama install, which
/// put a service and a tray application on the machine to do the same job.
pub fn vlm_dir() -> PathBuf {
    resource_dir("SAKATSU_VLM_DIR", "models/paddleocr-vl", |dir| {
        crate::vlm_setup::is_installed_in(dir)
    })
}

pub fn vlm_install_dir() -> PathBuf {
    vlm_dir()
}

/// Where scripts/fetch-translation-model.ps1 puts the translation model.
pub fn translation_model_dir() -> PathBuf {
    resource_dir("SAKATSU_MT_DIR", "models/fugumt-ja-en", |dir| {
        sakatsu_core::translate::FuguMt::is_present(dir)
    })
}

pub fn translation_model_install_dir() -> PathBuf {
    translation_model_dir()
}

/// Where word status and settings live.
pub fn store_dir() -> PathBuf {
    app_data_dir().join("store")
}

/// Open the store, degrading to an app without persistence rather than refusing to
/// start — but keeping the reason so the UI can warn instead of quietly losing marks.
pub fn open_store(dir: &std::path::Path) -> StoreState {
    match Store::open(dir) {
        Ok(store) => {
            // A store that had to set a file aside still works, so this is a notice and not
            // an error — but it is never silent. Somebody whose marks have gone should be
            // told where they went.
            let note = store.note().map(str::to_string);
            if let Some(said) = note.as_deref() {
                eprintln!("{said}");
                sakatsu_core::log_warn!("store", "{said}");
            }
            StoreState {
                store: Some(store),
                error: note,
            }
        }
        Err(error) => {
            // Nothing recoverable: the directory itself could not be used.
            let said = format!("Nothing can be saved this session: {error}");
            eprintln!("{said}");
            sakatsu_core::log_error!("store", "{said}");
            StoreState {
                store: None,
                error: Some(said),
            }
        }
    }
}

/// Where `scripts/fetch-dictionaries.ps1` puts the zips and their indexes.
/// Put back anything an interrupted offline install left moved aside.
///
/// **These directories were unreachable until now.** An install stages under
/// `.offline-setup-<pid>-<generation>` in the device data directory and removes it on success; a
/// process that stops mid-swap leaves that root behind with the only copy of a component under
/// `backup/<id>` and nothing at the destination. Nobody was going to open a temporary directory
/// named after a process id that no longer exists.
///
/// **Where each backup belongs comes from the install that made it**, not from this run's
/// configuration: the resource directories honour overrides a person can change, so reconstructing
/// the mapping here could send a backup somewhere its own install had never heard of. A root that
/// left something behind without a usable journal is reported and retained rather than guessed at.
///
/// Runs in `.setup()`, after the single-instance plugin has decided this process owns the install
/// directories. Before that it would have swept an installation still running in another one.
pub fn recover_interrupted_offline_installs<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Ok(entries) = std::fs::read_dir(device_data_dir()) else {
        return;
    };
    let roots: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".offline-setup-"))
        })
        .collect();
    if roots.is_empty() {
        return;
    }

    let recovered = sakatsu_core::setup::publication::recover_interrupted_publications(&roots);
    for (id, destination) in &recovered.restored {
        sakatsu_core::log_warn!(
            "setup",
            "{id} was restored to {} from an interrupted installation; its files had been moved              aside and never replaced",
            destination.display()
        );
        // **Taken up now, not at the next launch.** Every resource this could restore was already
        // read into a handle before `.setup()` runs — the dictionaries and the frequency list
        // before the builder, the kanji table and the particle senses as managed state — so a
        // recovery that only fixed the disk would leave this session serving the empty table it
        // loaded seconds earlier, with nothing to say why.
        //
        // `commands::adopt` is the installer's own path for exactly this, derived from the
        // component's `After` rather than a second list that could disagree with it.
        let mut loading = sakatsu_core::recovery::LoadingOutcome::Unknown;
        let mut detail = None;
        if let Some(component) = sakatsu_core::setup::catalog::find(id) {
            if commands::adopt(app, component).is_some() {
                let (outcome, why) = verify_adoption(app, component);
                loading = outcome;
                detail = why;
            } else {
                loading = sakatsu_core::recovery::LoadingOutcome::NotNeeded;
            }
        }
        // A frequency list answers for itself, because only it knows whether this is the list the
        // reader chose — the distinction `adopt` deliberately refuses to make.
        if let Some((outcome, why)) = adopt_recovered_frequency_list(app, id) {
            loading = outcome;
            detail = why;
        }
        match loading {
            sakatsu_core::recovery::LoadingOutcome::Failed => sakatsu_core::log_error!(
                "setup",
                "{id} was restored but could not be loaded: {}",
                detail.clone().unwrap_or_default()
            ),
            sakatsu_core::recovery::LoadingOutcome::Loaded => {
                sakatsu_core::log_info!("setup", "{id} was taken up without a restart")
            }
            _ => {}
        }
        if let Ok(mut kept) = restored_this_session().lock() {
            kept.push(sakatsu_core::recovery::InstallRecovery {
                id: id.clone(),
                state: sakatsu_core::recovery::InstallRecoveryState::Restored,
                destination: Some(destination.display().to_string()),
                saved: None,
                claimants: Vec::new(),
                detail,
                loading,
            });
        }
    }
    for failure in &recovered.failed {
        sakatsu_core::log_error!(
            "setup",
            "{} could not be restored to {}: {}. Its files are still at {}",
            failure.id,
            failure.destination.display(),
            failure.reason,
            failure.saved.display()
        );
    }
    for (id, saved) in &recovered.retained {
        sakatsu_core::log_warn!(
            "setup",
            "{id} has an older copy retained at {} beside a destination that already exists;              nothing was overwritten",
            saved.display()
        );
    }
    for failure in &recovered.failed {
        if let Ok(mut kept) = swept_failures().lock() {
            kept.push(sakatsu_core::recovery::InstallRecovery {
                id: failure.id.clone(),
                state: sakatsu_core::recovery::InstallRecoveryState::Failed,
                destination: Some(failure.destination.display().to_string()),
                saved: Some(failure.saved.display().to_string()),
                claimants: Vec::new(),
                detail: Some(failure.reason.clone()),
                loading: sakatsu_core::recovery::LoadingOutcome::NotNeeded,
            });
        }
    }
    for (destination, claimants) in &recovered.ambiguous {
        sakatsu_core::log_error!(
            "setup",
            "{} is offered by {} abandoned installations ({}); none was chosen, because picking              one by directory order is not a decision this can make",
            destination.display(),
            claimants.len(),
            claimants
                .iter()
                .map(|root| root.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for (root, reason) in &recovered.unidentified {
        sakatsu_core::log_error!(
            "setup",
            "an interrupted installation at {} was left in place: {reason}",
            root.display()
        );
    }

    // Only a root that has nothing left to offer is cleared away. `remove_dir` refuses a non-empty
    // one, which is the point: anything still holding a backup outlives this sweep.
    for root in &roots {
        if matches!(
            sakatsu_core::setup::publication::interrupted_publication(root),
            sakatsu_core::setup::publication::Interrupted::Nothing
        ) {
            for scratch in ["payload", "prepared", "backup"] {
                let _ = std::fs::remove_dir_all(root.join(scratch));
            }
            let _ = std::fs::remove_file(root.join("publication.json"));
            let _ = std::fs::remove_dir(root);
        }
    }
}

/// Everything startup does that **mutates the install directories**, in one place.
///
/// # Why it is a function rather than two calls in `setup`
///
/// Its placement is the whole safety property: it must run only after the single-instance plugin
/// has decided this process owns those directories, because a rejected launch running it would
/// sweep an installation still in progress in the accepted one. Two calls sitting in `.setup()`
/// were correct and unprovable — a test could only re-create the arrangement rather than exercise
/// it, and would go on passing if production moved them.
///
/// `src-tauri/tests/second_instance.rs` drives **this function**, in a child that registers the
/// same plugin and calls it from the same place. It also drives a deliberately wrong placement, so
/// the assertion is shown to be sensitive to the thing it is about.
pub fn recover_owned_resources<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    report_outstanding_recoveries(&dictionaries_dir());
    recover_interrupted_offline_installs(app);
}

/// What this session's startup sweep restored, and whether each became usable.
///
/// **Recorded when it happens, not reconstructed afterwards.** The report is built later, on
/// demand, and by then nothing on disk says which components this process put back or what
/// reloading them achieved — so the panel used to describe every restored component as needing a
/// restart, which was the safe direction and not always the true one.
/// What the sweep tried to restore and could not, **with the reason it gave**.
///
/// Kept rather than rebuilt. The report used to look at what was left on disk and say "the startup
/// sweep did not restore this; see the session log for the reason" — which is a reason nobody has
/// in front of them, for a failure the sweep had already described precisely.
static SWEPT_FAILURES: std::sync::OnceLock<
    std::sync::Mutex<Vec<sakatsu_core::recovery::InstallRecovery>>,
> = std::sync::OnceLock::new();

fn swept_failures() -> &'static std::sync::Mutex<Vec<sakatsu_core::recovery::InstallRecovery>> {
    SWEPT_FAILURES.get_or_init(Default::default)
}

static RESTORED_THIS_SESSION: std::sync::OnceLock<
    std::sync::Mutex<Vec<sakatsu_core::recovery::InstallRecovery>>,
> = std::sync::OnceLock::new();

fn restored_this_session() -> &'static std::sync::Mutex<Vec<sakatsu_core::recovery::InstallRecovery>>
{
    RESTORED_THIS_SESSION.get_or_init(Default::default)
}

/// Whether a component is usable now, asked of the handle rather than assumed.
///
/// **`adopt` returning a name is not evidence.** It says a reload was attempted; a reload that ran
/// and produced an empty table looks identical from the caller's side and is exactly the failure a
/// reader needs to hear about. So each component is asked whether it now holds anything.
fn verify_adoption<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    component: &'static sakatsu_core::setup::Component,
) -> (sakatsu_core::recovery::LoadingOutcome, Option<String>) {
    use sakatsu_core::recovery::LoadingOutcome;
    use sakatsu_core::setup::catalog::After;
    use tauri::Manager;

    let empty = |what: &str| {
        (
            LoadingOutcome::Failed,
            Some(format!(
                "the files were put back, but reloading them produced no {what}"
            )),
        )
    };
    match component.after {
        After::ImportDictionary => match app.try_state::<commands::DictionaryState>() {
            Some(state) if !state.0.now().dictionaries.is_empty() => (LoadingOutcome::Loaded, None),
            Some(_) => empty("dictionaries"),
            None => (LoadingOutcome::Unknown, None),
        },
        After::BuildKanjiDetails => match app.try_state::<commands::KanjiState>() {
            Some(state) if !state.0.now().is_empty() => (LoadingOutcome::Loaded, None),
            Some(_) => empty("characters"),
            None => (LoadingOutcome::Unknown, None),
        },
        After::BuildWordNetTable => match app.try_state::<commands::ParticleState>() {
            // `Categories` has no emptiness of its own to ask about, so this reports what is
            // actually known: the reload ran. Claiming more would be the inference this exists to
            // avoid.
            Some(_) => (LoadingOutcome::Unknown, None),
            None => (LoadingOutcome::Unknown, None),
        },
        After::UnpackFrequency => {
            // Decided by `adopt_recovered_frequency_list`, which knows whether this list is the
            // selected one; an unselected list needing no reload is not a failure.
            (LoadingOutcome::NotNeeded, None)
        }
        // Read on first use, so the restored files are already what the next use will find.
        After::Nothing | After::UnpackRuntime => (LoadingOutcome::NotNeeded, None),
        After::BuildAudioPack => (LoadingOutcome::NotNeeded, None),
    }
}

/// What interrupted installs are still outstanding, for the recovery panel.
///
/// **Reads, never repairs.** The sweep at startup is what restores; this answers "what is left" so
/// the interface can describe it, and calling it must not change anything a reader is looking at.
pub fn outstanding_install_recoveries() -> Vec<sakatsu_core::recovery::InstallRecovery> {
    use sakatsu_core::recovery::{InstallRecovery, InstallRecoveryState};
    use sakatsu_core::setup::publication::{interrupted_publication, Interrupted};

    let Ok(entries) = std::fs::read_dir(device_data_dir()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let root = entry.path();
        if !root.is_dir()
            || !root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".offline-setup-"))
        {
            continue;
        }
        match interrupted_publication(&root) {
            Interrupted::Nothing => {}
            Interrupted::Unidentified { root, reason } => found.push(InstallRecovery {
                id: root
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                state: InstallRecoveryState::Unidentified,
                destination: None,
                saved: Some(root.join("backup").display().to_string()),
                claimants: Vec::new(),
                detail: Some(reason),
                loading: sakatsu_core::recovery::LoadingOutcome::NotNeeded,
            }),
            Interrupted::Candidates(candidates) => {
                for candidate in candidates {
                    // Anything still here after the startup sweep is something the sweep declined
                    // to act on: a destination that exists, or one more than one root claims.
                    let state = if candidate.destination.exists() {
                        InstallRecoveryState::Retained
                    } else {
                        InstallRecoveryState::Competing
                    };
                    found.push(InstallRecovery {
                        id: candidate.id,
                        state,
                        destination: Some(candidate.destination.display().to_string()),
                        saved: Some(candidate.saved.display().to_string()),
                        claimants: vec![candidate.root.display().to_string()],
                        detail: None,
                        loading: sakatsu_core::recovery::LoadingOutcome::NotNeeded,
                    });
                }
            }
        }
    }
    // What this session restored, so the panel can say whether each is usable rather than telling
    // everybody to restart.
    if let Ok(kept) = restored_this_session().lock() {
        found.extend(kept.iter().cloned());
    }
    if let Ok(kept) = swept_failures().lock() {
        found.extend(kept.iter().cloned());
    }
    // Competing claims are one entry per destination, not one per root, or the panel would list the
    // same problem twice and offer to solve it separately.
    fold_competing(found)
}

/// Collapse several roots claiming one destination into a single entry naming all of them.
fn fold_competing(
    entries: Vec<sakatsu_core::recovery::InstallRecovery>,
) -> Vec<sakatsu_core::recovery::InstallRecovery> {
    use sakatsu_core::recovery::InstallRecoveryState;
    use std::collections::BTreeMap;

    let mut competing: BTreeMap<String, sakatsu_core::recovery::InstallRecovery> = BTreeMap::new();
    let mut others = Vec::new();
    for entry in entries {
        if entry.state != InstallRecoveryState::Competing {
            others.push(entry);
            continue;
        }
        let key = entry.destination.clone().unwrap_or_default();
        match competing.get_mut(&key) {
            Some(existing) => existing.claimants.extend(entry.claimants),
            None => {
                competing.insert(key, entry);
            }
        }
    }
    // A single claimant is not a competition: the sweep left it for some other reason, and calling
    // it competing would ask a person to choose between one thing.
    for (_, mut entry) in competing {
        // A single claimant is not a competition: the sweep left it for some other reason, and
        // calling it competing would ask a person to choose between one thing. **It is not
        // relabelled with an invented reason either** — the sweep's own failures arrive from
        // `swept_failures` carrying what it actually said, so anything reaching here is a leftover
        // whose state the report does not know and should not guess at.
        if entry.claimants.len() == 1 {
            entry.state = InstallRecoveryState::Unidentified;
            entry.detail = Some(
                "the startup sweep left this in place and did not record a reason for it".into(),
            );
        }
        others.push(entry);
    }
    others.sort_by(|left, right| left.id.cmp(&right.id));
    others
}

/// Take up a recovered frequency list — **but only if it is the one the reader chose.**
///
/// `commands::adopt` deliberately ignores `UnpackFrequency`, and is right to: fetching a list is not
/// choosing it, so an install must not change which one is in use. Recovery is a different question
/// with the same shape. The list it puts back may be the one `settings.frequency_source` already
/// names, and in that case the session is holding an **empty** frequency state loaded before the
/// files existed — the same stale-handle problem the kanji table had, arriving through the one
/// component `adopt` declines to touch.
///
/// So the selection decides. A restored list nobody selected is left alone, which preserves the
/// property `adopt` is protecting; a restored list that *is* selected is loaded, because the reader
/// already asked for it and is currently getting nothing.
fn adopt_recovered_frequency_list<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    id: &str,
) -> Option<(sakatsu_core::recovery::LoadingOutcome, Option<String>)> {
    use sakatsu_core::recovery::LoadingOutcome;

    use tauri::Manager;

    // Not a frequency list at all: this helper has no opinion, and the caller keeps whatever the
    // ordinary adoption path decided.
    let restored = id.strip_prefix("frequency-")?;

    // **Every branch below is an outcome, not a `false`.** They used to collapse into one: an
    // unselected list and a selected list whose loading failed both returned `false`, and the
    // caller then reported `not_needed` for both — telling a reader nothing needed doing when in
    // fact their chosen list had come back unusable.
    let Some(store) = app.try_state::<commands::StoreState>() else {
        return Some((LoadingOutcome::Unknown, None));
    };
    let settings = store.settings();
    if settings.frequency_source != restored {
        // Restoring a list nobody chose changes nothing, and should not: `adopt` declines to select
        // a list for the same reason.
        return Some((LoadingOutcome::NotNeeded, None));
    }
    let Some(state) = app.try_state::<commands::FrequencyState>() else {
        return Some((LoadingOutcome::Unknown, None));
    };
    let fresh = commands::FrequencyState::load(
        dictionaries_dir(),
        &settings.frequency_source,
        settings.frequency_direction,
    );
    let loaded = fresh.0.read().ok().and_then(|held| held.clone());
    // **Reported by what it holds, not by having run.** An empty list is no list — `load` says so
    // itself — so a reload that produced nothing is a failure to load, not a success.
    let usable = loaded.is_some();
    {
        let Ok(mut current) = state.0.write() else {
            return Some((LoadingOutcome::Unknown, None));
        };
        *current = loaded;
    }
    if usable {
        sakatsu_core::log_info!(
            "setup",
            "the recovered frequency list {restored} was taken up without a restart"
        );
        return Some((LoadingOutcome::Loaded, None));
    }
    Some((
        LoadingOutcome::Failed,
        Some(format!(
            "{restored} is the selected word list, and reading it back produced no entries"
        )),
    ))
}

/// Log every dictionary container with an unfinished publication, and what it needs.
///
/// The three outcomes need different words because they need different actions: one is finishable
/// by a routine, one is only bookkeeping, and one is a choice between two people's work. A single
/// "recovery pending" line would send somebody looking for a decision that does not exist, or
/// worse, hide one that does.
pub fn report_outstanding_recoveries(dir: &std::path::Path) {
    use sakatsu_core::dict::Outstanding;

    for (container, outcome) in sakatsu_core::dict::outstanding_recoveries(dir) {
        let name = container
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| container.display().to_string());
        match outcome {
            // The loudest, because it blocks reclamation *and* the next install, and nothing can
            // resolve it without a person looking at the file.
            Err(reason) => sakatsu_core::log_error!(
                "dictionary",
                "{name}: the recovery record is unreadable ({reason}). Nothing will be reclaimed                  and no new install of this dictionary will start until it is resolved."
            ),
            Ok(Outstanding::ConflictingPublication { record, live }) => sakatsu_core::log_warn!(
                "dictionary",
                "{name}: an unfinished install published {} but {} is live. Keeping one discards                  the other, so this needs a deliberate choice.",
                record.published,
                live.as_deref().unwrap_or("a legacy layout")
            ),
            Ok(Outstanding::Unresolved { record }) => sakatsu_core::log_warn!(
                "dictionary",
                "{name}: an install switched to {} and did not finish. The previous generation is                  kept until this is resolved.",
                record.published
            ),
            Ok(Outstanding::CleanupOnly { record }) | Ok(Outstanding::NeverSwitched { record }) => {
                sakatsu_core::log_info!(
                    "dictionary",
                    "{name}: an unfinished install (transaction {}) changed nothing; only its                      record is left.",
                    record.transaction
                )
            }
        }
    }
}

pub fn dictionaries_dir() -> PathBuf {
    resource_dir("SAKATSU_DICT_DIR", "dictionaries", |dir| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|entry| Dictionary::exists_in(&entry.path()))
        })
    })
}

pub fn dictionaries_install_dir() -> PathBuf {
    dictionaries_dir()
}

/// The app's own directory under the user profile. Matches the identifier in
/// `tauri.conf.json`, which is where Tauri itself would put app data.
pub(crate) fn app_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("com.sakatsu.app")
}

/// Large, device-specific resources live in LocalAppData. Settings and learning state stay
/// in `app_data_dir()` under the immutable Tauri identity.
pub fn device_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("SAKATSU_DEVICE_DATA_DIR") {
        return PathBuf::from(path);
    }
    let configured = app_data_dir().join("device-data-location.txt");
    if let Ok(value) = std::fs::read_to_string(configured) {
        let value = value.trim();
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("com.sakatsu.app")
}

pub fn set_device_data_dir(path: &std::path::Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(path)?;
    std::fs::create_dir_all(app_data_dir())?;
    sakatsu_core::setup::resources::write_location(
        &app_data_dir().join("device-data-location.txt"),
        path,
    )
}

/// Open every already-imported dictionary under `dir`.
///
/// Importing is **not** done here: it takes over a minute for a large dictionary,
/// and blocking startup on it would look like a hang. A dictionary appears once its
/// index exists; wiring import into the UI belongs with first-run setup
/// (`project notes` §9, phase 6).
///
/// Priority order decides which source is promoted when the analyzer has nothing, so
/// it is fixed deliberately rather than left to whatever order the filesystem
/// returns: a real dictionary before a name dictionary.
pub fn discover_dictionaries(dir: &std::path::Path) -> DictionaryState {
    // Which directories count and in what order is `Dictionary::discover`'s business — both
    // on-disk layouts, and the ranking that puts an ordinary dictionary before a name one. The
    // measurement harnesses call the same function, so a run cannot be scored against a different
    // set of dictionaries than the app opens.
    //
    // What is *not* shared is the failure policy below: a broken index must not stop the app from
    // starting, whereas a measurement run fails rather than quietly scoring less.
    let mut dictionaries = Vec::new();
    let mut tags = TagGlossary::jmdict();
    for path in Dictionary::discover(dir) {
        match Dictionary::open(&path) {
            Ok(dictionary) => {
                // Each dictionary is the authority on its own codes, so its bank is
                // merged over the built-in JMdict table.
                // From the generation the reader resolved, not the directory it was asked for:
                // a published dictionary keeps its tag bank beside its entries.
                if let Some(own) = read_tags(&dictionary.root().join("tags.json")) {
                    tags.merge(&own);
                }
                dictionaries.push(dictionary);
            }
            // A broken index must not stop the app from starting; the rest still work.
            Err(error) => {
                eprintln!("skipping dictionary at {}: {error}", path.display());
                sakatsu_core::log_warn!(
                    "dict",
                    "skipping dictionary at {}: {error}",
                    path.display()
                );
            }
        }
    }

    DictionaryState(Live::new(std::sync::Arc::new(commands::Dictionaries {
        dictionaries,
        tags,
    })))
}

fn read_tags(path: &std::path::Path) -> Option<std::collections::HashMap<String, TagInfo>> {
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    use binding::Action;
    use sakatsu_core::store::{Binding, Settings};

    /// Saved settings that differ from the defaults in all three bindings, so a value that
    /// came from the wrong one is visible rather than accidentally equal.
    fn saved() -> Settings {
        Settings {
            capture_bind: Binding::Mouse {
                button: sakatsu_core::store::MouseButton::Middle,
            },
            overlay_bind: Binding::Key {
                chord: "Backquote".into(),
            },
            switch_bind: Some(Binding::Key {
                chord: "Alt+Backquote".into(),
            }),
            ..Settings::default()
        }
    }

    /// The bug: one refused chord took the other two down with it.
    #[test]
    fn a_refused_binding_does_not_take_the_others_with_it() {
        let (saved, defaults) = (saved(), Settings::default());
        let (capture, overlay, switch) = after_refusal(&saved, &defaults, &[Action::Overlay]);

        assert_eq!(
            capture, &saved.capture_bind,
            "the capture binding registered"
        );
        assert_eq!(
            switch,
            saved.switch_bind.as_ref(),
            "and so did the switch chord"
        );
        assert_eq!(
            overlay, &defaults.overlay_bind,
            "only the refused one falls back"
        );
    }

    /// A refused switch chord is dropped, not replaced — its default is nothing.
    #[test]
    fn a_refused_switch_is_dropped_and_the_rest_are_kept() {
        let (saved, defaults) = (saved(), Settings::default());
        let (capture, overlay, switch) = after_refusal(&saved, &defaults, &[Action::Switch]);

        assert_eq!(capture, &saved.capture_bind);
        assert_eq!(overlay, &saved.overlay_bind);
        assert_eq!(switch, None);
    }

    /// Everything refused is the case the blanket reset was written for, and it still works.
    #[test]
    fn everything_refused_falls_back_to_everything_default() {
        let (saved, defaults) = (saved(), Settings::default());
        let (capture, overlay, switch) = after_refusal(
            &saved,
            &defaults,
            &[Action::Capture, Action::Overlay, Action::Switch],
        );

        assert_eq!(capture, &defaults.capture_bind);
        assert_eq!(overlay, &defaults.overlay_bind);
        assert_eq!(switch, None);
    }
}

#[cfg(test)]
mod analyzer_setup_tests {
    use super::parse_ichiran_command;

    #[test]
    fn a_saved_launcher_preserves_argument_boundaries_and_the_full_output_flag() {
        let json = r#"{
            "program":"wsl",
            "prefix_args":["-d","My Distribution","-e","/home/reader/.local/bin/sakatsu-ichiran","-f"]
        }"#;
        let (program, arguments) = parse_ichiran_command(json).expect("valid launcher");
        assert_eq!(program, "wsl");
        assert_eq!(
            arguments,
            [
                "-d",
                "My Distribution",
                "-e",
                "/home/reader/.local/bin/sakatsu-ichiran",
                "-f"
            ]
        );
    }

    #[test]
    fn an_old_launcher_without_the_full_output_flag_is_upgraded() {
        let (_, arguments) =
            parse_ichiran_command(r#"{"program":"wsl","prefix_args":["-e","ichiran-cli"]}"#)
                .expect("valid launcher");
        assert_eq!(arguments.last().map(String::as_str), Some("-f"));
    }

    #[test]
    fn an_empty_or_malformed_launcher_is_ignored() {
        assert!(parse_ichiran_command(r#"{"program":"","prefix_args":[]}"#).is_none());
        assert!(parse_ichiran_command("not json").is_none());
    }
}
