//! The command surface — everything the web UI can call, and nothing more.
//!
//! Commands are `pub` so the integration tests in `tests/` can build an app around
//! them. They live in this module rather than in `lib.rs` because
//! `#[tauri::command]` `#[macro_export]`s a helper macro to the crate root: a
//! public command declared *at* the root collides with its own helper (E0255).
//!
//! Keep these thin. They adapt `sakatsu-core` to the IPC boundary — nothing else.
//!
//! # `#[tauri::command(async)]` is not optional here
//!
//! A plain `#[tauri::command]` runs its body **on the main thread**, inline in the IPC
//! handler — `ExecutionContext::Blocking` is the macro's default. That is fine for
//! reading a field out of memory and wrong for everything else, in two escalating ways:
//!
//! 1. Any slow command freezes the whole window while it runs. OCR, translation and
//!    a network call to AnkiConnect are all seconds long.
//! 2. `begin_capture` **deadlocks outright**. Creating a `WebviewWindow` on Windows
//!    needs the event loop to pump messages while WebView2 starts, and the event loop
//!    is the main thread — which is busy inside the command, waiting for the window it
//!    just asked for. The app stops responding, and Windows swaps in a blank ghost
//!    window over the whole screen.
//!
//! So: **a command that touches the disk, the network, a subprocess, a model, or a
//! window is marked `(async)`**, which runs it on the thread pool instead. Only
//! in-memory getters are left plain.

pub use sakatsu_core::search::{CharReading, Form, Found, KanjiView, WordHit};

pub(crate) use sakatsu_core::setup::install::{reserve_component_installs, InstallPhase};
pub use sakatsu_core::setup::transport::{CatalogEntry, CatalogSource};
use sakatsu_core::setup::transport::{FetchProgress as SetupTransfer, SetupProgress};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::window::Color;
use tauri::{Emitter, Manager, State};

use sakatsu_core::ocr::{
    AutoOcr, MangaOcr, Ocr, OcrImage, PaddleOcr, Preprocess, PreprocessOptions, StubOcr, VlmOcr,
};
use sakatsu_core::terms;
use sakatsu_core::translate::{FuguMt, StubTranslator, Translator};

use crate::capture::{self, DesktopRect};
use crate::live::Live;
use crate::setup::{self, Component, Readiness};
use crate::warmup::{Access as WarmupAccess, PassageDigest, WarmupDiagnostics, WarmupState};

use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::anki::{sentence, Anki, DeckInfo, Note};
use sakatsu_core::audio::WordAudio;
use sakatsu_core::dict::{Dictionary, DictionaryMeta, TagGlossary, WordView};
use sakatsu_core::grammar::{self, GrammarPattern, Match};
use sakatsu_core::model::{Analysis, Token};
use sakatsu_core::persistence::{Persistence, PersistenceError};
use sakatsu_core::request::{RequestIdentity, WorkPriority};
use sakatsu_core::store::{
    AudioRecord, Binding, Counts, OcrChoice, Settings, Store, Theme, ThemeMode, WordKey, WordStatus,
};

/// The analyzer route the app runs on — policy, primary, fallback, readiness and generation,
/// held and replaced as one snapshot. See [`crate::route`].
pub use crate::route::AnalyzerState;

/// The built-in analyzer, kept alongside whichever one is preferred.
///
/// **Not behind [`Live`], because this one never changes.** Lindera is compiled into the binary
/// with its dictionary; there is nothing to install, nothing to probe and nothing to swap. It is
/// held separately so a caller can ask *the fast analyzer specifically* rather than whatever
/// `AnalyzerState` currently holds — see [`sakatsu_core::search`], which is the only such caller.
pub struct BuiltInAnalyzerState(pub std::sync::Arc<dyn Analyzer>);

/// The imported dictionaries, in lookup priority order, and the tags that explain them.
///
/// Order is the promotion priority the merge policy uses, so the most trusted
/// dictionary belongs first (`project notes` §6).
///
/// One struct rather than two pieces of state, because the tag glossary is built *from* the
/// dictionaries — each is the authority on its own codes and merges its bank over JMdict's.
/// Swapping them separately would leave a window where a dictionary's senses are on screen and
/// its tag codes cannot be explained.
pub struct Dictionaries {
    pub dictionaries: Vec<Dictionary>,
    /// The JMdict abbreviation table with every dictionary's own tag bank merged
    /// over it, so a `[v5m,vt]` code can be explained wherever it turns up.
    pub tags: TagGlossary,
}

/// The dictionaries, swappable — an import is usable the moment it finishes.
pub struct DictionaryState(pub Live<Dictionaries>);

/// What the learner has marked, and their settings.
///
/// `store` is optional so a store that cannot be opened degrades to a working app
/// without persistence rather than refusing to start. `error` then says why, because
/// silently not saving a learner's marked words would be worse than saying so.
pub struct StoreState {
    pub store: Option<Store>,
    pub error: Option<String>,
}

impl StoreState {
    /// This state as the thing `core::persistence` takes: the store, and why there is none.
    ///
    /// The whole of what the shell still owns here. The degradation policy — defaults on read,
    /// a typed refusal on write — is one place now rather than reassembled at each command.
    pub fn lend(&self) -> Persistence<'_> {
        Persistence {
            store: self.store.as_ref(),
            unavailable: self.error.as_deref(),
        }
    }

    /// The saved settings, or the defaults when there is no store.
    ///
    /// Public because startup reads them before anything is managed — the analyzer choice
    /// and the hold threshold both have to be applied before the first event arrives.
    pub fn settings(&self) -> Settings {
        self.lend().settings()
    }
}

/// Everything the study UI needs for one passage, in one round trip.
///
/// **Re-exported, not defined here.** It moved to [`sakatsu_core::study`] with the code that
/// builds it: every field was already a core type, so the shell was holding the shape of a
/// response it had no other business knowing about. Existing `use crate::commands::StudyView`
/// keeps working, and the wire format is unchanged — the field order moved with the struct.
pub use sakatsu_core::study::StudyView;

/// A pattern the UI can render: the match, plus the pack text that explains it.
#[derive(Debug, Serialize)]
pub struct ExplainedPattern {
    pub matched: Match,
    pub pattern: Option<GrammarPattern>,
}

/// Analyse a passage and break down its grammar.
///
/// The UI reaches this only by name, as `invoke("study", { text })` — the name and
/// the argument name are part of the contract, and `tests/ipc.rs` pins both.
#[tauri::command(async)]
pub fn study(
    analyzer: State<'_, AnalyzerState>,
    store: State<'_, StoreState>,
    kanji: State<'_, KanjiState>,
    dictionaries: State<'_, DictionaryState>,
    text: String,
    request: Option<RequestIdentity>,
) -> Result<StudyView, String> {
    let _analysis = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Analysis)
        .map_err(|e| e.to_string())?;

    let started = std::time::Instant::now();

    // **Each live handle resolved once, here, and lent for the whole call.** That is the shell's
    // job and the reason this function still exists: `sakatsu_core::study` takes references and
    // never reaches for a handle of its own, so "the whole passage saw one set of dictionaries"
    // is a property of the signature rather than a promise in a comment.
    //
    // It is stable handles, not a transaction — see the `study` module header for what that does
    // and does not guarantee.
    // **One route snapshot for the whole passage**: policy, primary, fallback and readiness read
    // together, so the retry and the execution record cannot describe two different selections.
    let route = analyzer.route();
    let identity = request.clone();
    let work = crate::work::scheduler()
        .start(
            (route.primary.engine() == "ichiran").then_some(crate::work::Lane::Ichiran),
            request,
        )
        .map_err(|error| error.to_string())?;
    let open = dictionaries.0.now();
    let kanji = kanji.0.now();
    let plan = route.plan();
    // Asked before a retry starts: a superseded request never launches one.
    let still_wanted = || work.checkpoint().is_ok();

    let view = sakatsu_core::study::study_resilient(
        &text,
        &plan,
        &open.dictionaries,
        &kanji,
        store.store.as_ref(),
        &still_wanted,
    )
    .inspect_err(|error| {
        sakatsu_core::log_error!(
            "analyze",
            "failed on {} chars: {error}",
            text.chars().count()
        );
    })
    .map_err(|unanswered| record_unanswered(&analyzer, identity.as_ref(), unanswered))?;
    // And after it returns, as before: nothing stale is published.
    work.checkpoint().map_err(|error| error.to_string())?;
    log_fallback(view.analysis.execution.as_ref());
    analyzer
        .last()
        .record(crate::runtime_status::analysis_result(
            identity.as_ref(),
            view.analysis.execution.as_ref(),
            false,
        ));

    sakatsu_core::log_info!(
        "analyze",
        "{} chars → {} tokens, {} grammar matches, in {:.2}s",
        text.chars().count(),
        view.analysis.tokens.len(),
        view.patterns.len(),
        started.elapsed().as_secs_f64()
    );
    sakatsu_core::log_text!("analyze", "passage: {text:?}");

    Ok(view)
}

/// Record a failed analysis — not a cancelled one, which answered nobody — and give its message.
fn record_unanswered(
    analyzer: &AnalyzerState,
    request: Option<&RequestIdentity>,
    unanswered: sakatsu_core::study::Unanswered,
) -> String {
    if !matches!(
        unanswered,
        sakatsu_core::study::Unanswered::Cancelled { .. }
    ) {
        analyzer
            .last()
            .record(crate::runtime_status::analysis_result(request, None, true));
    }
    unanswered_message(unanswered)
}

/// The message a passage with no analysis reports. A cancelled request says so in the scheduler's
/// own words, so the interface treats it exactly as it treats every other superseded request.
fn unanswered_message(unanswered: sakatsu_core::study::Unanswered) -> String {
    match unanswered {
        sakatsu_core::study::Unanswered::Cancelled { .. } => crate::work::Cancelled.to_string(),
        other => other.to_string(),
    }
}

/// Say in the log when a passage was read by the fallback — the reason and scope, never the text.
fn log_fallback(execution: Option<&sakatsu_core::model::Execution>) {
    if let Some(fallback) = execution.and_then(|execution| execution.fallback.as_ref()) {
        if fallback.scope == sakatsu_core::model::FallbackScope::Passage {
            sakatsu_core::log_warn!(
                "analyzer",
                "the built-in analyzer read this passage ({:?}): {}",
                fallback.reason,
                fallback.detail
            );
        }
    }
}

/// Mark a word, returning the status now stored so the UI never guesses.
#[tauri::command(async)]
pub fn set_word_status(
    state: State<'_, StoreState>,
    token: Token,
    status: WordStatus,
) -> Result<WordStatus, PersistenceError> {
    let _write = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Store)
        .map_err(|e| PersistenceError {
            code: sakatsu_core::persistence::ErrorCode::WriteFailed,
            message: e.to_string(),
        })?;

    state.lend().mark(&token, status)
}

/// What the store already holds for one word.
///
/// **For a word the passage response did not carry a status for.** `study` returns `statuses`
/// parallel to the top-level tokens; a *component* of a grouped word is not in that list, and the
/// panel showing it needs to say whether the reader has marked it — the component's own status,
/// never the word it is part of. Read-only, and `New` for a word nobody has marked.
#[tauri::command(async)]
pub fn word_status(state: State<'_, StoreState>, token: Token) -> WordStatus {
    // **Unmarked when there is no store**, which is what every other reader of a status does:
    // the panel works without one and simply shows nothing marked.
    let Some(store) = state.store.as_ref() else {
        return WordStatus::default();
    };
    store.status(&sakatsu_core::store::WordKey::for_token(&token))
}

/// How many words the learner has marked, by status.
#[tauri::command(async)]
pub fn word_counts(state: State<'_, StoreState>) -> Counts {
    state.lend().counts()
}

/// The saved settings, or the defaults when there is no store.
/// Pin the main window above the others, or let it go.
///
/// The capture frame is always on top and has to be — it is a window you look *through*.
/// This is the same wish for the window holding the analysis, so the two can sit over a game
/// or a manga viewer together.
///
/// No window, nothing to do: `save_settings` is reachable from the frame's webview too, and
/// at shutdown the main window can already be gone.
pub fn set_always_on_top<R: tauri::Runtime>(app: &tauri::AppHandle<R>, on: bool) {
    if let Some(window) = app.get_webview_window(MAIN_LABEL) {
        if let Err(error) = window.set_always_on_top(on) {
            sakatsu_core::log_warn!("window", "could not set always-on-top: {error}");
        }
    }
}

/// Pin or unpin the window now, rather than when the settings next save.
///
/// The save is debounced by 400ms, and a pin that takes half a second to happen reads as a
/// button that did not work. The setting still travels the ordinary way for persistence;
/// this is only about the window obeying the click that asked.
#[tauri::command]
pub fn pin_window<R: tauri::Runtime>(app: tauri::AppHandle<R>, on: bool) {
    set_always_on_top(&app, on);
}

/// The three caption buttons, as commands rather than as frontend window powers.
///
/// The capability file grants the page `core:window:default` — the getters — plus
/// start-dragging, and says why: geometry goes through commands rather than handing the
/// frontend a general power over the window. Minimise, maximise and close are the same kind of
/// thing, so they arrive the same way, and the page needs no new permission to have a title
/// bar.
///
/// **`toggle_maximize` is also called from native code's behalf.** The maximise button is
/// covered by the hit-test overlay in `titlebar.rs`, so the webview never sees that click: the
/// overlay reports it as an event and the page turns round and calls this. One path for both,
/// which is what keeps a keyboard user and a mouse user getting the same behaviour.
#[tauri::command]
pub fn minimize_window<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn toggle_maximize<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) -> bool {
    // Reported back rather than left for the page to ask about separately: the glyph has to
    // change with the state, and a second round trip to find out what just happened is a
    // frame the button spends showing the wrong shape.
    let maximized = window.is_maximized().unwrap_or(false);
    let _ = if maximized {
        window.unmaximize()
    } else {
        window.maximize()
    };
    !maximized
}

#[tauri::command]
pub fn close_window<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) {
    // `close`, not `destroy`: it runs the ordinary close path, so anything listening for it —
    // the settings save on the way out — still happens.
    let _ = window.close();
}

/// Whether the window is maximised, for the button's glyph on startup and after a snap.
///
/// A snap gesture, Win+arrow or a double-click on the drag region all change this without the
/// page being involved, so it cannot be tracked by remembering what was clicked.
#[tauri::command]
pub fn window_maximized<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) -> bool {
    window.is_maximized().unwrap_or(false)
}

/// How tall the title bar is and how wide its buttons are.
///
/// **Reported rather than restated**, the same move `frame_limits` makes for the capture
/// frame's floor. The overlay that gives the maximise button its snap-layout hit test is
/// positioned from these numbers in Rust; if the CSS held its own copy, the two would drift and
/// the flyout would appear over the close button — with nothing failing loudly to say so.
#[derive(serde::Serialize)]
pub struct TitlebarMetrics {
    pub height: f64,
    pub button_width: f64,
}

#[tauri::command]
pub fn titlebar_metrics() -> TitlebarMetrics {
    #[cfg(windows)]
    {
        TitlebarMetrics {
            height: crate::titlebar::TITLEBAR_HEIGHT,
            button_width: crate::titlebar::CAPTION_BUTTON_WIDTH,
        }
    }
    // Elsewhere the window keeps its own decorations, so the page draws no caption buttons and
    // these are only the height of a bar it still uses to hold the wordmark.
    #[cfg(not(windows))]
    {
        TitlebarMetrics {
            height: 32.0,
            button_width: 0.0,
        }
    }
}

/// Show the window's own system menu, at the pointer.
///
/// Right-clicking a title bar opens it on every other window, and with no native caption
/// `DefWindowProc` has nothing to open it over — so the page asks. Coordinates are the CSS
/// pixels the click reported; the conversion to screen pixels lives in `titlebar.rs` beside
/// the API that needs them.
#[tauri::command]
pub fn window_menu<R: tauri::Runtime>(window: tauri::WebviewWindow<R>, x: f64, y: f64) {
    #[cfg(windows)]
    {
        let (x, y) = crate::titlebar::to_screen(&window, x, y);
        crate::titlebar::system_menu(&window, x, y);
    }
    #[cfg(not(windows))]
    let _ = (window, x, y);
}

#[tauri::command]
pub fn settings(state: State<'_, StoreState>) -> Settings {
    state.settings()
}

/// Save the settings, apply the ones that live outside the store, and **return what was
/// actually saved**.
///
/// The reply is not a formality. [`Settings::normalise`] corrects what it is handed — it
/// clamps the tint and forces `hide_during_capture` on the moment there is one, because a
/// wash the frame is still wearing when the picture is taken is a wash the recognizer reads.
/// With nothing coming back, the panel went on showing the value it sent: the checkbox read
/// "off" while every capture flickered, and unchecking it did nothing, because the next save
/// sent the stale `false` and the store corrected it again.
///
/// So it normalises **first**, before the side effects, and every one of them — the tint
/// event included, which used to carry the unclamped number — sees what the store will hold.
#[tauri::command(async)]
pub fn save_settings<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, StoreState>,
    ocr: State<'_, OcrState>,
    mut settings: Settings,
) -> Result<Settings, PersistenceError> {
    let _write = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Store)
        .map_err(|e| PersistenceError {
            code: sakatsu_core::persistence::ErrorCode::WriteFailed,
            message: e.to_string(),
        })?;

    settings.normalise();

    // A failed backup or a read-only schema must leave the running session unchanged too.
    // Persist before touching hooks, emitting theme events, or configuring an engine; otherwise
    // the drawer reports failure while capture and logging already use the rejected values.
    state.lend().save_settings(settings.clone())?;

    // Some settings are read from places that cannot reach the store — the mouse hook and
    // the shortcut handler are bare functions — so saving has to push them out rather than
    // wait to be asked. Doing it here means it happens however the setting was changed.
    crate::binding::set_hold(settings.hold_ms);
    // Clamped by the accessor rather than by the setter, because zero is a real answer here
    // — it switches the double-tap-to-lock gesture off — and a clamp would turn it into a
    // very short window instead.
    crate::binding::set_lock_tap(settings.lock_tap());
    // Turning the search history off deletes the kept words with it — the same switch, the
    // same promise. Turning it back on starts keeping them again from here.
    crate::words_kept_at(&state);
    crate::binding::set_overlay_enabled(settings.overlay_enabled);
    // And take or give back the key with it. The header's overlay button changes the setting
    // and saves, so this is the path most people reach the switch by -- setting the flag
    // without touching the registration is what left the chord held by the OS and ignored
    // here.
    crate::binding::set_overlay_live(&app, settings.overlay_enabled);
    set_always_on_top(&app, settings.always_on_top);
    set_clickthrough(&app, settings.frame_clickthrough);
    // The frame's own window is created once and only ever hidden, so it cannot re-read this
    // on its own. See [`FRAME_TINT`].
    let _ = app.emit(FRAME_TINT, settings.frame_tint);
    // The frame cannot re-read this either, and the window's own theme and pre-paint colour have
    // to move with it.
    // By reference: `Theme` stopped being `Copy` when it grew the hand-set overrides, and passing
    // it by value here moved it out from under `apply_window_theme` on the next line.
    let _ = app.emit(THEME_CHANGED, &settings.theme);
    apply_window_theme(&app, &settings.theme);
    crate::clipboard::set_watching(settings.read_clipboard);
    sakatsu_core::analyzer::ichiran::set_quiet(settings.quiet);
    // Turning this off ends the session rather than only declining to use it — see
    // `ichiran::server::set_persistent`. Applied here so the change takes effect on the next
    // analysis rather than the next launch.
    sakatsu_core::analyzer::ichiran::server::set_persistent(settings.ichiran_server);
    // Before the rest, so a session that turns logging on here still gets a header saying
    // what it was turned on over.
    crate::apply_logging(&settings, "settings changed");
    crate::apply_collecting(&settings);
    ocr.configure(&settings);
    // The Loaded row read this once at startup and never again, so it named the previous engine
    // for the rest of the session. It changes here, so it is reported here.
    //
    // `chosen_name()` rather than `engine_name()`: a capture racing this save would otherwise
    // park the settings panel behind a twenty-second load. The two strings agree whenever it
    // matters, because `configure` has just cleared `loaded` if anything changed.
    let _ = app.emit(
        OCR_ENGINE,
        OcrEngineState {
            name: ocr.chosen_name(),
            ready: ocr.is_ready(),
        },
    );
    // And start loading it, for the one engine where the first capture would otherwise spend
    // twenty seconds starting a server. `configure` has just dropped whatever was loaded if
    // anything changed, so this reloads exactly when there is something new to load and
    // returns immediately when there is not.
    warm_ocr_in_the_background(app.clone());

    Ok(settings)
}

/// Where the log file is and how big it has grown.
///
/// The size is what makes the path worth showing: it answers "is this thing actually
/// recording" without opening anything.
#[derive(Debug, Serialize)]
pub struct LogFile {
    pub path: String,
    pub bytes: u64,
    pub exists: bool,
}

#[tauri::command(async)]
pub fn log_file() -> LogFile {
    let path = crate::log_path();
    let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    LogFile {
        exists: path.is_file(),
        bytes,
        path: path.display().to_string(),
    }
}

/// Where the collected corpus is, how big it is, and how much it holds.
///
/// **The passage count is the field that matters, and it is why this is not `LogFile`.** A log's
/// size answers "is this recording"; a corpus's size answers nothing a learner wants — 40 kB is
/// not a number of sentences. What they are deciding is whether there is enough collected to be
/// worth sending, and only a count says that.
#[derive(Debug, Serialize)]
pub struct CorpusFile {
    pub path: String,
    pub bytes: u64,
    pub exists: bool,
    /// Non-empty lines, counted from disk. Right whether collecting is on, off, or the file was
    /// edited by hand — see `corpus::held_in`.
    pub passages: usize,
}

#[tauri::command(async)]
pub fn corpus_file() -> CorpusFile {
    let path = crate::corpus_path();
    let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    CorpusFile {
        exists: path.is_file(),
        bytes,
        passages: sakatsu_core::corpus::held_in(&path),
        path: path.display().to_string(),
    }
}

/// Show the collected corpus in the file manager.
///
/// The same reasoning as [`reveal_log`], including why this is not "copy the path": revealing a
/// file the learner did not ask to have on the clipboard should not replace what they copied.
#[tauri::command(async)]
pub fn reveal_corpus() -> Result<(), String> {
    let path = crate::corpus_path();
    if !path.is_file() {
        return Err(
            "nothing collected yet — turn the switch on and capture something with Japanese in it"
                .into(),
        );
    }

    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map_err(|error| format!("could not open the folder: {error}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("not supported on this platform".into())
    }
}

/// Where the bundled third-party licence texts are, if they are there.
///
/// **The notices file is how this app satisfies the licences it is built under.** MIT asks for its
/// copyright and permission notice to travel with the software and Apache-2.0 §4 asks for the
/// licence itself; the About page names the projects, which is not the same thing and never was.
/// `scripts/collect-licences.py` writes the file and `bundle.resources` ships it beside the exe.
///
/// Falls back to the checked-in copy when running from a source tree, the same way the Ichiran
/// setup script does — otherwise this is dead in `cargo tauri dev`, which is where it is looked at
/// most.
fn notices_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    let installed = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("THIRD-PARTY-NOTICES.txt"));
    if let Some(path) = installed.filter(|path| path.is_file()) {
        return Some(path);
    }
    let in_tree = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("THIRD-PARTY-NOTICES.txt");
    in_tree.is_file().then_some(in_tree)
}

/// Whether the licence notices are there to be opened, so the row can say so rather than fail.
#[tauri::command]
pub fn notices_available<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> bool {
    notices_path(&app).is_some()
}

/// Open the third-party licence texts in whatever reads a text file.
///
/// Opened rather than rendered in the app: it is 685 kB of legal text across four hundred
/// packages, and a scroller for it inside the About page would be a worse reader than the one
/// every desktop already has.
#[tauri::command(async)]
pub fn open_notices<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let path = notices_path(&app).ok_or(
        "the licence file is missing from this install — it is generated by          scripts/collect-licences.py and shipped as a bundle resource",
    )?;

    #[cfg(windows)]
    {
        // `explorer <file>` opens it with the registered handler, where `/select,` would only
        // highlight it in a folder. The licences are meant to be read, not located.
        std::process::Command::new("explorer")
            .arg(path.as_os_str())
            .spawn()
            .map_err(|error| format!("could not open the licence file: {error}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("not supported on this platform".into())
    }
}

/// Where each creator link on the About page goes.
///
/// **A name, not a URL, crosses the boundary.** The page sends `"github"` or `"kofi"` and the
/// address is decided here, so this command can open exactly two pages and nothing a webview
/// could be talked into sending. The page keeps the same two addresses as real `href`s for
/// what a link is to everything else (its accessible name, a hover preview); `about.spec.ts`
/// pins those and the test below pins these.
fn creator_url(destination: &str) -> Result<&'static str, String> {
    match destination {
        "github" => Ok("https://github.com/S-0E51"),
        "kofi" => Ok("https://ko-fi.com/sevennightmares"),
        other => Err(format!("there is no creator link called {other:?}")),
    }
}

/// Open one of the creator's pages in the default browser, leaving the app where it was.
///
/// The same launcher as [`open_notices`]: `explorer <url>` hands the address to whatever the
/// desktop has registered for it, so it opens beside the app rather than inside the webview.
#[tauri::command(async)]
pub fn open_creator_link(destination: String) -> Result<(), String> {
    let url = creator_url(&destination)?;

    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(url)
            .spawn()
            .map_err(|error| format!("could not open the browser: {error}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err("not supported on this platform".into())
    }
}

#[cfg(test)]
mod creator_links {
    use super::creator_url;

    #[test]
    fn each_name_opens_exactly_its_page() {
        assert_eq!(creator_url("github"), Ok("https://github.com/S-0E51"));
        assert_eq!(creator_url("kofi"), Ok("https://ko-fi.com/sevennightmares"));
    }

    /// Nothing else opens, and an address is a name like any other: it is refused, not followed.
    #[test]
    fn anything_else_is_refused() {
        for other in [
            "",
            "GitHub",
            "ko-fi",
            "https://github.com/S-0E51",
            "https://example.com",
            "file:///C:/Windows/System32/calc.exe",
        ] {
            assert!(creator_url(other).is_err(), "{other:?} was accepted");
        }
    }
}

/// Show the log in the file manager.
///
/// Deliberately not "copy the path to the clipboard". Copying a reference headword is an
/// explicit request and [`copy_text`] tells the watcher to ignore that write; revealing a
/// diagnostic file is neither, and should not silently replace what the learner copied.
#[tauri::command(async)]
pub fn reveal_log() -> Result<(), String> {
    let path = crate::log_path();
    if !path.is_file() {
        return Err("there is no log file yet — turn logging on and use the app a little".into());
    }

    #[cfg(windows)]
    {
        // Explorer is a windowed program, so there is no console to suppress here — unlike
        // the analyzer sidecar, which is why `quiet` exists.
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map_err(|error| format!("could not open the folder: {error}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("not supported on this platform".into())
    }
}

/// Copy text for use outside Sakatsu.
///
/// Kept behind a command rather than the webview clipboard API so the clipboard watcher can
/// recognise the app's own write and avoid analysing the copied headword as a new passage.
#[tauri::command(async)]
pub fn copy_text(text: String) -> Result<(), String> {
    crate::clipboard::write_text(&text)
}

/// What an interrupted installation left, for the panel that offers to finish it.
///
/// Rebuilt on every call rather than cached. A recovery screen that answered from a snapshot would
/// go on describing a transaction somebody else had already resolved, and the whole point of the
/// identity check on the actions below is that the interface is *expected* to be out of date
/// sometimes — so the cheapest way to be right is to look again.
#[tauri::command(async)]
pub fn recovery_report() -> sakatsu_core::recovery::RecoveryReport {
    sakatsu_core::recovery::RecoveryReport {
        dictionaries: sakatsu_core::recovery::dictionaries(&crate::dictionaries_dir()),
        installs: crate::outstanding_install_recoveries(),
    }
}

/// Resolve a dictionary's unfinished publication by keeping what is live.
///
/// **What this gives up is not nothing, and the interface says so.** The live selection is
/// preserved — whatever is being served goes on being served — but clearing the record relinquishes
/// the protection on the older generation, which a later publish may then reclaim. That is the
/// trade, and it is the only action available for a conflicting publication, where restoring would
/// discard somebody else's newer work.
#[tauri::command(async)]
pub fn recovery_keep_current(
    container: String,
    transaction: String,
    published: String,
) -> Result<(), PersistenceError> {
    // Owned first: this rewrites a dictionary container's recovery record, and `dictionaries_dir()`
    // answers from a location a move changes.
    //
    // **No reload afterwards, and that is not an omission.** Keeping clears the record and leaves
    // the live generation exactly where it was, so every open handle is already serving what this
    // decided to keep. Rebuilding them would reopen every dictionary to arrive at the same answer.
    owned_dictionaries(|| {
        let dir = crate::dictionaries_dir().join(&container);
        sakatsu_core::dict::keep_current(
            &dir,
            &sakatsu_core::dict::Identity {
                transaction,
                published,
            },
        )
        .map_err(recovery_error)
    })
}

/// Resolve a dictionary's unfinished publication by putting the previous generation back.
///
/// **The readers are rebuilt before this reports success.** Restoring moves the live pointer, and a
/// `Dictionary` resolves its generation once and keeps it — deliberately, so a lookup in flight
/// cannot have its bytes replaced underneath it. Without a reload this session would go on serving
/// the version the reader had just asked to undo, and the panel would say the restoration worked.
/// That is the same reason `build_gloss_index` rebuilds after publishing.
///
/// A handle somebody is already holding keeps its own generation. That is the lease doing its job:
/// what changes is what the *next* lookup opens.
#[tauri::command(async)]
pub fn recovery_restore_previous<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    container: String,
    transaction: String,
    published: String,
) -> Result<(), PersistenceError> {
    owned_dictionaries(|| {
        let dir = crate::dictionaries_dir().join(&container);
        sakatsu_core::dict::restore_previous(
            &dir,
            &sakatsu_core::dict::Identity {
                transaction,
                published,
            },
        )
        .map_err(recovery_error)?;
        // Inside the claim, and before success is reported: `adopt`'s dictionary arm is the one
        // implementation of "rebuild the readers and drop the cached answers", and it is what the
        // installs use.
        if let Some(component) = sakatsu_core::setup::catalog::find("dictionaries") {
            adopt(&app, component);
        }
        Ok(())
    })
}

/// Run one dictionary-writing action under ownership, reporting a refusal the panel can print.
///
/// The manual recovery actions publish generations and rewrite pointers, which is the same tree an
/// install and a move write. They took no claim at all, so a reader could press **Restore previous
/// installation** while their resources were being relocated.
fn owned_dictionaries<T>(
    work: impl FnOnce() -> Result<T, PersistenceError>,
) -> Result<T, PersistenceError> {
    match crate::owning(
        [sakatsu_core::setup::claim::Claim::Dictionaries],
        |_owned| work(),
    ) {
        Ok(outcome) => outcome,
        Err(busy) => Err(PersistenceError {
            code: sakatsu_core::persistence::ErrorCode::WriteFailed,
            message: busy,
        }),
    }
}

/// A dictionary error as the interface needs it: a code to branch on, and the sentence to print.
///
/// `stale_recovery` is the one the panel treats specially — it means the screen is describing a
/// moment that has passed, so the answer is to refresh and look again rather than to report a
/// failure the reader can do anything about.
fn recovery_error(error: sakatsu_core::dict::DictError) -> PersistenceError {
    use sakatsu_core::dict::DictError;
    use sakatsu_core::persistence::ErrorCode;

    let code = match error {
        DictError::StaleRecovery(_) => ErrorCode::StaleRecovery,
        DictError::RecoveryOutstanding(_) | DictError::Superseded => ErrorCode::SettingsReadOnly,
        _ => ErrorCode::WriteFailed,
    };
    PersistenceError {
        code,
        message: error.to_string(),
    }
}

/// Why persistence is unavailable, if it is. The UI warns on this rather than letting
/// the learner mark words that are silently going nowhere.
#[tauri::command]
pub fn store_error(state: State<'_, StoreState>) -> Option<String> {
    state.error.clone()
}

/// The analyzer's tokens, with any name it shattered put back together.
///
/// **Applied here rather than inside the analyzer, and that is the whole design.** Ichiran is a
/// sidecar over a JMdict-derived database with no names in it; it cannot be given a user dictionary
/// and it cannot be taught. So the names are restored on this side, from the dictionaries the app
/// has already opened — which also means the built-in Lindera analyzer gets it without knowing.
///
/// Outside `Remembering`'s cache on purpose: that cache is keyed on text and holds what the
/// *analyzer* said, and this is not the analyzer. The cost is a handful of index reads — the shard
/// holding every name that starts with 善 is 584 entries.
///
/// See `core/src/dict/names.rs` for the rule and for what it deliberately will not do.
/// Let the dictionary settle a reading the analyzer was unsure of.
///
/// The built-in analyzer now reads from UniDic, whose token and lemma readings closed most of
/// the old IPADIC gap; Ichiran's JMdict-derived database can still win on contextual homographs
/// such as 者 as もの or しゃ and 乳房 as ちぶさ or にゅうぼう. Measured over the corpus that
/// remaining gap is five words in eleven hundred tokens, and Jitendex is already open and
/// already knows every reading each spelling has.
///
/// The same shape as the merge policy in `project notes` §6, one field across: the dictionary is
/// promoted where the analyzer has nothing better. It corrects only towards a reading every
/// open dictionary agrees on — see `dict::readings` for why a spelling with two readings on
/// record is deliberately left alone.
/// Analyse a passage. Kept because it is the smallest useful thing the core does,
/// and because `study` is a superset a caller may not want.
#[tauri::command(async)]
pub fn analyze(
    state: State<'_, AnalyzerState>,
    dictionaries: State<'_, DictionaryState>,
    text: String,
    request: Option<RequestIdentity>,
) -> Result<Analysis, String> {
    let _analysis = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Analysis)
        .map_err(|e| e.to_string())?;

    // **The same passes `study` runs, in the same order**, because it is now literally the same
    // function: `study::analyse` is a strict prefix of `study::study`. This ran the joins alone for
    // a while, which meant the two commands disagreed about the readings they returned — and once
    // the correction pass began setting `reading_ambiguous`, about whether a reading was flagged as
    // a guess at all. A caveat that depends on which command the caller reached for is worse than
    // none, and the way to stop that recurring is one callable service rather than two
    // descriptions of it.
    let route = state.route();
    let identity = request.clone();
    let work = crate::work::scheduler()
        .start(
            (route.primary.engine() == "ichiran").then_some(crate::work::Lane::Ichiran),
            request,
        )
        .map_err(|error| error.to_string())?;
    let open = dictionaries.0.now();
    let still_wanted = || work.checkpoint().is_ok();
    let analysis = sakatsu_core::study::analyse_resilient(
        &text,
        &route.plan(),
        &open.dictionaries,
        sakatsu_core::study::AnalysisOptions::PRODUCTION,
        &still_wanted,
    )
    .map_err(|unanswered| record_unanswered(&state, identity.as_ref(), unanswered))?
    .analysis;
    work.checkpoint().map_err(|error| error.to_string())?;
    log_fallback(analysis.execution.as_ref());
    state.last().record(crate::runtime_status::analysis_result(
        identity.as_ref(),
        analysis.execution.as_ref(),
        false,
    ));
    Ok(analysis)
}

/// Look one word up across every open dictionary and apply the merge policy.
///
/// Takes the whole token, not just a string: the policy needs the analyzer's own
/// senses to decide what is primary, and its reading to decide whether a
/// dictionary's reading should be adopted at all.
#[tauri::command(async)]
pub fn lookup(state: State<'_, DictionaryState>, token: Token) -> Result<WordView, String> {
    let open = state.0.now();
    sakatsu_core::lookup::word(&token, &open.dictionaries).map_err(|error| error.to_string())
}

/// Look one selected word up with the sentence-local evidence available to Analysis.
///
/// This is deliberately separate from [`lookup`] and `search_words`. A Reference query has
/// no selected position or neighbouring tokens, so applying this order there would make a
/// contextual claim without context. The returned `word` is already reordered so every
/// downstream consumer — summary, detail pane, and Anki — sees the same canonical answer.
#[tauri::command(async)]
pub fn lookup_in_context(
    state: State<'_, DictionaryState>,
    warmup: State<'_, WarmupState>,
    tokens: Vec<Token>,
    index: usize,
    request: Option<RequestIdentity>,
) -> Result<sakatsu_core::dict::ContextualWordView, String> {
    let ownership = crate::work::scheduler()
        .own(request)
        .map_err(|error| error.to_string())?;
    let passage = PassageDigest::of(&tokens)?;
    let found = warmup.contextual(&passage, index, WarmupAccess::Foreground, || {
        lookup_in_context_uncached(&state, &tokens, index)
    })?;
    ownership.checkpoint().map_err(|error| error.to_string())?;
    Ok(found)
}

fn lookup_in_context_uncached(
    state: &DictionaryState,
    tokens: &[Token],
    index: usize,
) -> Result<sakatsu_core::dict::ContextualWordView, String> {
    // Resolved once and lent, which is the whole of what the shell still owns here. The policy —
    // which spelling is queried, the fallback to a joined token's longest part, whether a promoted
    // source was reached by spelling or by a katakana fold — is `core::lookup`'s.
    let open = state.0.now();
    sakatsu_core::lookup::in_context(tokens, index, &open.dictionaries)
        .map_err(|error| error.to_string())
}

/// Prepare nearby Analysis answers without selecting or marking any word.
///
/// Indices are processed in the UI's reading-priority order by one speculative worker. A newer
/// request supersedes whatever remains in an older list; the item already reading from disk is
/// allowed to finish and becomes a useful cache entry rather than being discarded halfway.
#[tauri::command(async)]
pub fn prefetch_analysis(
    state: State<'_, DictionaryState>,
    warmup: State<'_, WarmupState>,
    tokens: Vec<Token>,
    indices: Vec<usize>,
    request: Option<RequestIdentity>,
) -> Result<usize, String> {
    let ownership = crate::work::scheduler()
        .own(request)
        .map_err(|error| error.to_string())?;
    let intent = warmup.begin_analysis_intent();
    let _only_speculation = warmup.prefetch_guard();
    // Once for the whole list. The passage does not change between indices, and digesting it per
    // index was the bulk of what this command cost.
    let passage = PassageDigest::of(&tokens)?;
    let mut warmed = 0;
    let mut seen = HashSet::new();
    for index in indices {
        if ownership.checkpoint().is_err() || !warmup.analysis_intent_is_current(intent) {
            break;
        }
        if index >= tokens.len() || !seen.insert(index) {
            continue;
        }
        // A speculative lookup that fails is skipped, not propagated. `?` here rejected the whole
        // command over one bad token - abandoning every remaining neighbour that would have
        // succeeded - and the UI's `.catch(() => {})` swallowed the reason, so a token that failed
        // consistently disabled prefetch for that passage with nothing anywhere to say why.
        // Nothing is lost by carrying on: the real lookup runs again when the word is selected,
        // and reports its error then, to someone who is waiting for it.
        match warmup.contextual(&passage, index, WarmupAccess::Prefetch, || {
            lookup_in_context_uncached(&state, &tokens, index)
        }) {
            Ok(_) => warmed += 1,
            Err(error) => {
                sakatsu_core::log_warn!("warmup", "prefetch skipped token {index}: {error}");
            }
        }
    }
    Ok(warmed)
}

/// The kanji table, from `scripts/fetch-kanjidic.ps1`.
///
/// Empty until it has been fetched, which is a supported state: the reference page says
/// how to get it rather than showing an empty section.
/// The frequency list that orders search results, when one is installed.
///
/// `None` is the ordinary state and not a degraded one: nothing ships a corpus, and the
/// interface says results are in the dictionary's order rather than pretending to a
/// ranking it cannot make.
pub struct FrequencyState(
    pub std::sync::RwLock<Option<sakatsu_core::dict::frequency::Frequencies>>,
);

impl FrequencyState {
    /// Load the list the settings name, from `dictionaries/frequency/<id>`.
    ///
    /// Behind a lock so it can be swapped while the app runs. It used to be plain, which
    /// meant changing the setting saved a value nothing read until the next launch —
    /// indistinguishable, from the reader's side, from the setting not working.
    ///
    /// `RwLock` rather than `Mutex`: searches only read it, and two of them have no reason
    /// to wait for each other.
    pub fn load(
        root: PathBuf,
        chosen: &str,
        direction: sakatsu_core::dict::frequency::Direction,
    ) -> FrequencyState {
        if chosen.is_empty() {
            return FrequencyState(std::sync::RwLock::new(None));
        }
        let dir = root.join("frequency").join(chosen);
        if !dir.is_dir() {
            return FrequencyState(std::sync::RwLock::new(None));
        }
        // A list the app fetched itself is read the way it is actually written, whatever the
        // setting says. The setting is global and both offered lists are ranks, so a stray
        // `count` reversed them outright - see `frequency::shipped_direction`.
        let direction =
            sakatsu_core::dict::frequency::shipped_direction(chosen).unwrap_or(direction);
        let loaded = sakatsu_core::dict::frequency::Frequencies::load(&dir, direction);
        // An empty list is no list. Reporting one by name while ordering nothing would be
        // the interface claiming a ranking that is not happening.
        FrequencyState(std::sync::RwLock::new(
            (!loaded.is_empty()).then_some(loaded),
        ))
    }
}

/// Read a different frequency list, without restarting.
///
/// The lists are big — BCCWJ is a million rows — so this is `async` and runs off the
/// command thread. Silent on success: the next search carries the corpus's name back in
/// `Found::ranked_by`, which is where the interface says what ordered the results.
#[tauri::command(async)]
pub fn reload_frequency<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, FrequencyState>,
    source: String,
    direction: sakatsu_core::dict::frequency::Direction,
) -> Result<Option<String>, String> {
    let fresh = FrequencyState::load(crate::dictionaries_dir(), &source, direction);
    let loaded = fresh
        .0
        .into_inner()
        .map_err(|_| "frequency lock poisoned")?;
    let name = loaded.as_ref().map(|f| f.name.clone());
    sakatsu_core::log_info!(
        "session",
        "frequency reloaded: {}",
        name.clone().unwrap_or_else(|| "none".into())
    );
    *state.0.write().map_err(|_| "frequency lock poisoned")? = loaded;
    if let Some(warmup) = app.try_state::<WarmupState>() {
        warmup.invalidate();
    }
    Ok(name)
}

/// Switch analyzer without restarting.
///
/// The setting used to be read once, at startup, and the interface said as much — "changing this
/// needs a restart", on the row of a dropdown that had already changed. Nothing about the
/// analyzer required that: every command takes it by reference and hands it back, which is the
/// shape a swap is trivial for.
///
/// `async` because choosing is not free. `choose_analyzer` settles Automatic by *asking Ichiran
/// to analyse 水* — the only honest test of a sidecar that needs WSL, Docker and PostgreSQL
/// behind it — and on a cold WSL that is seconds. On the command thread those are seconds with
/// the window frozen.
///
/// Returns the engine that is now live, which is not always the one asked for: Automatic falls
/// back to the built-in analyzer when Ichiran does not answer, and the interface says which it
/// landed on rather than leaving the reader to guess from the results.
#[tauri::command(async)]
pub fn reload_analyzer<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AnalyzerState>,
    choice: sakatsu_core::store::AnalyzerChoice,
) -> Result<String, String> {
    let was = state.now().engine().to_string();
    let built_in = app.state::<BuiltInAnalyzerState>().0.clone();
    // **Published unconditionally, with a new generation**: this is the reader's own selection,
    // and any probe that started before it finds its answer dropped.
    let fresh = crate::probe_route(crate::route::effective_policy(choice), built_in);
    let engine = fresh.primary.engine().to_string();
    state.publish(fresh);
    if let Some(warmup) = app.try_state::<WarmupState>() {
        warmup.invalidate();
    }

    // The cache the old engine filled is not thrown away, and does not need to be: `Remembering`
    // records which engine wrote it and refuses a file another engine left. Switching to Lindera
    // and back finds the Ichiran answers still there.
    sakatsu_core::log_info!(
        "analyzer",
        "switched from {was} to {engine} without a restart"
    );
    let _ = app.emit(RELOADED, "analyzer");
    Ok(engine)
}

/// Which engine is running right now.
///
/// Asked on load so the settings row can say what Automatic resolved to. It reads the name off
/// the analyzer already chosen and does **not** probe anything — `readiness` is the command that
/// actually runs 水 through Ichiran, and doing that here would put a WSL spawn in the path of
/// opening the settings page.
#[tauri::command]
pub fn analyzer_engine(state: State<'_, AnalyzerState>) -> String {
    state.now().engine().to_string()
}

pub struct KanjiState(pub Live<sakatsu_core::dict::KanjiDic>);

impl KanjiState {
    pub fn load(path: PathBuf) -> KanjiState {
        KanjiState(Live::new(Arc::new(sakatsu_core::dict::KanjiDic::load(
            &path,
        ))))
    }

    /// Read the table again, after `scripts/fetch-kanjidic.ps1` or the in-app fetch built it.
    pub fn reload(&self) {
        self.0.set(Arc::new(sakatsu_core::dict::KanjiDic::load(
            &crate::kanji_dir().join("kanjidic.tsv"),
        )));
    }
}

/// Look one character up directly, without requiring it to occur in a dictionary word.
///
/// A single-kanji query is a direct question about the character. Requiring a word result first
/// made a fetched KANJIDIC entry unreachable for characters the word dictionaries did not happen
/// to lead with. No reading is marked `here`: there is no containing word whose pronunciation can
/// establish one.
#[tauri::command]
pub fn lookup_kanji(state: State<'_, KanjiState>, character: String) -> Option<KanjiView> {
    let mut chars = character.trim().chars();
    let character = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let dic = state.0.now();
    dic.get(character)
        .map(|kanji| sakatsu_core::search::kanji_view(character, kanji, None, false))
}

/// A compact character in a component's related row.
#[derive(Debug, Serialize)]
pub struct RelatedKanji {
    pub character: String,
    pub meanings: Vec<String>,
    pub strokes: u8,
    pub grade: Option<u8>,
}

#[derive(Debug, Serialize)]
pub struct RelatedKanjiResult {
    pub component: String,
    pub total: usize,
    pub characters: Vec<RelatedKanji>,
}

/// One word written with a character, as its compound list shows it.
///
/// Straight off the dictionary entry rather than through `dict::merge`: a compound row shows a
/// spelling, a reading and a line of meaning, and merging every one of six words across every
/// installed dictionary to draw six lines would cost more than the page it draws. Choosing a
/// compound looks it up properly, through the same path any other result takes.
#[derive(Debug, Clone, Serialize)]
pub struct CompoundWord {
    pub headword: String,
    pub reading: String,
    pub glosses: Vec<String>,
}

/// The commonest words written with one character, and how many there are altogether.
#[derive(Debug, Clone, Serialize)]
pub struct CompoundsResult {
    pub character: String,
    /// Every entry written with it, not the number returned — so the page can say "6 of 1369".
    pub total: usize,
    pub words: Vec<CompoundWord>,
    /// False when no installed dictionary has the bucket. The page must say so rather than draw
    /// an empty list, which is indistinguishable from a character nothing uses — the same
    /// distinction `needs_gloss_index` draws for English search.
    pub indexed: bool,
}

/// The commonest words written with one character.
///
/// Lazy, like [`kanji_related`] beside it: a reader who never opens a character never pays for
/// this, which is the rule the Reference research sets for every heavy affordance.
#[tauri::command(async)]
pub fn kanji_compounds(
    state: State<'_, DictionaryState>,
    character: String,
    limit: usize,
) -> CompoundsResult {
    let open = state.0.now();
    let indexed = open
        .dictionaries
        .iter()
        .any(|dictionary| dictionary.has_kanji());
    let mut words = Vec::new();
    let mut total = 0usize;
    let mut seen: Vec<String> = Vec::new();

    for dictionary in open
        .dictionaries
        .iter()
        .filter(|dictionary| dictionary.has_kanji())
    {
        let Ok((hits, count)) = dictionary.compounds(&character, limit) else {
            continue;
        };
        total += count;
        for hit in hits {
            // One row per spelling. Two dictionaries holding 銀行 is one word to a reader.
            if seen.contains(&hit.entry.term) {
                continue;
            }
            seen.push(hit.entry.term.clone());
            words.push(CompoundWord {
                headword: hit.entry.term,
                reading: hit.entry.reading,
                glosses: hit
                    .entry
                    .definitions
                    .first()
                    .map(|definition| definition.glosses.clone())
                    .unwrap_or_default(),
            });
        }
    }
    words.truncate(limit);

    CompoundsResult {
        character,
        total,
        words,
        indexed,
    }
}

/// One sanitized, typed stroke diagram, loaded only when a character is expanded.
#[tauri::command(async)]
pub fn kanji_diagram(character: String) -> Option<sakatsu_core::dict::KanjiDiagram> {
    let mut chars = character.chars();
    let character = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    sakatsu_core::dict::load_diagram(&crate::kanji_dir().join("kanjivg"), character)
}

/// Characters sharing one visible KRADFILE component.
#[tauri::command(async)]
pub fn kanji_related(
    state: State<'_, KanjiState>,
    component: String,
    excluding: String,
    limit: usize,
) -> RelatedKanjiResult {
    let excluding = excluding.chars().next().unwrap_or('\0');
    let dic = state.0.now();
    let (total, characters) = dic.related(&component, excluding, limit);
    let characters = characters
        .into_iter()
        .filter_map(|character| {
            let kanji = dic.get(character)?;
            Some(RelatedKanji {
                character: character.to_string(),
                meanings: kanji.meanings.clone(),
                strokes: kanji.strokes,
                grade: kanji.grade,
            })
        })
        .collect();
    RelatedKanjiResult {
        component,
        total,
        characters,
    }
}

/// Search the dictionaries by hand — Japanese or English, one box.
///
/// The learner does not sort their query into buckets, so this does not make them:
///
/// - **Japanese** goes through the analyzer first, then the ordinary [`lookup`] path. That
///   is what makes 食べた find 食べる and 行かない find 行く, and it reuses the merge policy
///   rather than growing a second one.
/// - **English** goes to the gloss index. Each hit is merged with a synthetic token, which
///   is the same shape `lookup` already produces for every Lindera user — the analyzer's
///   senses are empty there too, and `merge` promotes and *names* the dictionary it took
///   the definition from.
#[tauri::command(async)]
pub fn search_words(
    analyzer: State<'_, AnalyzerState>,
    built_in: State<'_, BuiltInAnalyzerState>,
    state: State<'_, DictionaryState>,
    kanji: State<'_, KanjiState>,
    frequency: State<'_, FrequencyState>,
    warmup: State<'_, WarmupState>,
    query: String,
) -> Result<Found, String> {
    let query = query.trim().to_string();
    warmup.reference(&query, WarmupAccess::Foreground, || {
        search_words_uncached(
            &analyzer,
            &built_in,
            &state,
            &kanji,
            &frequency,
            query.clone(),
        )
    })
}

fn search_words_uncached(
    analyzer: &AnalyzerState,
    built_in: &BuiltInAnalyzerState,
    state: &DictionaryState,
    kanji: &KanjiState,
    frequency: &FrequencyState,
    query: String,
) -> Result<Found, String> {
    let preferred = analyzer.now();
    let dictionaries = state.0.now();
    let kanji = kanji.0.now();
    let frequency = frequency.0.read().ok();
    sakatsu_core::search::search(
        &query,
        sakatsu_core::search::Resources {
            preferred: preferred.as_ref(),
            built_in: built_in.0.as_ref(),
            dictionaries: &dictionaries.dictionaries,
            kanji: &kanji,
            frequency: frequency.as_ref().and_then(|value| value.as_ref()),
        },
    )
    .map_err(|error| error.to_string())
}

/// Warm the vocabulary half of one stable Reference query.
///
/// Grammar, terms and particle tables are compiled in and already answer immediately. This does
/// only the expensive half and returns no presentation data; the ordinary search remains the one
/// source of visible state and joins this computation when it arrives while warmup is in flight.
#[tauri::command(async)]
pub fn prefetch_reference(
    analyzer: State<'_, AnalyzerState>,
    built_in: State<'_, BuiltInAnalyzerState>,
    state: State<'_, DictionaryState>,
    kanji: State<'_, KanjiState>,
    frequency: State<'_, FrequencyState>,
    warmup: State<'_, WarmupState>,
    query: String,
) -> Result<bool, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(false);
    }
    let intent = warmup.begin_reference_intent();
    let _only_speculation = warmup.prefetch_guard();
    if !warmup.reference_intent_is_current(intent) {
        return Ok(false);
    }
    warmup.reference(&query, WarmupAccess::Prefetch, || {
        search_words_uncached(
            &analyzer,
            &built_in,
            &state,
            &kanji,
            &frequency,
            query.clone(),
        )
    })?;
    Ok(warmup.reference_intent_is_current(intent))
}

/// Session-only effectiveness counters for Settings → Diagnostics.
#[tauri::command]
pub fn warmup_diagnostics(state: State<'_, WarmupState>) -> WarmupDiagnostics {
    state.diagnostics()
}

/// What is running, and what the last recognition, analysis and translation each used.
///
/// The interface shows an operation's result only when its `request` is the one it owns: the
/// slot holds the latest *published* result, and the reader may have moved on since.
#[tauri::command(async)]
pub fn runtime_status(
    ocr: State<'_, OcrState>,
    analyzer: State<'_, AnalyzerState>,
    translator: State<'_, TranslatorState>,
) -> sakatsu_core::runtime_status::RuntimeStatus {
    crate::runtime_status::snapshot(&ocr, &analyzer, &translator)
}

/// How long an unload waits for work already using the engine before reporting it busy.
///
/// Long enough for a capture or a sentence to finish; short enough that a model still loading —
/// PaddleOCR-VL's server can take twenty seconds — gets a "try again" rather than a frozen button.
const UNLOAD_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// Release the recognition engine from memory. Files, settings and caches stay.
///
/// Holds the recognition gate for the whole unload: no capture or warm-up can start against the
/// engine being dropped, and one already running is waited for, briefly. Returns whether anything
/// was loaded to release.
#[tauri::command(async)]
pub fn unload_ocr<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    ocr: State<'_, OcrState>,
) -> Result<bool, String> {
    let _held = crate::lifecycle::gates()
        .exclusive(&[crate::lifecycle::Gate::Recognition], UNLOAD_WAIT)
        .map_err(|busy| busy.to_string())?;
    let released = ocr.unload();
    let _ = app.emit(
        OCR_ENGINE,
        OcrEngineState {
            name: ocr.chosen_name(),
            ready: false,
        },
    );
    Ok(released)
}

/// Release the translation model from memory. Its files and remembered translations stay.
#[tauri::command(async)]
pub fn unload_translator(translator: State<'_, TranslatorState>) -> Result<bool, String> {
    let _held = crate::lifecycle::gates()
        .exclusive(&[crate::lifecycle::Gate::Translation], UNLOAD_WAIT)
        .map_err(|busy| busy.to_string())?;
    Ok(translator.unload())
}

/// The text behind "Copy details for a bug report", for the reader to preview.
///
/// Built from a fixed list of facts — see `sakatsu_core::diagnostics` — never from the settings.
/// Copying it goes through [`copy_text`], so the clipboard watcher ignores the write.
#[tauri::command(async)]
pub fn diagnostic_report<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    ocr: State<'_, OcrState>,
    analyzer: State<'_, AnalyzerState>,
    translator: State<'_, TranslatorState>,
    store: State<'_, StoreState>,
) -> String {
    let status = crate::runtime_status::snapshot(&ocr, &analyzer, &translator);
    let setup = app
        .try_state::<crate::setup_manager::SetupState>()
        .map(|setup| crate::setup_manager::build_snapshot(&setup, &analyzer));
    let input = crate::runtime_status::report_input(status, setup.as_ref(), store.settings().log);
    sakatsu_core::diagnostics::report(&input)
}

/// Whether English search is available — see [`Found::needs_gloss_index`].
#[tauri::command(async)]
pub fn gloss_index_ready(state: State<'_, DictionaryState>) -> bool {
    let open = state.0.now();
    !open.dictionaries.is_empty()
        && open
            .dictionaries
            .iter()
            .any(sakatsu_core::dict::Dictionary::has_glosses)
}

/// Which dictionaries are missing an index, so the reader can be offered the repair.
///
/// The shape is [`sakatsu_core::dict::Upkeep`], declared in the generated contract rather than
/// here: the panel programs against it, and a type the generator does not know about is a type
/// the interface has to guess at.
///
/// **Not [`gloss_index_ready`].** That answers whether English search works, which is a different
/// capability and was the only one anything reported — see `Upkeep`'s own note for what that cost.
#[tauri::command(async)]
pub fn dictionary_upkeep(state: State<'_, DictionaryState>) -> Vec<sakatsu_core::dict::Upkeep> {
    state
        .0
        .now()
        .dictionaries
        .iter()
        .map(sakatsu_core::dict::Upkeep::of)
        .collect()
}

/// Build the English gloss index for dictionaries imported before it existed.
///
/// Background thread and an event, for the reason at the top of this file: it reads the
/// whole of `terms.ndjson` — about ten seconds for Jitendex's 433,885 entries — and doing
/// that on the command thread would freeze the window.
///
/// Reads the entries rather than the original zip, so **nothing is re-imported** and the
/// download is not needed.
///
/// **The open handles do not see it, and that is deliberate.** This comment used to say the
/// result was usable immediately because the index lands beside the dictionary a handle already
/// points at. It does not: the backfill publishes a *new generation*, and a `Dictionary` stays on
/// the one it resolved so a lookup in flight cannot have its bytes replaced underneath it. The
/// readers are therefore rebuilt below, before the session cache is dropped — see the comment at
/// that step, which has been right all along while this one was not.
#[tauri::command]
pub fn build_gloss_index<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Emitter;

    std::thread::spawn(move || {
        let started = std::time::Instant::now();

        // **Owned before the directory is resolved, and kept until the readers are rebuilt.** This
        // publishes a new generation of every dictionary it touches, which makes it one of the
        // operations that writes the resource tree — the easiest one to miss, because nothing
        // downloads and no directory is swapped. Claiming *after* working out where the
        // dictionaries are would let a move land in between and leave this indexing an abandoned
        // location; releasing before the reload would leave this session's handles on it.
        let payload = crate::owning([sakatsu_core::setup::claim::Claim::Dictionaries], |owned| {
            let dir = crate::dictionaries_dir();
            let outcome = sakatsu_core::setup::backfill::rebuild_gloss_indexes(&dir, owned);
            let built = outcome.built.clone();

            sakatsu_core::log_info!(
                "dictionary",
                "gloss index: {} in {:.1}s",
                outcome.summary(),
                started.elapsed().as_secs_f64()
            );

            // **Rebuild the readers, then drop the cached answers — in that order.**
            //
            // This used to invalidate the result cache alone, on the reasoning that "the
            // dictionaries themselves stay open and see their new files immediately". They do
            // not, and now they deliberately do not: the backfill publishes a **new
            // generation**, and a `Dictionary` stays on the one it resolved so a lookup in
            // flight cannot have its bytes replaced underneath it. A reader that is never
            // replaced therefore never sees the index, which is what `build_gloss_index`
            // exists to deliver.
            //
            // So the readers are rebuilt from disk first, exactly as `adopt` does after an
            // install, and only then is the session cache dropped — clearing it first would
            // let a query land between the two and cache another "index unavailable" answer.
            if !built.is_empty() {
                if let Some(state) = app.try_state::<DictionaryState>() {
                    let fresh = crate::discover_dictionaries(&dir);
                    state.0.set(fresh.0.now());
                }
                if let Some(warmup) = app.try_state::<WarmupState>() {
                    warmup.invalidate();
                }
            }

            match outcome.failed {
                None => serde_json::json!({ "ok": true, "detail": built.join(", ") }),
                Some(error) => serde_json::json!({ "ok": false, "detail": error }),
            }
        })
        .unwrap_or_else(|busy| {
            sakatsu_core::log_warn!("dictionary", "gloss index not started: {busy}");
            serde_json::json!({ "ok": false, "detail": busy })
        });
        let _ = app.emit("gloss-index-built", payload);
    });
}

// ---------------------------------------------------------------------------------------
// Setup: fetching the gigabyte the installer cannot carry
// ---------------------------------------------------------------------------------------

/// Downloads in flight, so they can be stopped.
///
/// Keyed by component id. A Cancel button that only hides the progress bar while 400 MB
/// keeps arriving is a lie, and on a metered connection an expensive one.
#[derive(Default)]
pub struct FetchState(pub Arc<Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>>);

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug, Serialize)]
pub struct IchiranSetupStarted {
    pub detail: String,
}

/// One visible installer at a time. The setup component is used on three screens and can
/// be unmounted while PowerShell is still working; without a process-level guard, opening
/// another screen could start a second database build against the same checkout.
static ICHIRAN_SETUP_RUNNING: AtomicBool = AtomicBool::new(false);

/// Open the bundled Ichiran setup in a real console and report when it closes.
///
/// A console is part of the interface here, not leakage from a subprocess: installing
/// WSL or Docker can require a UAC prompt, a restart, or a first-run agreement, and the
/// upstream image takes several minutes to build and restore. Keeping that work visible
/// is how a non-technical user can tell progress from a hang. The ordinary analyzer
/// subprocess remains hidden by the Quiet setting.
#[tauri::command(async)]
pub fn start_ichiran_setup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<IchiranSetupStarted, String> {
    let lifecycle = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Publication)
        .map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    {
        let _ = (app, lifecycle);
        return Err("Guided Ichiran setup is currently available on Windows only".into());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|error| format!("could not locate installed resources: {error}"))?;
        let installed = resource_dir.join("scripts").join("setup-ichiran.ps1");
        let script = if installed.is_file() {
            installed
        } else if cfg!(debug_assertions) {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("scripts")
                .join("setup-ichiran.ps1")
        } else {
            installed
        };
        if !script.is_file() {
            return Err(format!(
                "the bundled setup script is missing: {}",
                script.display()
            ));
        }

        let command_file = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("could not locate Sakatsu Reader's data: {error}"))?
            .join("ichiran-command.json");

        if ICHIRAN_SETUP_RUNNING.swap(true, Ordering::AcqRel) {
            return Ok(IchiranSetupStarted {
                detail: "The Ichiran setup window is already open. Leave it open while it works."
                    .into(),
            });
        }

        // A GUI process has no console to inherit. Ask Windows for a new one explicitly,
        // otherwise PowerShell can run invisibly — exactly the failed experience this
        // command replaces.
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .args(["-Install", "-Guided", "-CommandFile"])
            .arg(command_file)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                ICHIRAN_SETUP_RUNNING.store(false, Ordering::Release);
                return Err(format!("could not open PowerShell: {error}"));
            }
        };

        let events = app.clone();
        std::thread::spawn(move || {
            let _lifecycle = lifecycle;
            let payload = match child.wait() {
                Ok(status) if status.success() => serde_json::json!({
                    "ok": true,
                    "detail": "Ichiran setup completed. Choose Use Ichiran now to check it and start reading."
                }),
                Ok(status) => {
                    let detail = match status.code() {
                        Some(12) => "Restart Windows if requested, open Ubuntu once to finish its welcome screen, then choose Set up again.".to_string(),
                        Some(22) => "Open Docker Desktop once, finish its welcome screen, then choose Set up again.".to_string(),
                        Some(23) => "Enable this distribution under Docker Desktop > Settings > Resources > WSL Integration, then choose Set up again.".to_string(),
                        Some(30) => "The existing native Ichiran database is stopped. Start PostgreSQL in WSL, then choose Set up again.".to_string(),
                        Some(44) => "Ichiran is installed but its database did not become ready yet. Choose Set up again; existing data will be reused.".to_string(),
                        _ => format!(
                            "Setup stopped before completion ({}). The PowerShell window explains what is still needed.",
                            status
                        ),
                    };
                    serde_json::json!({ "ok": false, "detail": detail })
                }
                Err(error) => serde_json::json!({
                    "ok": false,
                    "detail": format!("Could not wait for the setup window: {error}")
                }),
            };
            ICHIRAN_SETUP_RUNNING.store(false, Ordering::Release);
            let _ = events.emit("ichiran-setup-finished", payload);
        });

        Ok(IchiranSetupStarted {
            detail: "Setup opened in PowerShell. Leave that window open while it works.".into(),
        })
    }
}

/// What the app can fetch, what it costs, and what is already here.
#[tauri::command(async)]
pub fn catalog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    analyzer: State<'_, AnalyzerState>,
) -> Vec<CatalogEntry> {
    let scripts = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("scripts"));

    sakatsu_core::setup::catalog::all()
        .iter()
        .map(|component| CatalogEntry {
            id: component.id.to_string(),
            label: component.label.to_string(),
            about: component.about.to_string(),
            licence: component.licence.to_string(),
            home: component.home.to_string(),
            sources: component
                .sources()
                .iter()
                .map(|source| CatalogSource {
                    name: source.name.to_string(),
                    licence: source.licence.to_string(),
                    home: source.home.to_string(),
                })
                .collect(),
            size: sakatsu_core::setup::human_size(component.bytes),
            bytes: component.bytes,
            tier: component.tier,
            needs_reload: component.needs_reload(),
            fetchable: component.fetchable(),
            script: component.script.and_then(|name| {
                scripts
                    .as_ref()
                    .map(|dir| dir.join(name).display().to_string())
            }),
            ready: installed(component.id, &analyzer),
            update_available: update_available(component.id),
        })
        .collect()
}

/// Whether a component is on disk.
///
/// **The disk, not the loaded state.** Lookup states are hot-reloaded after setup, but the disk
/// remains the durable answer before and after that handoff. Asking a transient state made a
/// freshly fetched component report missing while adoption was in flight, so the setup screen
/// offered the same large download again. `readiness` has always answered from the filesystem;
/// these two must not disagree about one component in the same session.
pub(crate) fn installed(id: &str, analyzer: &AnalyzerState) -> bool {
    match id {
        "dictionaries" => !dictionary_subdirectories(&crate::dictionaries_dir()).is_empty(),
        // **Both frequency lists were missing from this match entirely**, so they fell to the
        // `_ => false` below and reported not-installed for ever: the setup screen went on
        // offering "Get 6 MB" to somebody who already had the list on disk, with no way to tell
        // from the interface that the download had ever worked. Asked of the load root and with
        // the loader's own predicate, so "installed" here and "usable" there cannot diverge.
        "frequency-jpdb" => sakatsu_core::dict::frequency::is_present(
            &crate::dictionaries_dir().join("frequency").join("jpdb"),
        ),
        "frequency-bccwj" => sakatsu_core::dict::frequency::is_present(
            &crate::dictionaries_dir().join("frequency").join("bccwj"),
        ),
        "kanji" => crate::kanji_dir().join("kanjidic.tsv").is_file(),
        "pronunciation" => crate::audio_dir().join("index.tsv").is_file(),
        "wordnet" => crate::wordnet_dir().join("categories.tsv").is_file(),
        "manga-ocr" => MangaOcr::is_present(&crate::model_dir()),
        "paddle-ocr" => PaddleOcr::is_present(&crate::paddle_model_dir()),
        "paddle-vl" => crate::vlm_setup::installed(),
        "translation" => FuguMt::is_present(&crate::translation_model_dir()),
        // What the app actually chose at startup, which `choose_analyzer` already
        // established by asking Ichiran to analyse something. Asking again here would
        // spawn a WSL subprocess on every `catalog` call — on mount of both setup
        // screens and after every completed download — to learn what is already known,
        // and on a machine with no WSL distro that is seconds of cold start each time.
        "ichiran" => analyzer.now().engine() == "ichiran",
        // **Not a silent default.** Both frequency lists were absent from the arms above and
        // landed here, which reads as "not installed" and is indistinguishable from the truth:
        // the setup screen offered a download that had already succeeded, for ever, and nothing
        // anywhere said so. `catalog` calls this for every entry in `catalog::all()`, and
        // `fresh_install` invokes `catalog`, so in a debug build - which is what tests are - a
        // new component that forgets to add an arm fails a test instead of shipping a button
        // that never changes.
        _ => {
            debug_assert!(
                false,
                "installed() has no rule for catalog id `{id}`, so it will report                  not-installed however many times the download succeeds"
            );
            false
        }
    }
}

fn update_available(id: &str) -> bool {
    if id != "kanji" {
        return false;
    }
    let dir = crate::kanji_dir();
    dir.join("kanjidic.tsv").is_file()
        && (!dir.join("krad.tsv").is_file() || !dir.join("kanjivg").is_dir())
}

/// Where each component's files belong.
///
/// Deferred to the functions that already own this, environment overrides and all, rather
/// than restated in the catalog — `SAKATSU_MODEL_DIR` and friends have to keep working.
pub fn destination(id: &str) -> Option<PathBuf> {
    Some(match id {
        "dictionaries" => crate::dictionaries_install_dir(),
        // Under the dictionaries, in a `frequency/` of their own, and named by the value
        // `settings.frequency_source` holds — `FrequencyState::load` joins the two.
        "frequency-jpdb" => crate::dictionaries_install_dir()
            .join("frequency")
            .join("jpdb"),
        "frequency-bccwj" => crate::dictionaries_install_dir()
            .join("frequency")
            .join("bccwj"),
        "kanji" => crate::kanji_install_dir(),
        "pronunciation" => crate::audio_install_dir(),
        "wordnet" => crate::wordnet_install_dir(),
        "manga-ocr" => crate::model_install_dir(),
        "paddle-ocr" => crate::paddle_model_install_dir(),
        "paddle-vl" => crate::vlm_install_dir(),
        "translation" => crate::translation_model_install_dir(),
        _ => return None,
    })
}

/// Unpack a llama.cpp release zip, flat, into a directory this app owns.
///
/// **Flat, and entry names are taken apart rather than trusted.** A zip entry may name
/// `..\..\somewhere`, and joining that onto a destination writes outside it — the archive is
/// from a release this app pins, but "we fetched it from a trusted host" is exactly the
/// reasoning that makes a path-traversal bug survive review.
///
/// `enclosed_name` returns `None` for any name that would climb out, so such an entry is
/// **dropped rather than relocated**: filing it under its last component would put a file
/// somebody deliberately misnamed into the directory this app runs binaries from. Everything
/// else is flattened to its final component, which is all a llama.cpp release needs — the
/// server looks for its DLLs beside itself.
pub(crate) fn unpack_runtime(
    zip: &std::path::Path,
    into: &std::path::Path,
) -> Result<usize, String> {
    if into.exists() {
        std::fs::remove_dir_all(into).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(into).map_err(|error| error.to_string())?;

    let file = std::fs::File::open(zip).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let mut written = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry
            .enclosed_name()
            .and_then(|path| path.file_name().map(|name| name.to_os_string()))
        else {
            continue;
        };
        let mut out = std::fs::File::create(into.join(name)).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|error| error.to_string())?;
        written += 1;
    }

    if !into.join("llama-server.exe").is_file() {
        return Err("the downloaded runtime has no llama-server.exe in it".into());
    }
    Ok(written)
}

/// Where a component's downloaded archives go, under its own directory.
///
/// `work/`, which is what `scripts/fetch-*.ps1` use and what `core/tests/setup_build.rs`
/// looks for. Writing them beside the built table instead meant anyone who set up through
/// the app had the archives and the cross-check still skipped — quietly switching off the
/// test whose existence is the reason two implementations of those parsers is acceptable.
pub fn archives_in(component_dir: &std::path::Path) -> PathBuf {
    component_dir.join("work")
}

/// Fetch a component and make it usable.
///
/// Background thread and events, for the reason at the top of this file with more force
/// than anywhere else: this is minutes of work, and on the command thread the window
/// would be frozen for all of them.
///
/// A dictionary does not stop at the download. The zip is imported and the gloss index
/// built in the same run, because "downloaded but not imported" is a state that means
/// nothing to the person who pressed the button.
#[tauri::command]
pub fn fetch_component<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, FetchState>,
    id: String,
) -> bool {
    use tauri::Emitter;

    // Every path from here reports, including these. Returning silently left the UI
    // waiting on an event that would never come, and `busy` is bound to `disabled` on
    // every Get button in both screens — one unknown id disabled the whole interface.
    let give_up = |why: &str| {
        let _ = app.emit(
            "fetch-done",
            serde_json::json!({ "id": &id, "ok": false, "detail": why }),
        );
    };
    let Some(component) = sakatsu_core::setup::catalog::find(&id) else {
        give_up("nothing in the catalog is called that");
        return false;
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let jobs = Arc::clone(&state.0);
    let mut guarded = jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guarded.contains_key(&id) {
        // Do not emit `fetch-done`: that event belongs to the original job, and emitting
        // one for a duplicate click would make the UI hide progress while work continues.
        return false;
    }
    guarded.insert(id.clone(), Arc::clone(&cancel));
    drop(guarded);

    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let outcome = install_component(&app, component, &cancel, None, None);

        match &outcome {
            Ok(detail) => sakatsu_core::log_info!(
                "setup",
                "{} ready in {:.1}s — {detail}",
                component.id,
                started.elapsed().as_secs_f64()
            ),
            Err(error) => sakatsu_core::log_warn!("setup", "{} failed: {error}", component.id),
        }

        let payload = match outcome {
            Ok(detail) => serde_json::json!({ "id": component.id, "ok": true, "detail": detail }),
            Err(error) => {
                serde_json::json!({ "id": component.id, "ok": false, "detail": error })
            }
        };
        let mut guarded = jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if guarded
            .get(component.id)
            .is_some_and(|registered| Arc::ptr_eq(registered, &cancel))
        {
            guarded.remove(component.id);
        }
        drop(guarded);
        let _ = app.emit("fetch-done", payload);
    });
    true
}

/// Event: something the app had already loaded has been loaded again.
///
/// Carries what changed, so the interface can refresh the one view that depends on it rather
/// than everything. `Reference.svelte` redraws its dictionary list on `dictionaries`; the
/// About page recounts its credits.
pub const RELOADED: &str = "reloaded";

/// Take a freshly fetched component into the running app.
///
/// The counterpart to `Component::needs_reload`: that says *whether* something read at startup
/// has been replaced on disk, and this is what does the replacing. Both are derived from
/// `After` rather than listed by hand, so a fourth component cannot be added to one and
/// forgotten in the other.
///
/// Returns what was taken up, or `None` for the components loaded lazily anyway — a
/// recognition model is read on first use, so there is nothing here to refresh.
pub fn adopt<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    component: &'static sakatsu_core::setup::Component,
) -> Option<&'static str> {
    use sakatsu_core::setup::catalog::After;
    use tauri::Manager;

    let what = match component.after {
        After::ImportDictionary => {
            // Rebuilt from the directory rather than appended to: `discover_dictionaries` is
            // what decides lookup priority, and a new dictionary does not simply go last.
            let fresh = crate::discover_dictionaries(&crate::dictionaries_dir());
            app.try_state::<DictionaryState>()?.0.set(fresh.0.now());
            "dictionaries"
        }
        After::BuildKanjiDetails => {
            app.try_state::<KanjiState>()?.reload();
            "kanji"
        }
        After::BuildWordNetTable => {
            app.try_state::<ParticleState>()?.reload();
            "wordnet"
        }
        // A frequency list is already swapped in place by `reload_frequency` when the setting
        // that names it changes, and fetching one does not make it the chosen one.
        After::BuildAudioPack => {
            app.try_state::<AudioState>()?.0.reload();
            "pronunciation"
        }
        After::Nothing | After::UnpackFrequency | After::UnpackRuntime => return None,
    };

    if let Some(warmup) = app.try_state::<WarmupState>() {
        warmup.invalidate();
    }

    sakatsu_core::log_info!("setup", "{what} reloaded without a restart");
    let _ = app.emit(RELOADED, what);
    Some(what)
}

/// Own the component, work out where it goes, fetch it, and take it up — in that order.
///
/// # Why the order is the whole point
///
/// This used to be three steps in two places. `fetch_component` and the setup worker each resolved
/// `destination(id)` **before** spawning the work, `run_fetch` claimed the component once it was
/// running, and `adopt` ran **after** the claim had been dropped. Two windows, both real:
///
/// - a move landing between the resolution and the claim left the install publishing into a
///   directory nothing would look at again;
/// - a move landing between publication and adoption left this session's handles on the old
///   location, with the files correctly in the new one.
///
/// Neither shows up as two operations overlapping, which is why claiming alone did not close them.
/// So: claim, then resolve, then publish, then adopt, and only then release.
pub(crate) fn install_component<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    component: &'static sakatsu_core::setup::Component,
    cancel: &Arc<AtomicBool>,
    setup_generation: Option<u64>,
    on_phase: Option<&mut dyn FnMut(InstallPhase)>,
) -> Result<String, String> {
    // This guard covers resolution, transfer, publication and adoption. A second caller must not
    // attach to the same BITS bytes and then race the first caller through import or atomic
    // rename — and neither may run while the resources are being moved.
    crate::owning(
        [sakatsu_core::setup::claim::Claim::Component(component.id)],
        |_owned| {
            let into = destination(component.id).ok_or("there is nowhere to put it")?;
            let detail = run_fetch(app, component, &into, cancel, setup_generation, on_phase)?;

            // **Picked up before anyone is told the fetch finished.** This used to be the moment
            // the screen said "restart Sakatsu Reader to use it", which put the last step of an
            // operation the app had just spent minutes on into the reader's hands — and made a
            // successful download indistinguishable, until they did it, from one that had achieved
            // nothing. Nothing about these tables required a restart; they were simply read once
            // and held in a plain field. See `live.rs`.
            //
            // Order matters: `fetch-done` is what the interface waits on, so reloading after it
            // would leave a window where the screen says a dictionary is ready and a lookup in it
            // still finds nothing.
            adopt(app, component);
            Ok(detail)
        },
    )?
}

/// Transfer this component's artifacts and publish them. **Called under ownership**, never alone:
/// [`install_component`] is what claims, and it claims before it resolves `into`.
pub(crate) fn run_fetch<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    component: &'static sakatsu_core::setup::Component,
    into: &std::path::Path,
    cancel: &Arc<AtomicBool>,
    setup_generation: Option<u64>,
    on_phase: Option<&mut dyn FnMut(InstallPhase)>,
) -> Result<String, String> {
    run_fetch_with(
        app,
        component,
        into,
        cancel,
        setup_generation,
        on_phase,
        &mut transfer_artifact,
    )
}

#[doc(hidden)]
pub type TransferArtifact = dyn for<'a> FnMut(
    &str,
    &str,
    &std::path::Path,
    u64,
    sakatsu_core::setup::Verification<'a>,
    &Arc<AtomicBool>,
    &mut dyn FnMut(sakatsu_core::setup::Progress),
) -> Result<u64, String>;

/// The transfer orchestration with its byte source supplied by the caller.
///
/// Production supplies [`transfer_artifact`]. Keeping that boundary explicit lets the offline
/// regression drive the same event, verification and publication sequence through a controlled
/// loopback transport. It does not need to ask BITS or a production host to cooperate, and a red
/// test cannot accidentally become a real download.
#[doc(hidden)]
pub fn run_fetch_with<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    component: &'static sakatsu_core::setup::Component,
    into: &std::path::Path,
    cancel: &Arc<AtomicBool>,
    setup_generation: Option<u64>,
    on_phase: Option<&mut dyn FnMut(InstallPhase)>,
    transfer: &mut TransferArtifact,
) -> Result<String, String> {
    use tauri::Emitter;

    // Archives go in `work/`, which is where `scripts/fetch-*.ps1` put them and where
    // `core/tests/setup_build.rs` looks for them. Writing them beside the built table
    // instead meant anyone who set up through the app had the archive on disk and the
    // cross-check still printed "SKIP" and passed — silently disabling the test whose
    // existence is the reason duplicating the two parsers is acceptable at all.
    let work = archives_in(into);
    let count = component.artifacts.len();
    for (index, artifact) in component.artifacts.iter().enumerate() {
        let target = work.join(artifact.file);
        // **One value, two events.** These were two `json!` literals that had to be kept saying the
        // same thing about one transfer, and one of them already disagreed with the frontend
        // interface that mirrored it. `SetupProgress::of` derives the second from the first.
        let starting = SetupTransfer::starting(
            component.id,
            artifact.file,
            index + 1,
            count,
            Some(artifact.expected_bytes().unwrap_or(artifact.min_bytes)),
        );
        let _ = app.emit("fetch-progress", &starting);
        if let Some(generation) = setup_generation {
            let _ = app.emit("setup-progress", SetupProgress::of(generation, &starting));
        }

        let verification = sakatsu_core::setup::Verification {
            expected_bytes: artifact.expected_bytes(),
            sha256: artifact.sha256(),
        };
        let mut progress = |progress: sakatsu_core::setup::Progress| {
            let during =
                SetupTransfer::during(component.id, artifact.file, index + 1, count, &progress);
            let _ = app.emit("fetch-progress", &during);
            if let Some(generation) = setup_generation {
                let _ = app.emit("setup-progress", SetupProgress::of(generation, &during));
            }
        };
        transfer(
            component.label,
            artifact.url,
            &target,
            artifact.min_bytes,
            verification,
            cancel,
            &mut progress,
        )?;
    }

    install_staged(app, component, into, &work, cancel, on_phase)
}

/// Turn already-verified source artifacts into their live representation. Online setup and
/// offline essentials imports share this exact path, so fixes to parsing, health checks or
/// publication cannot land in one installer and miss the other.
pub(crate) fn install_staged<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    component: &'static sakatsu_core::setup::Component,
    into: &std::path::Path,
    work: &std::path::Path,
    cancel: &Arc<AtomicBool>,
    on_phase: Option<&mut dyn FnMut(InstallPhase)>,
) -> Result<String, String> {
    struct Hooks<'a, 'b, R: tauri::Runtime> {
        app: &'a tauri::AppHandle<R>,
        on_phase: Option<&'b mut dyn FnMut(InstallPhase)>,
    }
    impl<R: tauri::Runtime> sakatsu_core::setup::install::InstallHooks for Hooks<'_, '_, R> {
        fn detail(&mut self, id: &str, detail: &str) {
            say(self.app, id, detail);
        }
        fn phase(&mut self, phase: InstallPhase) {
            if let Some(report) = self.on_phase.as_deref_mut() {
                report(phase);
            }
        }
        fn verify_model(&mut self, id: &str, work: &std::path::Path) -> Result<(), String> {
            verify_raw_component(id, work)
        }
        fn install_runtime(&mut self, into: &std::path::Path) -> Result<String, String> {
            let zip = into.join("llama-vulkan.zip");
            let runtime = into.join(crate::vlm_setup::RUNTIME);
            let files = unpack_runtime(&zip, &runtime)?;
            // 95 MB of DLLs against a 33 MB zip, and nothing reads the zip again.
            let _ = std::fs::remove_file(&zip);

            crate::vlm_setup::write_marker(into)?;

            // **Time a read before calling it done.** The same weights run three orders of
            // magnitude apart on a graphics card and a processor, so whether this engine is
            // usable is a fact about the machine rather than about the model — and Settings
            // only offers it as the everyday reader once it knows which. Off the install
            // thread's critical path in the sense that failing to time it costs nothing:
            // untimed simply means the offer stays quiet.
            let timing = self.app.clone();
            std::thread::spawn(move || crate::vlm_setup::measure_first_read(&timing));

            Ok(format!("{files} runtime files, and the weights"))
        }
    }
    sakatsu_core::setup::install::install_staged(
        component,
        into,
        work,
        cancel,
        &mut Hooks { app, on_phase },
    )
}

fn transfer_artifact(
    label: &str,
    url: &str,
    target: &std::path::Path,
    min_bytes: u64,
    verification: sakatsu_core::setup::Verification<'_>,
    cancel: &Arc<AtomicBool>,
    on_progress: &mut dyn FnMut(sakatsu_core::setup::Progress),
) -> Result<u64, String> {
    #[cfg(windows)]
    {
        match crate::bits::download(
            &format!("Sakatsu Reader — {label}"),
            url,
            target,
            min_bytes,
            verification,
            cancel,
            on_progress,
        ) {
            Ok(bytes) => return Ok(bytes),
            Err(crate::bits::BitsError::Paused) => return Err("paused".into()),
            Err(error) => sakatsu_core::log_warn!(
                "setup",
                "{label}: {error}; continuing through resumable HTTP"
            ),
        }
    }

    sakatsu_core::setup::download_verified(
        url,
        target,
        min_bytes,
        verification,
        cancel,
        on_progress,
    )
    .map_err(|error| error.to_string())
}

/// Opening both ONNX sessions catches missing runtime DLLs, corrupt graphs and incomplete model
/// sets before setup says Recognition is ready. Live OCR corpus tests remain the accuracy check;
/// setup health is deliberately a structural check so a blank fixture cannot reject a model.
pub(crate) fn verify_raw_component(id: &str, destination: &std::path::Path) -> Result<(), String> {
    let fixture = ocr_health_fixture()?;
    match id {
        "manga-ocr" => MangaOcr::load(destination)
            .and_then(|engine| engine.recognize(&fixture).map(|_| ()))
            .map_err(|error| format!("manga-ocr health check failed: {error}")),
        "paddle-ocr" => PaddleOcr::open(destination)
            .and_then(|engine| engine.recognize(&fixture).map(|_| ()))
            .map_err(|error| format!("PP-OCRv6 health check failed: {error}")),
        _ => Ok(()),
    }
}

/// A tiny, copyright-free bitmap of 日本 compiled into the executable.
///
/// Session construction catches broken graphs, but only inference proves that ONNX Runtime,
/// tensor shapes and the model's execution provider work together on this machine. Monochrome
/// packed pixels keep the fixture below 2 KiB and avoid a separately extractable asset.
fn ocr_health_fixture() -> Result<OcrImage, String> {
    use base64::Engine;

    const WIDTH: u32 = 192;
    const HEIGHT: u32 = 80;
    const PACKED: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAfAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAPAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAPAAAAAAAAAAAAAAAAAAAAAP////AAAAAOAAAAAAAAAAAAAAAAAAAAAf////gAAAAOAAAAAAAAAAAAAAAAAAAAAf////gAAAAOAAAAAAAAAAAAAAAAAAAAAeAAAHgAAAAOAAAAAAAAAAAAAAAAAAAAAeAAAHAAAAAOAAAAAAAAAAAAAAAAAAAAAeAAAHAAAAAOAAAAAAAAAAAAAAAAAAAAAeAAAHAAAAAOAAAAAAAAAAAAAAAAAAAAAeAAAHAAf//////gAAAAAAAAAAAAAAAAAeAAAHAAf//////gAAAAAAAAAAAAAAAAAeAAAHAAf//////gAAAAAAAAAAAAAAAAAOAAAHAAAAA/gAAAAAAAAAAAAAAAAAAAAOAAAHAAAAB/wAAAAAAAAAAAAAAAAAAAAOAAAHAAAAB/wAAAAAAAAAAAAAAAAAAAAOAAAHAAAAD+4AAAAAAAAAAAAAAAAAAAAOAAAHAAAADu8AAAAAAAAAAAAAAAAAAAAOAAAHAAAAHucAAAAAAAAAAAAAAAAAAAAP////AAAAPOOAAAAAAAAAAAAAAAAAAAAP////AAAAOOPAAAAAAAAAAAAAAAAAAAAP////AAAAeOHgAAAAAAAAAAAAAAAAAAAOAAAHAAAA8ODwAAAAAAAAAAAAAAAAAAAOAAAHAAAB4OB4AAAAAAAAAAAAAAAAAAAOAAAHAAADwOB8AAAAAAAAAAAAAAAAAAAOAAAHAAAHgOA+AAAAAAAAAAAAAAAAAAAOAAAHAAAPgOAfAAAAAAAAAAAAAAAAAAAOAAAHAAAfAOAPgAAAAAAAAAAAAAAAAAAeAAAHAAA+AOAH4AAAAAAAAAAAAAAAAAAeAAAHAAD8AOAD8AAAAAAAAAAAAAAAAAAeAAAHAAH4AOAA/AAAAAAAAAAAAAAAAAAeAAAHAAfh///4fgAAAAAAAAAAAAAAAAAeAAAHAA/D///4PwAAAAAAAAAAAAAAAAAeAAAHAA+D///4DwAAAAAAAAAAAAAAAAAeAAAHAAYAAOAABgAAAAAAAAAAAAAAAAAeAAAHAAAAAOAAAAAAAAAAAAAAAAAAAAAf////gAAAAOAAAAAAAAAAAAAAAAAAAAAf////gAAAAOAAAAAAAAAAAAAAAAAAAAAf////gAAAAOAAAAAAAAAAAAAAAAAAAAAeAAAHgAAAAPAAAAAAAAAAAAAAAAAAAAAeAAAHgAAAAPAAAAAAAAAAAAAAAAAAAAAeAAAHAAAAAPAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAPAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let packed = base64::engine::general_purpose::STANDARD
        .decode(PACKED)
        .map_err(|error| error.to_string())?;
    let mut rgba = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for pixel in 0..(WIDTH * HEIGHT) as usize {
        let dark = packed
            .get(pixel / 8)
            .is_some_and(|byte| byte & (1 << (7 - pixel % 8)) != 0);
        let value = if dark { 0 } else { 255 };
        rgba.extend_from_slice(&[value, value, value, 255]);
    }
    OcrImage::new(WIDTH, HEIGHT, rgba).map_err(|error| error.to_string())
}

#[cfg(test)]
mod ocr_fixture_tests {
    use super::ocr_health_fixture;
    #[test]
    fn bundled_ocr_health_fixture_is_nonempty_and_well_formed() {
        let fixture = ocr_health_fixture().expect("fixture");
        assert_eq!((fixture.width(), fixture.height()), (192, 80));
        assert!(fixture
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] == [0, 0, 0]));
    }
}

/// A step that has no byte count — importing, indexing — still has to show it is alive.
///
/// Public so the command-boundary test can watch this emitter. Transfer-time events are covered
/// separately by `transfer_progress.rs`, which drives [`run_fetch_with`] through loopback HTTP.
pub fn say<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str, what: &str) {
    use tauri::Emitter;
    // One step of one, said out loud. The frontend was already substituting exactly that for the
    // fields this emission used to leave out.
    let _ = app.emit("fetch-progress", SetupTransfer::working(id, what));
}

/// Stop a download in progress.
#[tauri::command]
pub fn cancel_fetch(state: State<'_, FetchState>, id: String) {
    if let Some(flag) = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&id)
    {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Withdraw one UI subscriber from native work it no longer owns.
///
/// This never cancels setup or persistence: their operation identities do not enter these lanes.
/// Native inference already running may finish and populate a shared cache; its withdrawn caller
/// still fails the checkpoint and the frontend retains its final stale-result guard.
#[tauri::command]
pub fn cancel_request(request: RequestIdentity) {
    crate::work::scheduler().cancel(&request);
}

/// Explain a tag code such as `v5m` or `surname`.
///
/// The dictionary's own glossary, which expands a code into a name and no more. The
/// glossary in [`sakatsu_core::terms`] is what carries an actual explanation; this stays
/// because a dictionary's private codes — JMnedict's `surname` — are only in its own bank.
#[tauri::command]
pub fn explain_tag(state: State<'_, DictionaryState>, code: String) -> Option<String> {
    state.0.now().tags.explain(&code).map(ToString::to_string)
}

/// One glossary entry, by id or by any name it answers to.
///
/// Free of state: the glossary is a table compiled into the binary, so this works before
/// any dictionary is installed. That matters — the grammar panel explains forms on a fresh
/// install where nothing has been downloaded yet.
#[tauri::command]
pub fn explain_term(id: String) -> Option<terms::TermView> {
    terms::get_view(&id)
}

/// What a badge's tag means, for every tag on one word, in a single call.
///
/// Two glossaries asked in order: the built-in one, which explains what a code means
/// grammatically, then the dictionary's own bank, which is the only thing that knows its
/// private codes — JMnedict's `surname`.
///
/// **Only what can be explained is returned.** The interface uses the absence to decide
/// whether a chip is offered at all, which is why this answers for a list rather than one
/// at a time: a word's chips have to be drawn together, and a round trip each would be a
/// dozen for a page of definitions and a flicker on every one.
#[tauri::command]
pub fn explain_tags(
    state: State<'_, DictionaryState>,
    codes: Vec<String>,
) -> std::collections::HashMap<String, TagExplanation> {
    let open = state.0.now();
    codes
        .into_iter()
        .filter_map(|code| {
            let found = terms::get(&code)
                .map(|term| TagExplanation {
                    label: term.label.to_string(),
                    detail: term.detail.to_string(),
                })
                .or_else(|| {
                    open.tags.explain(&code).map(|note| TagExplanation {
                        // A bank note has no name of its own beyond the code itself.
                        label: code.clone(),
                        detail: note.to_string(),
                    })
                })?;
            Some((code, found))
        })
        .collect()
}

/// One tag explained. Owned, because a bank note is read at runtime rather than compiled in.
#[derive(Debug, Clone, Serialize)]
pub struct TagExplanation {
    pub label: String,
    pub detail: String,
}

/// Glossary entries a search should offer, best match first.
#[tauri::command]
pub fn search_terms(query: String) -> Vec<terms::TermView> {
    terms::search_views(&query)
}

/// The whole glossary, for browsing rather than searching.
#[tauri::command]
pub fn all_terms() -> Vec<terms::TermView> {
    terms::all_views()
}

/// The open dictionaries and their own attribution.
///
/// The UI shows this: both shipped dictionaries are CC BY-SA, which *requires*
/// attribution, so this is a licence obligation and not a nicety (`project notes` §8).
#[tauri::command]
pub fn dictionaries(state: State<'_, DictionaryState>) -> Vec<DictionaryMeta> {
    state
        .0
        .now()
        .dictionaries
        .iter()
        .map(|dictionary| dictionary.meta().clone())
        .collect()
}

/// One thing the About page credits.
#[derive(Debug, Serialize)]
pub struct CreditEntry {
    pub name: String,
    pub licence: String,
    pub url: String,
    pub why: String,
    /// The section this belongs under, already named — every label lives beside its variant in
    /// `credits::Group` so a rename cannot leave the page saying the old word.
    pub group: String,
}

/// What the app is built on, and the app's own version.
///
/// The **software** half, plus the grammar references under `Group::Consulted`, which are not
/// software at all. Every dictionary, model and frequency list carries its licence in the catalog
/// because the setup screen shows it before the download, so the About page reads `catalog` for
/// those rather than a second copy of the same strings. See `core/src/credits.rs`.
#[tauri::command]
pub fn credits() -> (String, Vec<CreditEntry>) {
    let entries = sakatsu_core::credits::all()
        .iter()
        // `Group::on_about` decides, not this list and not the page. Everything stays credited in
        // `credits::all` — which is what the manifest drift test reads — and the plumbing group is
        // simply not shown; see that method for why.
        .filter(|credit| credit.group.on_about())
        .map(|credit| CreditEntry {
            name: credit.name.to_string(),
            licence: credit.licence.to_string(),
            url: credit.url.to_string(),
            why: credit.why.to_string(),
            group: credit.group.label().to_string(),
        })
        .collect();
    (sakatsu_core::credits::VERSION.to_string(), entries)
}

/// Resolve a matched pattern's explanatory text from the bundled grammar packs.
#[tauri::command]
pub fn explain_pattern(id: String) -> Option<GrammarPattern> {
    grammar::pack::find(&id).cloned()
}

/// Look grammar up by hand, across both bundled packs.
///
/// The imported JLPT pack carries no match rules, so its several hundred entries are
/// only reachable this way — a reference shelf rather than something the engine spots
/// in a sentence.
#[tauri::command]
pub fn search_grammar(query: String) -> Vec<GrammarPattern> {
    grammar::pack::search(&query, 25)
        .into_iter()
        .cloned()
        .collect()
}

/// Grammar points explicitly reviewed as examples of one glossary concept.
#[tauri::command]
pub fn patterns_for_term(id: String) -> Vec<GrammarPattern> {
    grammar::pack::all()
        .into_iter()
        .flat_map(|pack| pack.patterns.iter())
        .filter(|pattern| pattern.term_ids.iter().any(|term_id| term_id == &id))
        .cloned()
        .collect()
}

/// One particle as the reference page shows it: no scores, no cues.
///
/// A view struct rather than `particles::ParticleEntry` itself, the way `TagExplanation`
/// is one. `Sense` carries the `Cue`s that rank it inside a sentence, and those are the
/// engine's workings — there is no sentence here to rank against, and sending them would
/// be sending the interface something it has no business drawing.
#[derive(Debug, Clone, Serialize)]
pub struct ParticleView {
    pub surface: String,
    /// Prose about the particle as a whole, above its senses. May be empty.
    pub note: String,
    pub senses: Vec<ParticleSenseView>,
}

/// One reading of a particle, with nothing said about how likely it is.
#[derive(Debug, Clone, Serialize)]
pub struct ParticleSenseView {
    pub id: String,
    pub label: String,
    pub gloss: String,
    /// The dictionary lines this reading answers to; empty when it answers to none.
    pub dictionary: Vec<String>,
    pub examples: Vec<grammar::pack::Example>,
}

/// Particles matching a search, best match first.
///
/// **Every sense, in the order they are written, with no favourite.** The ranking that
/// `particle_senses` does needs the words either side, and a search box has none — so
/// there is nothing to rank and saying so plainly is the honest answer. A reader asking
/// what に does is asking for the whole range, not for a guess at one of them.
#[tauri::command]
pub fn search_particles(query: String) -> Vec<ParticleView> {
    grammar::particles::search(&query)
        .into_iter()
        .map(|entry| ParticleView {
            surface: entry.surface.clone(),
            note: entry.note.clone(),
            senses: entry
                .senses
                .iter()
                .map(|sense| ParticleSenseView {
                    id: sense.id.clone(),
                    label: sense.label.clone(),
                    gloss: sense.gloss.clone(),
                    dictionary: sense.dictionary.clone(),
                    examples: sense.examples.clone(),
                })
                .collect(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Capture + OCR
// ---------------------------------------------------------------------------

/// Which engine is loaded, and whether it is loaded *yet*.
///
/// **Emitted rather than polled, because the answer is only interesting while it is expensive
/// to get.** [`OcrState::engine`] holds the load lock for as long as the model takes, and
/// PaddleOCR-VL starts the app-managed llama.cpp server and maps about 1.8 GB of model data first.
/// A command asking "are you ready?" would queue behind exactly the work it is asking about.
///
/// It doubles as the fix for an older bug: the interface read [`ocr_engine`] once at startup
/// and never again, so the Loaded row in Settings went on naming the engine the reader had
/// already switched away from.
pub const OCR_ENGINE: &str = "ocr-engine";

/// The payload of [`OCR_ENGINE`].
#[derive(Debug, Clone, Serialize)]
pub struct OcrEngineState {
    /// One of `auto`, `manga-ocr`, `pp-ocrv6`, `vlm`, `stub` — the same vocabulary
    /// [`ocr_engine`] returns, so one listener can keep that answer current.
    pub name: String,
    pub ready: bool,
}

/// Load the selected engine now, off the calling thread, if it is one worth pre-paying for.
///
/// **The wait does not get shorter, it gets moved.** Selecting PaddleOCR-VL used to cost
/// nothing until the first capture, which then spent up to twenty seconds starting a server
/// before it read anything — the one moment the reader is watching and waiting. Choosing an
/// engine is a deliberate act with a natural pause after it, so the load goes there instead.
///
/// Called from the two places an engine becomes the selected one: saving the settings, and
/// startup with a remembered choice. Neither may block — one is an IPC command answering the
/// settings panel, the other is the window opening — so both hand the work to a thread.
///
/// A capture arriving mid-warm is not a race: [`OcrState::engine`] holds `loaded` for the whole
/// load, so `read_frame` blocks on the warm already running and then uses it, rather than
/// starting a second one.
pub fn warm_ocr_in_the_background<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    warm_ocr(app, false);
}

/// Startup has an explicit loading surface, so prepare the selected OCR before capture.
pub fn warm_ocr_on_startup<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    warm_ocr(app, true);
}

fn warm_ocr<R: tauri::Runtime>(app: tauri::AppHandle<R>, startup: bool) {
    use tauri::Manager;
    let Some(ocr) = app.try_state::<OcrState>() else {
        return;
    };
    // Claim before spawning: the first status read must see queued work as loading too.
    if !ocr.begin_warming(startup) {
        return;
    }
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    std::thread::spawn(move || {
        use tauri::Manager;
        let Some(ocr) = app.try_state::<OcrState>() else {
            return;
        };
        let chosen = ocr.chosen_name();
        if chosen == "stub" {
            // Do not cache an unavailable engine before first-run setup installs its models.
            ocr.warming.store(false, Ordering::Release);
            return;
        }
        let lane = if chosen == "vlm" {
            crate::work::Lane::Vlm
        } else {
            crate::work::Lane::Ocr
        };
        let request = RequestIdentity {
            owner: "desktop-session".into(),
            operation: "ocr-warmup".into(),
            generation: GENERATION.fetch_add(1, Ordering::Relaxed) + 1,
            priority: WorkPriority::Speculative,
        };
        let Ok(_permit) = crate::work::scheduler().enter(lane, Some(request)) else {
            ocr.warming.store(false, Ordering::Release);
            return;
        };
        // Unloaded on purpose: nothing reloads it behind the reader's back.
        if ocr.held_back.load(Ordering::Acquire) {
            ocr.warming.store(false, Ordering::Release);
            return;
        }

        let started = std::time::Instant::now();
        let loaded = ocr.warmed_engine();
        ocr.warming.store(false, Ordering::Release);

        match &loaded {
            Ok(engine) => sakatsu_core::log_info!(
                "ocr",
                "warmed {} in {:.2}s before it was asked for",
                engine.engine(),
                started.elapsed().as_secs_f64()
            ),
            // Logged, not surfaced. Nobody asked for this load, so nobody is owed an error
            // dialog for it; the capture that does ask will fail in front of someone waiting
            // for it and can say so then.
            Err(error) => sakatsu_core::log_warn!("ocr", "could not warm the engine: {error}"),
        }

        // **Required, not decoration.** `read_frame` emits the readiness pair only when it
        // finds the engine cold, so after a successful warm it emits nothing at all — and the
        // hint, whose `ocrReady` starts false and moves only on this event, would spend the
        // whole of a fast read saying "Loading…".
        let _ = tauri::Emitter::emit(
            &app,
            OCR_ENGINE,
            OcrEngineState {
                name: match &loaded {
                    Ok(engine) => engine.engine().to_string(),
                    Err(_) => ocr.chosen_name(),
                },
                ready: loaded.is_ok(),
            },
        );
    });
}

/// The recognition engine, loaded on first use.
///
/// Startup prepares the selected engine in the background. Switching a conventional engine
/// later remains lazy, and an explicitly unloaded engine stays unloaded until requested.
pub struct OcrState {
    manga_dir: PathBuf,
    paddle_dir: PathBuf,
    /// Which engine to load. Changing it drops whatever is loaded, so the next capture
    /// picks the other one up — the models are hundreds of megabytes and holding both
    /// resident to save a few seconds on a rare switch is a poor trade.
    choice: Mutex<OcrChoice>,
    /// Everything about the engine that the user can change from the settings, held
    /// together because they invalidate the loaded engine the same way.
    tuning: Mutex<Tuning>,
    loaded: Mutex<Option<Arc<dyn Ocr>>>,
    /// Whether `loaded` holds an engine — readable **without** taking `loaded`.
    ///
    /// The word "without" is the whole point. [`OcrState::engine`] holds that mutex for the
    /// entire load, and for PaddleOCR-VL that can include starting the app-managed llama.cpp
    /// server and loading its weights. Anything that asks "is the engine up?" through the mutex is answered only once
    /// the answer has stopped mattering — including the hint whose entire job is to say what
    /// the wait is for.
    ///
    /// Written inside the `loaded` critical section, so the flag follows the mutex and never
    /// leads it. The two can disagree only between one lock being released and the next taken,
    /// and the worst that costs is a single wrong frame of a hint.
    ready: AtomicBool,
    /// Whether a background load is already in flight, so only one ever is.
    ///
    /// [`OcrState::engine`] is correct without this — it holds `loaded` for the whole load, so
    /// a second caller blocks and then gets the cached engine rather than starting a second
    /// one. What this prevents is the *pile-up*: every settings save during a twenty-second
    /// warm would otherwise spawn a thread that sits on that mutex for the rest of it.
    warming: AtomicBool,
    /// The last recognition this state answered, for the status display.
    last: crate::runtime_status::LastResult,
    /// Set when the reader unloads the engine: the warm-up leaves it unloaded until something
    /// explicitly asks for recognition, which clears it.
    held_back: AtomicBool,
    /// [`Settings::preload_ocr_on_startup`]. Off, no warm-up runs — at startup or after a
    /// settings change — and a capture is the only thing that loads an engine.
    preload: AtomicBool,
}

/// The parts of the recognition setup that come from settings rather than from disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Tuning {
    vlm_endpoint: String,
    vlm_model: String,
    preprocess: PreprocessOptions,
}

impl OcrState {
    /// Look for the models, loading nothing yet.
    pub fn new(manga_dir: PathBuf, paddle_dir: PathBuf, settings: &Settings) -> OcrState {
        OcrState {
            manga_dir,
            paddle_dir,
            choice: Mutex::new(settings.ocr),
            tuning: Mutex::new(tuning_from(settings)),
            loaded: Mutex::new(None),
            ready: AtomicBool::new(false),
            warming: AtomicBool::new(false),
            last: Default::default(),
            held_back: AtomicBool::new(false),
            preload: AtomicBool::new(settings.preload_ocr_on_startup),
        }
    }

    /// What to add to a failed read, when the configured endpoint explains the failure.
    ///
    /// Kept beside the settings that hold the endpoint rather than passed down to the engine,
    /// because the engine is a `dyn Ocr` that could be any of five things and only one of them
    /// has an endpoint at all.
    fn vlm_endpoint_advice(&self) -> Option<String> {
        if *self.choice.lock().ok()? != OcrChoice::Vlm {
            return None;
        }
        let endpoint = self.tuning.lock().ok()?.vlm_endpoint.clone();
        crate::vlm_setup::advice_for_unreachable(&endpoint)
    }

    /// Take the recognition settings. The next capture loads whatever they now describe.
    ///
    /// One entry point for all three, because all three invalidate the same thing: a
    /// different engine, a different endpoint, and different preparation each mean the
    /// object held here is no longer what the settings ask for. Splitting them into
    /// separate setters is how one of them ends up forgetting to drop the cache.
    pub fn configure(&self, settings: &Settings) {
        // Before anything is invalidated, so the warm-up the save starts next already sees it.
        self.preload
            .store(settings.preload_ocr_on_startup, Ordering::Release);
        let mut changed = false;

        {
            let mut current = self
                .choice
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *current != settings.ocr {
                *current = settings.ocr;
                changed = true;
            }
        }
        {
            let next = tuning_from(settings);
            let mut current = self
                .tuning
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *current != next {
                *current = next;
                changed = true;
            }
        }

        if changed {
            let mut loaded = self
                .loaded
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *loaded = None;
            // Inside the guard's scope, after the line above: the flag follows `loaded`.
            self.ready.store(false, Ordering::Relaxed);
        }
    }

    fn chosen(&self) -> OcrChoice {
        *self
            .choice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn tuning(&self) -> Tuning {
        self.tuning
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Use a specific engine — for tests, and for wiring a different one later.
    pub fn with_engine(engine: Arc<dyn Ocr>) -> OcrState {
        OcrState {
            manga_dir: PathBuf::new(),
            paddle_dir: PathBuf::new(),
            choice: Mutex::new(OcrChoice::Manga),
            tuning: Mutex::new(Tuning::default()),
            loaded: Mutex::new(Some(engine)),
            // **True, because this constructor is handed a loaded engine.** Starting it false
            // would leave every state built this way reporting itself permanently cold, and
            // `tests/capture_ipc.rs` builds one holding a live `StubOcr`.
            ready: AtomicBool::new(true),
            warming: AtomicBool::new(false),
            last: Default::default(),
            held_back: AtomicBool::new(false),
            preload: AtomicBool::new(true),
        }
    }

    /// Whether the engine is loaded — the one question that must be answerable *during* a load.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed) && !self.warming.load(Ordering::Acquire)
    }

    /// Outside startup, only a VLM selection is preloaded: its managed server startup is
    /// substantially longer than a conventional model load. Startup has its own explicit warm
    /// request; later conventional switches retain their lazy behavior. External endpoints
    /// only initialize a client and never start a process owned by someone else.
    fn wants_warming(&self) -> bool {
        self.chosen() == OcrChoice::Vlm
    }

    fn begin_warming(&self, startup: bool) -> bool {
        if !self.preload.load(Ordering::Acquire)
            || (!startup && !self.wants_warming())
            || self.is_ready()
            || self.held_back.load(Ordering::Acquire)
        {
            return false;
        }
        !self.warming.swap(true, Ordering::AcqRel)
    }

    /// Loaded engine if available, otherwise the selected one, without waiting on a model load.
    pub fn engine_name(&self) -> String {
        self.status().loaded.unwrap_or_else(|| self.chosen_name())
    }

    /// The name of the engine the settings ask for, touching only `choice`.
    ///
    /// The loading hint needs the selection even while the model lock is held. Status reads
    /// also use this directly so they cannot recurse through the loaded-engine readout.
    fn chosen_name(&self) -> String {
        match self.chosen() {
            OcrChoice::Auto if AutoOcr::is_present(&self.manga_dir, &self.paddle_dir) => {
                "auto".to_string()
            }
            OcrChoice::Manga if MangaOcr::is_present(&self.manga_dir) => "manga-ocr".to_string(),
            OcrChoice::Paddle if PaddleOcr::is_present(&self.paddle_dir) => {
                sakatsu_core::ocr::paddle::ENGINE.to_string()
            }
            // Deliberately not probed here. This is called to label the settings panel, and
            // reaching across the network to draw a label would stall the interface every
            // time it is asked — including while the endpoint is being typed.
            OcrChoice::Vlm => "vlm".to_string(),
            _ => "stub".to_string(),
        }
    }

    /// The last recognition this state answered.
    pub fn last(&self) -> &crate::runtime_status::LastResult {
        &self.last
    }

    /// Release the loaded engine from memory, keeping its files, the settings and every cache.
    ///
    /// **Call only with the recognition gate held** (`unload_ocr` does), so no capture or warm-up
    /// is using it. Stops the PaddleOCR-VL server only if this process started it: an endpoint
    /// somebody else runs is never touched. Returns whether anything was actually released.
    pub fn unload(&self) -> bool {
        let released = self
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .is_some();
        self.ready.store(false, Ordering::Relaxed);
        self.held_back.store(true, Ordering::Release);
        let stopped = crate::vlm_setup::stop_managed_runtime();
        released || stopped
    }

    /// The engine as the status display describes it, without waiting on a load in progress.
    ///
    /// `try_lock`, never `lock`: [`OcrState::engine`] holds `loaded` for the whole of a load —
    /// up to twenty seconds for PaddleOCR-VL — and a status read that waited for it would freeze
    /// the page it is drawn on. A held lock *is* the answer: something is loading.
    pub fn status(&self) -> sakatsu_core::runtime_status::EngineStatus {
        use sakatsu_core::runtime_status::{
            Acceleration, EngineRole, EngineState, EngineStatus, Hosting,
        };
        let configured = self.chosen_name();
        let hosting = if self.chosen() == OcrChoice::Vlm {
            let endpoint = self.tuning().vlm_endpoint;
            if crate::vlm_setup::is_managed_endpoint(&endpoint) {
                Hosting::ManagedLocal
            } else {
                Hosting::Endpoint {
                    external: !sakatsu_core::net::is_loopback(&endpoint),
                }
            }
        } else {
            Hosting::InProcess
        };
        let describe = |engine: Option<&Arc<dyn Ocr>>| match engine {
            _ if self.warming.load(Ordering::Acquire) => (EngineState::Loading, None),
            Some(engine) => (EngineState::Ready, Some(engine.engine().to_string())),
            None if configured == "stub" => (EngineState::NotInstalled, None),
            None => (EngineState::NotLoaded, None),
        };
        let (state, loaded) = match self.loaded.try_lock() {
            Ok(guard) => describe(guard.as_ref()),
            Err(std::sync::TryLockError::WouldBlock) => (EngineState::Loading, None),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                describe(poisoned.into_inner().as_ref())
            }
        };
        let acceleration = if state != EngineState::Ready {
            Acceleration::NotRunning
        } else {
            match hosting {
                Hosting::InProcess => sakatsu_core::onnx::acceleration(),
                Hosting::ManagedLocal => crate::vlm_setup::acceleration(),
                Hosting::Endpoint { .. } => Acceleration::Unknown,
            }
        };
        EngineStatus {
            role: EngineRole::Recognition,
            configured,
            loaded,
            hosting,
            state,
            acceleration,
        }
    }

    fn warmed_engine(&self) -> Result<Arc<dyn Ocr>, String> {
        let engine = self.engine()?;
        if let Err(error) = engine.warm() {
            self.discard_failed_warm(&engine);
            return Err(error.to_string());
        }
        Ok(engine)
    }

    fn discard_failed_warm(&self, failed: &Arc<dyn Ocr>) {
        let mut loaded = self
            .loaded
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // A newer settings choice or capture must not lose its engine to an old warmup.
        if loaded
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, failed))
        {
            *loaded = None;
            self.ready.store(false, Ordering::Release);
        }
    }

    /// The engine, loading it the first time.
    ///
    /// Returns an `Arc` so the lock is released before inference runs: recognition
    /// takes long enough that holding this would block every other command.
    fn engine(&self) -> Result<Arc<dyn Ocr>, String> {
        // Reached only by an explicit use: the warm-up returns before this while held back.
        self.held_back.store(false, Ordering::Release);
        let mut loaded = self
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(engine) = loaded.as_ref() {
            return Ok(Arc::clone(engine));
        }

        let tuning = self.tuning();
        let chosen = self.chosen();

        let engine: Arc<dyn Ocr> = if chosen == OcrChoice::Auto
            && AutoOcr::is_present(&self.manga_dir, &self.paddle_dir)
        {
            Arc::new(AutoOcr::new(
                self.manga_dir.clone(),
                self.paddle_dir.clone(),
            ))
        } else if chosen == OcrChoice::Paddle && PaddleOcr::is_present(&self.paddle_dir) {
            Arc::new(PaddleOcr::open(&self.paddle_dir).map_err(|error| error.to_string())?)
        } else if chosen == OcrChoice::Manga && MangaOcr::is_present(&self.manga_dir) {
            Arc::new(MangaOcr::load(&self.manga_dir).map_err(|error| error.to_string())?)
        } else if chosen == OcrChoice::Vlm {
            // Nothing to check for on disk: the model is on the other end of a URL.
            //
            // **Start the managed service rather than letting the capture fail on it.** This
            // used to note that an absent server "shows up as a refusal on the first read",
            // which was the honest description of an app that could not start one. It can now,
            // and a reader who chose this engine has already agreed to run it — being told
            // "is the server running?" about a service they never knew existed is the failure
            // the guided path was built to remove.
            //
            // A custom endpoint is left alone: that process is not this app's to launch.
            crate::vlm_setup::ensure_managed_runtime(
                &tuning.vlm_endpoint,
                std::time::Duration::from_secs(20),
            );
            Arc::new(
                VlmOcr::new(&tuning.vlm_endpoint, &tuning.vlm_model)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            // Says why rather than returning empty text.
            Arc::new(StubOcr)
        };

        // Preparation goes in front of whichever engine that was, never inside one: both
        // installed engines normalize to match their own training, and this must not
        // disturb that. See `sakatsu_core::ocr::preprocess`.
        let engine: Arc<dyn Ocr> = if tuning.preprocess.any() {
            Arc::new(Preprocess::new(engine, tuning.preprocess))
        } else {
            engine
        };

        *loaded = Some(Arc::clone(&engine));
        self.ready.store(true, Ordering::Relaxed);
        Ok(engine)
    }
}

fn tuning_from(settings: &Settings) -> Tuning {
    Tuning {
        vlm_endpoint: settings.vlm_endpoint.clone(),
        vlm_model: settings.vlm_model.clone(),
        preprocess: PreprocessOptions {
            tighten: settings.preprocess.tighten,
            contrast: settings.preprocess.contrast,
            upscale: settings.preprocess.upscale,
        },
    }
}

/// The label of the reader window — the one the app's lifetime is tied to.
///
/// **Closing it ends the session**, which `run()`'s window-event handler enforces. It has to,
/// because nothing else can: the capture frame is `decorations(false)` and `skip_taskbar(true)`,
/// so it has no close button and no taskbar entry, and no binding restores this window. Left to
/// Tauri's default — exit when the last window closes — an open frame kept the process alive with
/// no reader, no way to shut it down and an always-on-top overlay that could not be dismissed.
pub const MAIN_LABEL: &str = "main";

/// The label of the persistent capture frame.
pub const FRAME_LABEL: &str = "frame";

/// Emitted whenever the frame goes on or off screen, carrying which it now is.
///
/// **From `show_frame` and `hide_frame`, not from their callers.** Every way the frame
/// appears or disappears goes through one of those two — the button, both bindings, the
/// frame's own toolbar, switching the feature off — so emitting there covers all of them
/// by construction, where emitting per caller would cover the ones someone remembered.
///
/// The exception falls out for free: `read_frame` hides and re-shows the window around a
/// capture when `hide_during_capture` is set, and it does that with `window.hide()`
/// directly. That flicker is not a state change — the frame is up before and after — and
/// it does not reach here.
pub const FRAME_VISIBLE: &str = "frame-visible";

/// Emitted whenever the frame's tint changes, carrying the new value.
///
/// **Because the frame window outlives every change to it.** `close_frame` hides that window
/// rather than destroying it — see the note there — so its webview is built once, on the
/// first `show_frame` of the session, and `Frame.svelte` reads the tint once when it mounts.
/// Every later change to the setting had nowhere to arrive: the slider moved, the store was
/// written, and the overlay went on wearing whatever it was wearing at launch until the app
/// was restarted.
///
/// From `save_settings`, which is where the other settings that live outside the store are
/// pushed to whoever holds them, and for the same reason: it is the one path every change
/// takes, however it was made.
pub const FRAME_TINT: &str = "frame-tint";

/// Broadcast when the palette changes.
///
/// **The capture frame is a second document.** Writing custom properties onto the main window's
/// root never reaches it, and the frame's window is created once and only ever hidden — so it can
/// no more re-read this than it could re-read the tint. Same shape as [`FRAME_TINT`], for the same
/// reason, and `Frame.svelte` handles them side by side.
pub const THEME_CHANGED: &str = "theme-changed";

/// Put the window's own theme and its pre-paint colour in step with the chosen palette.
///
/// Two things the webview cannot do for itself:
///
/// 1. **The window theme.** `tauri.conf.json` used to pin this to `Dark`, which forces
///    `prefers-color-scheme` *inside* the webview to say dark whatever the desktop is set to — so
///    "follow the system" was not merely unimplemented, it was unobservable. Passing `None` hands
///    the question back to the OS; passing a theme states it.
/// 2. **The background colour.** That is what the window paints in the moment before the webview
///    draws anything. It was `#0a090c`, which is `--bg`'s dark value, so a light palette would
///    have opened near-black and flashed white a frame later.
pub fn apply_window_theme<R: tauri::Runtime>(app: &tauri::AppHandle<R>, theme: &Theme) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window(MAIN_LABEL) else {
        return;
    };

    let wanted = match theme.mode {
        ThemeMode::Light => Some(tauri::Theme::Light),
        ThemeMode::Dark => Some(tauri::Theme::Dark),
        ThemeMode::System => None,
    };
    let _ = window.set_theme(wanted);

    // Read back rather than assume: under `System` the answer is the desktop's, and it is the
    // only way to know which colour to pre-paint.
    let resolved = window.theme().unwrap_or(tauri::Theme::Dark);
    let colour = match resolved {
        tauri::Theme::Light => Color(0xf7, 0xf6, 0xf8, 0xff),
        _ => Color(0x0a, 0x09, 0x0c, 0xff),
    };
    let _ = window.set_background_color(Some(colour));
}

/// How much of the frame is chrome rather than region, in logical pixels.
///
/// The frame does not photograph itself because its interior is transparent — the desktop
/// composites the window beneath straight through it — and because what gets read is
/// inset past the parts that *are* painted. `FRAME_BORDER` is that inset, and it must agree
/// with `Frame.svelte`: too small and the frame's own border enters the OCR, too large and
/// text at the edge is silently cut off.
pub const FRAME_BORDER: f64 = 2.0;
/// The drag strip along the frame's top, in logical pixels.
///
/// **The frame has to be movable without a keybinding**, and the interior cannot be the
/// handle: it is what you look through, and a drag region there would swallow the clicks
/// click-through exists to pass on. So there is a strip, and a strip inside the frame is
/// paint — which means a capture must skip it, exactly as it once skipped the toolbar.
///
/// It is the same 28px the old toolbar occupied, and deliberately so: a strip you are meant
/// to grab has to be worth aiming at, and a hairline is not. What changed is what it holds —
/// a handle rather than a row of controls — so it costs the region the same and asks nothing
/// of it beyond being grabbable.
///
/// Must match `.grip`'s height in `Frame.svelte`: too small and the strip's own pixels enter
/// the OCR, too large and text under it is silently clipped.
pub const FRAME_GRIP: f64 = 28.0;

/// The smallest the capture frame may be, in logical pixels.
///
/// **Stated to the window manager, not only enforced here.** The frame was built with no
/// `min_inner_size` at all, and a window that declares no minimum gets the platform's own —
/// on Windows, well over a hundred pixels wide. That is what kept a folded frame, which needs
/// room for a grip and a chevron and nothing else, from getting small.
///
/// Both the builder and [`clamp_frame_size`] read these, so the window manager and this
/// app's own resize path cannot end up holding different beliefs about how small is small.
///
/// **These bound the window, and with `resizable(false)` they are the only bound there is.**
/// Windows no longer supplies a resize border, so no track-size minimum of its own applies to
/// a drag — every resize arrives through the handles and `clamp_frame_size`.
///
/// Width is below a column of vertical text, which is the narrowest thing anyone aims this
/// at. Height has to carry the drag strip on top of that.
pub const FRAME_MIN_WIDTH: f64 = 32.0;

/// The least interior the frame will leave to read, in logical pixels.
///
/// **This is the number that decides how small a line of text can be captured cleanly**, and the
/// only part of the height floor that is about reading rather than about chrome. The grip and the
/// two borders are fixed overhead on top of it, so the window's floor is always 32px taller than
/// whatever this says.
///
/// It was 24, which made the smallest frame 56px tall and left a single line of subtitle or dialogue
/// impossible to isolate: aiming at an 18px line meant taking 24px, and the extra six caught the
/// descenders of the line above or the tops of the one below — which is not a tidier capture but a
/// worse one, because the recogniser then has two partial lines to explain.
///
/// A floor on *aiming*, not a judgement about what will recognise. Below roughly this a frame is
/// hard to place at all, and thinner than any line anybody is reading; how well a given strip reads
/// is the recogniser's business and the learner can see the answer.
pub const FRAME_MIN_INTERIOR: f64 = 12.0;

/// Height has to carry the drag strip as well, or the smallest allowed frame is chrome with
/// nothing under it — which `the_clamp_floor_is_what_the_window_is_told` catches, and did.
///
/// Written as the sum it is, so that lowering [`FRAME_MIN_INTERIOR`] cannot leave the chrome
/// unaccounted for; `the_smallest_frame_leaves_exactly_the_interior_it_promises` pins the two
/// together against `interior_rect`, which does this arithmetic in the other direction.
pub const FRAME_MIN_HEIGHT: f64 = FRAME_GRIP + FRAME_BORDER * 2.0 + FRAME_MIN_INTERIOR;

/// How wide the resize handles are, matching `--thickness` in `Frame.svelte`.
///
/// Click-through needs it — it decides how far in from an edge the pointer still counts as
/// the frame's chrome rather than the hole you read through — and so does grabbing the edge
/// at all, now that `resizable(false)` has left these as the only resize targets.
///
/// **Eight, because that is what the main window has.** `SM_CXSIZEFRAME + SM_CXPADDEDBORDER`
/// is 8px, and `tao` keeps `WS_THICKFRAME` on the undecorated reader, so its rim is 8px and
/// its title bar below is clear of it. Matching the number is the whole point: the same
/// gesture should want the same aim in both windows.
///
/// It was widened to 14 when `resizable(false)` removed the native border, on the reasoning
/// that 8px alone is thin. **That fixed the wrong half.** The rim was not hard to hit because
/// it was narrow — it was hard to *avoid*, because it lay on top of the drag strip. A 14px
/// rim and 20px corners over a 28px grip left about 14px of it draggable, inset 20px at each
/// end. Widening the rim made that worse, not better.
///
/// What fixes it is the corners no longer dipping past the rim (see [`FRAME_CORNER`]), so the
/// grip keeps everything below 8px across its whole width. The strip gained drag area while
/// the chrome stayed exactly 28px — no taller frame, no thicker slab.
pub const FRAME_HANDLE: f64 = 8.0;

/// How big the corner handles are, matching `--corner` in `Frame.svelte`.
///
/// Bigger than [`FRAME_HANDLE`], and deliberately: a corner sizes both axes at once, so it
/// is the most useful target on the frame as well as the easiest to aim at.
///
/// **It has to be stated here too.** `over_chrome` decides where the pointer stops passing
/// through, and a handle drawn wider than that decision is a handle that does nothing: the
/// CSS put a resize target at the frame's bottom-left corner while the watcher still called
/// that spot region, so the press went to the application behind and the resize never
/// started. The top corners were saved by accident — the drag strip's band already covers
/// them — which is why it only ever went wrong at the bottom.
///
/// **Width only, now.** The corner squares are drawn [`FRAME_HANDLE`] tall rather than square,
/// so that none of them reaches below the rim into the drag strip; they stay 20px *wide*,
/// which is the axis that makes a corner easy to aim at along an edge. `over_chrome` still
/// counts the taller square as chrome, which is the safe direction it always was: a dead click
/// rather than a lost resize, and the top of the frame is grip there anyway.
///
/// `Frame.svelte` caps it with `min(20px, 30%)`, and [`over_chrome`] applies the same 30% cap
/// per axis. Letting the window count *more* as chrome than the CSS draws is normally the safe
/// direction - a dead click rather than a lost resize - but a corner is an `&&` across both
/// axes, so on a narrow frame an uncapped value made one half of that test true everywhere and
/// turned the corner into a plain band. Here the two have to agree.
pub const FRAME_CORNER: f64 = 20.0;

/// Whether the frame is letting the pointer through, and which watcher is current.
static CLICKTHROUGH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WATCHER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Whether the last `place_frame` was already sitting on the floor.
///
/// **The latch that keeps one line one line.** `Frame.svelte` clamps the requested rectangle
/// to the minimum before it calls, so once a drag reaches the floor every remaining
/// pointer-move asks for exactly the minimum again — a condition that is true hundreds of
/// times per gesture, not once. Logging it each time buries the line it was written to
/// surface. This flips on the way in and is cleared by the first request above the floor, so
/// the log gets the moment the limit was hit and nothing after it.
static AT_FLOOR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether a watcher thread is currently running.
///
/// Separate from `CLICKTHROUGH`, and the difference matters in one case: closing the frame
/// ends the watcher while the setting stays on, so "is it wanted" and "is it happening" are
/// genuinely two questions. Reopening asks the second and gets a new thread.
static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Let clicks reach whatever is behind the frame, without losing the frame.
///
/// `set_ignore_cursor_events` is per *window* and all or nothing, so switching it on would
/// take the toolbar, the close button and all eight resize handles with it — the way out
/// gone along with the way in, and a frame that can only be dismissed from another window.
///
/// So the pointer is watched, and the window ignores it only over the part you look through.
/// Over the toolbar or within a handle's width of an edge it goes solid again, which keeps
/// every control and both gestures working:
///
/// ```text
///   ┌──────────────────────────┐
///   │ 🔒 ▣ ✕   toolbar: solid  │
///   ├──────────────────────────┤
///   │        interior:         │  ← clicks pass through
///   │     clicks pass through  │
///   └──────────────────────────┘
///      edges: solid, so it still drags and resizes
/// ```
///
/// Polled, because a window ignoring the cursor is told nothing about it — there are no
/// enter or leave events to listen for once it is transparent to the mouse. The low-level
/// mouse hook is not available either: `binding::mouse` installs it only when a mouse
/// *binding* exists. `cursor_position` is what `center_frame_on_cursor` already uses.
///
/// The thread runs only while this is on, and retires the moment it is switched off or a
/// newer watcher starts — `WATCHER` is the generation, the same guard the overlay hold uses.
pub fn set_clickthrough<R: tauri::Runtime>(app: &tauri::AppHandle<R>, on: bool) {
    use std::sync::atomic::Ordering;

    CLICKTHROUGH.store(on, Ordering::SeqCst);
    let Some(window) = app.get_webview_window(FRAME_LABEL) else {
        return;
    };

    // Already on and already watched: nothing to do. `save_settings` calls this on every
    // save, so without it a run of saves would spawn a thread each — they retire correctly,
    // but starting one to immediately stop another is work nobody asked for.
    if on && WATCHING.load(Ordering::SeqCst) {
        return;
    }

    // Retire whatever was watching, either way: off means stop, on means this call owns it.
    let mine = WATCHER.fetch_add(1, Ordering::SeqCst) + 1;
    if !on {
        let _ = window.set_ignore_cursor_events(false);
        return;
    }

    let app = app.clone();
    WATCHING.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        // What the window was last told, so a decision that has not changed costs nothing.
        let mut ignoring: Option<bool> = None;
        while WATCHER.load(Ordering::SeqCst) == mine && CLICKTHROUGH.load(Ordering::SeqCst) {
            let Some(window) = app.get_webview_window(FRAME_LABEL) else {
                return; // the frame is gone; so is the reason to watch
            };
            let over_chrome = pointer_over_chrome(&app, &window).unwrap_or(true);
            if ignoring != Some(!over_chrome) {
                let _ = window.set_ignore_cursor_events(!over_chrome);
                ignoring = Some(!over_chrome);
            }
            // Fast enough that reaching for the close button feels immediate, slow enough
            // that this is not a busy loop. It only runs in this one mode.
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        // Only if this is still the current watcher — a newer one has taken over otherwise,
        // and saying "nobody is watching" would let a third start beside it.
        if WATCHER.load(Ordering::SeqCst) == mine {
            WATCHING.store(false, Ordering::SeqCst);
        }
        // Leaving the window able to see the pointer is the safe end state: a frame that
        // ignores clicks with nothing watching cannot be recovered from its own toolbar.
        if let Some(window) = app.get_webview_window(FRAME_LABEL) {
            let _ = window.set_ignore_cursor_events(false);
        }
    });
}

/// Whether the pointer is on the frame's chrome — its tab, or the region's resize rim.
///
/// `None` when the position cannot be read, and every caller treats that as "on the chrome":
/// a frame that stops ignoring the pointer is merely in the way, where one that ignores it
/// wrongly cannot be got hold of.
fn pointer_over_chrome<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    window: &tauri::WebviewWindow<R>,
) -> Option<bool> {
    let cursor = app.cursor_position().ok()?;
    let at = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    // The constants are logical; everything else here is physical.
    let scale = window.scale_factor().ok()?;

    Some(over_chrome(
        cursor.x - at.x as f64,
        cursor.y - at.y as f64,
        size.width as f64,
        size.height as f64,
        scale,
    ))
}

/// The geometry of the above, with no window to ask.
///
/// `x` and `y` are the pointer relative to the frame's top-left, everything in physical
/// pixels; the constants are logical, which is what `scale` is for. Separated out because
/// this is the part that can be quietly wrong — an edge off by a pixel, or a scale factor
/// forgotten — in a way that only shows up as a button that sometimes does not press.
fn over_chrome(x: f64, y: f64, width: f64, height: f64, scale: f64) -> bool {
    if x < 0.0 || y < 0.0 || x > width || y > height {
        return false; // not over the frame at all
    }
    // The rim, the corners and the drag strip. The frame carries no buttons any more — lock
    // and click-through live in the main window's header — so these three are the whole of
    // it, and everything else is what you look through.
    let handle = FRAME_HANDLE * scale;
    let grip = (FRAME_BORDER + FRAME_GRIP) * scale;
    // The corner squares are wider than the rim, so they need saying separately — see
    // [`FRAME_CORNER`]. Both axes, or this would be another edge rather than a corner.
    // **Capped at 30% per axis, exactly as `Frame.svelte` caps `--corner`.** Uncapped, a frame
    // narrower than 67 px got a corner wider than the one actually drawn - at the 32 px floor,
    // 20 px against a drawn 9.6 - so `x <= corner || x >= width - corner` was true for every x
    // and the corner test collapsed into "anywhere in the top or bottom 20 px". Clicks that
    // should have passed through a narrow frame were absorbed by a corner that was not there.
    //
    // Below 40 px on an axis the two zones would meet outright and the whole frame would count
    // as chrome; `FRAME_MIN_HEIGHT` (44) keeps height clear of that today, and the cap is what
    // stops it mattering if these constants ever move.
    //
    // Percentages resolve per axis in the CSS too: `left`/`right` against width, `top`/`bottom`
    // and the corner squares' own `height` against height.
    let corner_x = (FRAME_CORNER * scale).min(width * 0.30);
    let corner_y = (FRAME_CORNER * scale).min(height * 0.30);
    let in_corner =
        (x <= corner_x || x >= width - corner_x) && (y <= corner_y || y >= height - corner_y);

    y <= grip
        || x <= handle
        || y <= handle
        || x >= width - handle
        || y >= height - handle
        || in_corner
}

/// Turn click-through on or off now, rather than when the settings next save.
///
/// The frame's toolbar calls this and emits `frame-clickthrough` for the main window to save.
/// It does not write the setting itself: that file has one writer, and a second one is how a
/// setting goes missing — the same rule the lock follows.
#[tauri::command]
pub fn frame_clickthrough<R: tauri::Runtime>(app: tauri::AppHandle<R>, on: bool) {
    set_clickthrough(&app, on);
}

/// Where the frame sits, in logical desktop pixels. What gets remembered between runs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FrameRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for FrameRect {
    /// A landscape strip near the top of the screen: the shape of a line of manga text,
    /// and out of the way of what is usually being read.
    fn default() -> Self {
        FrameRect {
            x: 240.0,
            y: 160.0,
            width: 640.0,
            height: 200.0,
        }
    }
}

/// Whether the frame is on screen right now.
///
/// A plain function as well as a command, because the binding layer asks the same
/// question from outside the IPC boundary — tapping the show/hide key has to know
/// whether it is showing or hiding.
pub fn frame_is_visible<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    app.get_webview_window(FRAME_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// Hide the frame if it is there. Hidden, not closed, so showing it again is instant —
/// which matters when this runs on every hold of a key.
pub fn hide_frame<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(FRAME_LABEL) {
        let _ = window.hide();
        let _ = app.emit(FRAME_VISIBLE, false);
        crate::binding::set_capture_live(app, false);
    }
}

/// Put the frame over the mouse pointer, showing it if it was hidden.
///
/// This is what the show/hide binding does on a tap, and it is the point of the binding:
/// point at the text and tap, and the frame comes to you. Merely re-showing it wherever it
/// was left means dragging it every time, which is the same feature with the useful part
/// removed.
///
/// Everything here is in physical pixels — cursor, monitor and window all report them —
/// and the result is clamped to the monitor the pointer is on, so summoning the frame near
/// an edge does not push half of it off the screen.
pub fn center_frame_on_cursor<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    show_frame(app, None)?;

    let window = app
        .get_webview_window(FRAME_LABEL)
        .ok_or("the capture frame is not open")?;
    let cursor = app.cursor_position().map_err(|error| error.to_string())?;

    // The monitor under the pointer, not the primary one: the frame should arrive on the
    // screen being pointed at.
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .map_err(|error| error.to_string())?
        .or(app.primary_monitor().map_err(|error| error.to_string())?)
        .ok_or("no monitor under the cursor")?;

    let size = window.outer_size().map_err(|error| error.to_string())?;
    let (origin, bounds) = (monitor.position(), monitor.size());

    let clamp = |wanted: f64, start: i32, span: u32, extent: u32| {
        let last = i64::from(start) + i64::from(span) - i64::from(extent);
        (wanted.round() as i64).clamp(i64::from(start), last.max(i64::from(start))) as i32
    };
    let x = clamp(
        cursor.x - f64::from(size.width) / 2.0,
        origin.x,
        bounds.width,
        size.width,
    );
    let y = clamp(
        cursor.y - f64::from(size.height) / 2.0,
        origin.y,
        bounds.height,
        size.height,
    );

    crate::binding::trace(|| {
        format!(
            "cursor ({:.0},{:.0}) → frame {}x{} at ({x},{y}) on monitor ({},{}) {}x{}",
            cursor.x,
            cursor.y,
            size.width,
            size.height,
            origin.x,
            origin.y,
            bounds.width,
            bounds.height
        )
    });

    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

/// Close and reopen the app.
///
/// Several settings are read once at startup and cannot be changed under a running app:
/// which analyzer to use, and which frequency list orders search results. Those said
/// "takes effect when Sakatsu Reader restarts" and then left the restarting to whoever
/// read it — which, for the frequency list, looked exactly like the setting not working:
/// the choice saved, the ordering did not move, and nothing on screen connected the two.
///
/// Settings are already on disk before this runs. The interface saves on a debounce, so
/// the button asks for that to be flushed first rather than trusting the timing — a
/// restart that raced the save would come back to the old value and look like the setting
/// had been ignored twice over.
#[tauri::command]
pub fn restart_app<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    sakatsu_core::log_info!("session", "restarting at the reader's request");
    app.restart();
}

/// Switch the overlay feature off, or back on, and say which it now is.
///
/// The setting has otherwise only ever travelled outward — the interface owns it and
/// pushes it down. This is the one thing that changes it from below, so it does all three
/// jobs that keeps consistent: the atomic the bindings read, the store that outlives the
/// run, and an event so the button in the header changes colour to match.
pub fn switch_overlay<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, String> {
    let _write = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Store)
        .map_err(|e| e.to_string())?;

    let store = app
        .try_state::<StoreState>()
        .ok_or("settings are unavailable")?;
    let mut settings = store.settings();
    let now = !settings.overlay_enabled;
    settings.overlay_enabled = now;

    crate::binding::set_overlay_enabled(now);
    // And give the key back, or take it again. Without this the chord stayed registered with
    // the OS while `dispatch` ignored every press — the bound key dead in every application
    // and doing nothing in this one.
    crate::binding::set_overlay_live(app, now);
    if let Some(open) = store.store.as_ref() {
        open.set_settings(settings).map_err(|e| e.to_string())?;
    }
    // Off, the window goes too — the same rule the button follows, for the same reason.
    if !now {
        hide_frame(app);
    }
    let _ = app.emit("overlay-enabled", now);
    Ok(now)
}

/// Lock the capture frame where it is, or let it travel again, and say which it now is.
///
/// Reached by double-tapping the show/hide binding. The setting has otherwise only ever
/// travelled outward — the header's lock button owns it and the debounced save writes it — so
/// this does the two jobs that keeps consistent: the store that outlives the run, and an event
/// so the button changes state to match. Exactly the shape [`switch_overlay`] has, and for the
/// same reason: a setting with two writers and no way to hear the other one is a setting that
/// goes missing.
///
/// There is no atomic to push, unlike the switch: the binding layer reads `frame_locked` from
/// the store at the moment it needs it, which is once per tap on a thread that can afford it.
pub fn toggle_frame_lock<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, String> {
    let _write = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Store)
        .map_err(|e| e.to_string())?;

    let store = app
        .try_state::<StoreState>()
        .ok_or("settings are unavailable")?;
    let mut settings = store.settings();
    let now = !settings.frame_locked;
    settings.frame_locked = now;

    if let Some(open) = store.store.as_ref() {
        open.set_settings(settings).map_err(|e| e.to_string())?;
    }
    let _ = app.emit(FRAME_LOCKED, now);
    Ok(now)
}

/// Emitted when the frame's lock changes from below — a double tap on the show/hide binding.
///
/// The header's lock button is the other writer, and it cannot see this happen: it would go
/// on drawing an open shackle over a frame that no longer moves, which is the same class of
/// bug the overlay switch had before `overlay-enabled` existed.
pub const FRAME_LOCKED: &str = "frame-locked";

/// Show the frame where it was left, creating it the first time.
///
/// # An existing frame keeps its own geometry
///
/// **A remembered rect is only ever applied when the caller asks for one, or when the window is
/// being built.** A frame that already exists is the authoritative record of where it is and how
/// big — it was resized by dragging its own handles, and hiding a window does not make it forget.
/// The store is a *copy* of that, written on a debounce, and it is behind by however long the
/// debounce has left to run.
///
/// Re-imposing that copy on every show is what made a resize fail to stick. The show/hide binding
/// is the everyday way the frame comes and goes, and it passes `None` — so each tap read the
/// store and stretched the window back to whatever had last been written down. Resize, tap away,
/// tap back, and the frame returned at its old size: the resize had happened, the window knew it,
/// and showing it threw the knowledge away.
///
/// So an existing window is only shown. It is still *moved* by
/// [`center_frame_on_cursor`], which is the binding's other half and the thing that is actually
/// wanted from a tap — the frame comes to the pointer, at the size you gave it.
pub fn show_frame<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    rect: Option<FrameRect>,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FRAME_LABEL) {
        // Only what was asked for. `None` means "wherever it already is", not "wherever it was
        // when the settings were last written".
        if let Some(rect) = rect {
            window
                .set_position(tauri::LogicalPosition::new(rect.x, rect.y))
                .map_err(|error| error.to_string())?;
            window
                .set_size(tauri::LogicalSize::new(rect.width, rect.height))
                .map_err(|error| error.to_string())?;
        }
        window.show().map_err(|error| error.to_string())?;
        let _ = app.emit(FRAME_VISIBLE, true);
        crate::binding::set_capture_live(app, true);
        return Ok(());
    }

    // Nothing exists yet, so something has to say where it goes. The store is exactly right
    // here — this is the first show of the session and the window has no geometry of its own to
    // preserve. `unwrap_or_default` covers a profile that has never placed it; the builder's
    // `min_inner_size` turns those zeros into the floor rather than an invisible window.
    let rect = rect.unwrap_or_else(|| {
        app.try_state::<StoreState>()
            .and_then(|store| store.settings().frame_rect)
            .map(|[x, y, width, height]| FrameRect {
                x,
                y,
                width,
                height,
            })
            .unwrap_or_default()
    });

    tauri::WebviewWindowBuilder::new(
        app,
        FRAME_LABEL,
        tauri::WebviewUrl::App("index.html?frame=1".into()),
    )
    .title("屠つ — capture frame")
    .position(rect.x, rect.y)
    .inner_size(rect.width, rect.height)
    .transparent(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    // **Not resizable by Windows — resizable by us.**
    //
    // `decorations(false)` still leaves `WS_THICKFRAME` on, which puts an OS resize border at
    // the outermost pixels, outside the eight handles this frame draws. Dragging there never
    // reached `place_frame` at all: the OS sized the window, applied its own track-size
    // minimums, and our floor was never consulted. That is why the height stopped exactly at
    // `min_inner_size` while the width stopped at a number nothing in this codebase contains,
    // and why the resize log stayed silent through a session that plainly hit a limit.
    //
    // Turning this off does not stop the frame being resized. It stops *Windows* resizing it,
    // so every drag goes through the handles and `clamp_frame_size` — and our minimum becomes
    // the only one there is. `set_size` is unaffected: the flag governs user interaction, not
    // programmatic sizing. Dragging is unaffected too, because `data-tauri-drag-region` moves
    // the window through `WM_NCLBUTTONDOWN`, which wants no thick frame.
    .resizable(false)
    // Stated, because silence here is not "no minimum" — it is the platform's default one,
    // which on Windows is wide enough to stop a folded frame ever getting small.
    .min_inner_size(FRAME_MIN_WIDTH, FRAME_MIN_HEIGHT)
    // Deliberately not focused. The frame is summoned *while* reading something else, and
    // a window that steals focus every time would interrupt the thing being read.
    .focused(false)
    .build()
    .map_err(|error| format!("could not open the capture frame: {error}"))?;

    let _ = app.emit(FRAME_VISIBLE, true);
    crate::binding::set_capture_live(app, true);
    // A frame built just now has none of the saved state applied to it yet, and
    // `set_clickthrough` does nothing when there is no window to talk to — so a setting saved
    // before the frame existed would only take effect the next time something saved.
    let through = app
        .try_state::<StoreState>()
        .map(|store| store.settings().frame_clickthrough)
        .unwrap_or(false);
    set_clickthrough(app, through);
    Ok(())
}

/// Show the capture frame, creating it the first time.
#[tauri::command(async)]
pub fn open_frame<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    analyzer: State<'_, AnalyzerState>,
    rect: Option<FrameRect>,
) -> Result<(), String> {
    // **The earliest honest signal of intent.** Opening the capture frame means a read is
    // coming, and the seconds between that and the shutter are the only free ones there are:
    // the sidecar's session takes about five to wake, and paying it here rather than after the
    // capture is the whole difference between a hidden wait and none.
    analyzer.now().warm();
    crate::reprobe_if_due(&app);
    show_frame(&app, rect)
}

/// Hide the frame. Hidden rather than closed, so the next open is instant.
#[tauri::command(async)]
pub fn close_frame<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    hide_frame(&app);
}

/// Put the frame on screen if it is not, take it off if it is.
///
/// **The decision belongs here, not in the caller.** The note below explains why there is
/// no command for asking whether the frame is visible; the same reasoning is what puts the
/// answer and the action in one place. A caller that asked, then acted, would be holding a
/// stale answer across an IPC round trip, and the binding can show or hide the frame during
/// exactly that gap.
///
/// Takes the rect `open_frame` does, so the frame that comes back comes back where it was.
#[tauri::command(async)]
pub fn toggle_frame<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    rect: Option<FrameRect>,
) -> Result<(), String> {
    if frame_is_visible(&app) {
        hide_frame(&app);
        Ok(())
    } else {
        show_frame(&app, rect)
    }
}

// There is deliberately no command for "is the frame open".
//
// There was one, and conflating *exists* with *is visible* is exactly what let the capture
// binding resurrect an overlay that had just been put away: a hidden frame answered "not
// open", the UI read that as "there is none yet", and opened one — moving it to its stored
// position on the way. Nothing in the UI needs the answer now. Showing is unconditional and
// reading refuses on its own, in `read_frame`, which is the only place that can check it
// without a race anyway. `frame_is_visible` stays as a plain function for the binding layer.

/// Move and resize the frame, in logical pixels.
///
/// Sizing goes through a command rather than through `core:window:allow-set-size` because
/// the capability is scoped per window and there is no reason to hand the frontend a
/// general power over window geometry to get a resize handle.
#[tauri::command(async)]
pub fn place_frame<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    rect: FrameRect,
) -> Result<(), String> {
    let window = app
        .get_webview_window(FRAME_LABEL)
        .ok_or("the capture frame is not open")?;

    let (window_width, window_height) = clamp_frame_size(rect.width, rect.height);

    window
        .set_position(tauri::LogicalPosition::new(rect.x, rect.y))
        .map_err(|error| error.to_string())?;
    window
        .set_size(tauri::LogicalSize::new(window_width, window_height))
        .map_err(|error| error.to_string())?;

    // Say so when the window did not become what it was told to become.
    //
    // A resize drag calls this on every pointer-move, so logging each one would bury the one
    // line worth reading — hence only the disagreement. `set_size` returns `Ok` whether or
    // not the window manager honoured it, which is exactly how a floor can sit under this
    // code and be invisible from inside it. Reading the size back is the only way to see one.
    if let (Ok(got), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
        let (got_width, got_height) = (f64::from(got.width) / scale, f64::from(got.height) / scale);
        // Also when a resize *reaches* the floor, whether or not the window argued.
        //
        // Logging only disagreements answered one question and left the other open: if the
        // drag never asks for a small size, the window never has anything to refuse, and the
        // silence looks identical to success.
        //
        // **Once a gesture, which takes a latch and not just a condition.** The drag clamps
        // its own request to the minimum, so "is at the floor" stays true for every move that
        // follows — see [`AT_FLOOR`]. Only the arrival is worth a line.
        use std::sync::atomic::Ordering;
        let at_floor = rect.width <= FRAME_MIN_WIDTH || rect.height <= FRAME_MIN_HEIGHT;
        let reached_floor = at_floor && !AT_FLOOR.swap(true, Ordering::Relaxed);
        if !at_floor {
            AT_FLOOR.store(false, Ordering::Relaxed);
        }
        if reached_floor
            || (got_width - window_width).abs() > 1.0
            || (got_height - window_height).abs() > 1.0
        {
            sakatsu_core::log_warn!(
                "frame",
                "asked for {window_width:.0}x{window_height:.0}, got \
                 {got_width:.0}x{got_height:.0} (scale {scale}); the drag requested \
                 {:.0}x{:.0} against a floor of {FRAME_MIN_WIDTH:.0}x{FRAME_MIN_HEIGHT:.0}",
                rect.width,
                rect.height
            );
        }
    }
    Ok(())
}

/// Where the frame is now, in logical pixels, so the UI can remember it.
#[tauri::command(async)]
pub fn frame_rect<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<FrameRect, String> {
    let window = app
        .get_webview_window(FRAME_LABEL)
        .ok_or("the capture frame is not open")?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.inner_size().map_err(|e| e.to_string())?;

    Ok(FrameRect {
        x: f64::from(position.x) / scale,
        y: f64::from(position.y) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
    })
}

/// Read whatever is under the frame's interior.
///
/// The interior, not the window: the rect is inset past the border and toolbar, which are
/// the only parts of the frame that paint anything. Everything inside that inset is
/// transparent, so the desktop shows the window underneath and the capture sees it too.
///
/// Physical pixels here — the window reports logical ones, and a 150% display makes those
/// two disagree by half. `crate::capture` is where that conversion lives.
#[tauri::command(async)]
pub fn read_frame<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    ocr: State<'_, OcrState>,
    store: State<'_, StoreState>,
    analyzer: State<'_, AnalyzerState>,
    request: Option<RequestIdentity>,
) -> Result<String, String> {
    let window = app
        .get_webview_window(FRAME_LABEL)
        .ok_or("the capture frame is not open")?;

    // Hiding the frame pauses reading through it. Without this a capture after a hold
    // would read whatever happens to be under the frame's last position, which is not
    // something the learner asked for and not something they can see.
    if !window.is_visible().unwrap_or(false) {
        // The binding no longer reaches this — it stops at `binding::dispatch` and does
        // nothing at all. This is the backstop for the race it cannot cover: the frame
        // being hidden between the press and the read landing.
        sakatsu_core::log_warn!("capture", "read refused: the frame is hidden");
        return Err(
            "capture is off while the frame is hidden — tap the show/hide binding to bring it back"
                .into(),
        );
    }

    let lane = if ocr.chosen_name() == "vlm" {
        crate::work::Lane::Vlm
    } else {
        crate::work::Lane::Ocr
    };
    let identity = request.clone();
    let permit = crate::work::scheduler()
        .enter(lane, request)
        .map_err(|error| error.to_string())?;

    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.inner_size().map_err(|e| e.to_string())?;

    let region = interior_rect((position.x, position.y), (size.width, size.height), scale)
        .ok_or("the capture frame is too small to read")?;

    // The geometry, because this is where the subtle bugs live: an inset a pixel out puts
    // the frame's own border into the OCR or clips the text, and neither is an error —
    // only a worse read. Having the numbers written down is the difference between
    // guessing and checking.
    sakatsu_core::log_info!(
        "capture",
        "frame {}x{} at ({},{}) scale {scale} → region {}x{} at ({},{})",
        size.width,
        size.height,
        position.x,
        position.y,
        region.width,
        region.height,
        region.x,
        region.y
    );

    // Either trust the transparent interior, or stop arguing and get out of the shot.
    //
    // The frame's middle paints nothing, so the desktop should composite the window
    // beneath straight through it. "Should" is doing work in that sentence — it depends on
    // the compositor — so hiding is offered as the certain alternative, at the cost of a
    // flicker on every read.
    let image = if store.settings().hide_during_capture {
        let _ = window.hide();
        // One frame at 60Hz is 16ms; 120ms is what a compositor reliably needs to have
        // actually stopped drawing the window, and is what the reference tool uses.
        std::thread::sleep(std::time::Duration::from_millis(120));
        let captured = capture::capture_desktop_rect(region);
        let _ = window.show();
        captured?
    } else {
        capture::capture_desktop_rect(region)?
    };

    // Again here, because the overlay is not the only way in: the capture binding fires
    // without it, and a clipboard image never opens a frame at all. Cheap when the session is
    // already up — one guarded no-op — and this is the last moment before it is needed.
    analyzer.now().warm();
    crate::reprobe_if_due(&app);

    // **Say what the wait is before paying it.** A cold PaddleOCR-VL read spends its first
    // seconds starting a service, which from out here looks identical to a slow read — and the
    // two deserve different patience. `is_ready` is an atomic precisely so this question can be
    // asked while `engine()` holds the load lock.
    //
    // Warmed here rather than left to `recognize`, so "Reading…" can replace "Loading…" at the
    // moment inference actually starts. Emitting only afterwards would show one word for both
    // phases and never show the other at all.
    if !ocr.is_ready() {
        let _ = app.emit(
            OCR_ENGINE,
            OcrEngineState {
                name: ocr.chosen_name(),
                ready: false,
            },
        );
        let loaded = ocr.engine();
        let _ = app.emit(
            OCR_ENGINE,
            OcrEngineState {
                // Off the returned `Arc` rather than `engine_name()`: the same string, without
                // taking the lock a second time.
                name: match &loaded {
                    Ok(engine) => engine.engine().to_string(),
                    Err(_) => ocr.chosen_name(),
                },
                ready: loaded.is_ok(),
            },
        );
        // **Propagated, never swallowed.** A failed load caches nothing, so falling through
        // would send `recognize` back into `ensure_managed_runtime` for a second twenty-second
        // wait before reporting the same failure.
        loaded?;
    }

    // `engine()` caches, so the call inside this costs one uncontended mutex.
    permit.checkpoint().map_err(|error| error.to_string())?;
    let result = recognize(&ocr, &image, identity.as_ref());
    permit.checkpoint().map_err(|error| error.to_string())?;
    result
}

/// The frame's interior, in physical pixels: what actually gets read.
///
/// Separated from [`read_frame`] because this is the part that can be wrong without
/// anyone noticing. An inset that is one pixel too small puts the frame's own purple
/// border into the OCR, which then tries to read it; one too large silently clips the
/// text. Neither shows up as an error, only as worse recognition.
///
/// `position` and `size` arrive in physical pixels from the window; the insets are
/// logical, hence the scale. On a 150% display those differ by half, which is the whole
/// reason [`crate::capture`] keeps banging on about it.
/// The smallest the frame may be, for the resize drag to hold itself to.
///
/// **A drag has to know the floor, not just be corrected by it.** `place_frame` clamps the
/// size but cannot clamp the origin, because it is handed a rectangle and never learns which
/// edge moved. So dragging a top or left edge past the minimum used to leave the size pinned
/// while the origin kept travelling — the frame slid across the screen instead of refusing to
/// shrink, which looked like "it resizes and moves at the same time" and like "it will not go
/// any smaller".
///
/// Reported rather than restated in the UI, so there is still one source for these numbers.
#[derive(serde::Serialize)]
pub struct FrameLimits {
    pub min_width: f64,
    pub min_height: f64,
}

#[tauri::command]
pub fn frame_limits() -> FrameLimits {
    FrameLimits {
        min_width: FRAME_MIN_WIDTH,
        min_height: FRAME_MIN_HEIGHT,
    }
}

/// Hold a requested frame size to [`FRAME_MIN_WIDTH`] and [`FRAME_MIN_HEIGHT`].
///
/// A pure function so it can be tested — `place_frame` needs a live window and cannot be.
/// The same move `interior_rect` got, for the same reason.
fn clamp_frame_size(width: f64, height: f64) -> (f64, f64) {
    (width.max(FRAME_MIN_WIDTH), height.max(FRAME_MIN_HEIGHT))
}

/// The window size goes in and the readable interior comes out, so the arithmetic stays a
/// pure function the tests can drive at every scale.
///
/// **The result is a [`DesktopRect`], which is signed, and it used to be an `ocr::Rect`, which is
/// not.** A `.max(0)` here turned a frame on a monitor left of the primary into a region at x = 0
/// and the capture read the wrong screen — see `DesktopRect`'s own note. There is nothing to clamp
/// against at this point: a negative desktop coordinate is a perfectly ordinary place to be, and
/// deciding what is on-screen is the job of whichever monitor holds it.
fn interior_rect(position: (i32, i32), size: (u32, u32), scale: f64) -> Option<DesktopRect> {
    let border = (FRAME_BORDER * scale).round() as i32;
    // The top carries the drag strip as well as the border; every other side is border alone.
    let top = ((FRAME_BORDER + FRAME_GRIP) * scale).round() as i32;

    let width = size.0 as i32 - border * 2;
    let height = size.1 as i32 - top - border;
    if width <= 0 || height <= 0 {
        return None;
    }

    Some(DesktopRect {
        x: position.0 + border,
        y: position.1 + top,
        width: width as u32,
        height: height as u32,
    })
}

#[cfg(test)]
mod runtime_unpack_tests {
    use super::*;
    use std::io::Write as _;

    fn zip_with(entries: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sakatsu-unpack-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("runtime.zip");

        let file = std::fs::File::create(&path).expect("create");
        let mut writer = zip::ZipWriter::new(file);
        for (name, body) in entries {
            writer
                .start_file::<_, ()>(*name, zip::write::FileOptions::default())
                .expect("entry");
            writer.write_all(body).expect("write");
        }
        writer.finish().expect("finish");
        path
    }

    #[test]
    fn a_release_zip_is_unpacked_flat_and_keeps_its_server() {
        let zip = zip_with(&[
            ("llama-server.exe", b"server"),
            ("ggml-vulkan.dll", b"vulkan"),
            ("mtmd.dll", b"multimodal"),
        ]);
        let into = zip.parent().expect("parent").join("runtime");

        let written = unpack_runtime(&zip, &into).expect("unpack");
        assert_eq!(written, 3);
        for name in ["llama-server.exe", "ggml-vulkan.dll", "mtmd.dll"] {
            assert!(into.join(name).is_file(), "{name} should be there");
        }
        let _ = std::fs::remove_dir_all(zip.parent().expect("parent"));
    }

    /// **A zip entry is a name somebody else chose**, and joining it onto a destination is how
    /// an archive writes outside it. The release is pinned and fetched from a host this app
    /// trusts — which is exactly the reasoning that leaves this class of bug in place — so the
    /// name is taken apart rather than believed.
    #[test]
    fn an_entry_that_climbs_out_of_the_directory_is_dropped_entirely() {
        let zip = zip_with(&[
            ("llama-server.exe", b"server"),
            ("../../escaped.dll", b"nope"),
            ("nested/deep/inner.dll", b"flattened"),
        ]);
        let root = zip.parent().expect("parent").to_path_buf();
        let into = root.join("runtime");

        // Two of three: the climbing entry is skipped rather than relocated. That is the
        // stronger behaviour and worth pinning — quietly filing an entry somebody named with
        // a `..` into the directory this app runs binaries from would still be running it.
        let written = unpack_runtime(&zip, &into).expect("unpack");
        assert_eq!(written, 2);
        assert!(!into.join("escaped.dll").exists(), "not relocated");
        assert!(!root.join("escaped.dll").exists(), "must not climb out");
        assert!(
            !root.parent().expect("above").join("escaped.dll").exists(),
            "must not climb further out"
        );
        // Directories are flattened, because a llama.cpp release is flat and the server looks
        // for its DLLs beside itself.
        assert!(into.join("inner.dll").is_file());
        assert!(!into.join("nested").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// An archive without the server is not a runtime, however well it unpacked.
    ///
    /// Reported here rather than at the first capture, where it would arrive as a failed read
    /// on a component that had just said it installed successfully.
    #[test]
    fn a_zip_with_no_server_in_it_is_refused() {
        let zip = zip_with(&[("ggml-vulkan.dll", b"vulkan")]);
        let into = zip.parent().expect("parent").join("runtime");

        let error = unpack_runtime(&zip, &into).expect_err("should refuse");
        assert!(error.contains("llama-server.exe"), "{error}");
        let _ = std::fs::remove_dir_all(zip.parent().expect("parent"));
    }

    /// Re-running setup replaces the runtime rather than merging into it.
    ///
    /// A stale DLL left beside a newer server is the kind of mismatch that fails at load time
    /// with nothing useful to say.
    #[test]
    fn unpacking_again_clears_what_was_there_before() {
        let zip = zip_with(&[("llama-server.exe", b"server")]);
        let into = zip.parent().expect("parent").join("runtime");

        std::fs::create_dir_all(&into).expect("dir");
        std::fs::write(into.join("stale-from-an-older-build.dll"), b"old").expect("write");

        unpack_runtime(&zip, &into).expect("unpack");
        assert!(!into.join("stale-from-an-older-build.dll").exists());
        assert!(into.join("llama-server.exe").is_file());
        let _ = std::fs::remove_dir_all(zip.parent().expect("parent"));
    }
}

#[cfg(test)]
mod ocr_state_tests {
    use super::*;
    use sakatsu_core::ocr::OcrError;

    #[test]
    fn a_real_warm_failure_releases_the_cached_engine_for_retry() {
        struct Fails;
        impl Ocr for Fails {
            fn recognize(&self, _: &OcrImage) -> Result<String, OcrError> {
                panic!("warmup ran recognition")
            }
            fn engine(&self) -> &str {
                "warm-fails"
            }
            fn warm(&self) -> Result<(), OcrError> {
                Err(OcrError::Failed("warm fixture".into()))
            }
        }
        let state = OcrState::with_engine(Arc::new(Fails));
        assert!(state.warmed_engine().is_err());
        assert!(!state.is_ready());
        assert!(state.loaded.lock().unwrap().is_none());
    }

    #[test]
    fn a_loaded_router_is_not_ready_while_its_deferred_models_warm() {
        let state = OcrState::with_engine(Arc::new(StubOcr));
        state.warming.store(true, Ordering::Release);
        assert!(!state.is_ready());
        assert_eq!(
            state.status().state,
            sakatsu_core::runtime_status::EngineState::Loading
        );
        state.warming.store(false, Ordering::Release);
        assert!(state.is_ready());
    }

    #[test]
    fn failed_warmup_cannot_discard_a_newer_engine() {
        let state = OcrState::with_engine(Arc::new(StubOcr));
        let old: Arc<dyn Ocr> = Arc::new(StubOcr);
        state.discard_failed_warm(&old);
        assert!(state.is_ready());
        let current = state.engine().unwrap();
        state.discard_failed_warm(&current);
        assert!(!state.is_ready());
        assert!(state.loaded.lock().unwrap().is_none());
    }

    #[test]
    fn startup_warm_is_visible_before_a_worker_can_start_and_cannot_duplicate() {
        let state = cold();
        assert!(
            !state.begin_warming(false),
            "conventional settings changes remain lazy"
        );
        assert!(state.begin_warming(true));
        assert_eq!(
            state.status().state,
            sakatsu_core::runtime_status::EngineState::Loading
        );
        assert!(!state.begin_warming(true), "one worker owns the warmup");
    }

    #[test]
    fn startup_does_not_reload_an_explicitly_unloaded_or_ready_engine() {
        let state = cold();
        state.held_back.store(true, Ordering::Release);
        assert!(!state.begin_warming(true));
        let loaded = OcrState::with_engine(Arc::new(StubOcr));
        assert!(!loaded.begin_warming(true));
    }

    #[test]
    fn a_disabled_preload_refuses_every_warm_up_until_it_is_turned_back_on() {
        let off = Settings {
            preload_ocr_on_startup: false,
            ocr: OcrChoice::Vlm,
            ..Settings::default()
        };
        let state = OcrState::new(PathBuf::new(), PathBuf::new(), &off);
        assert!(!state.begin_warming(true), "no startup warm-up");
        assert!(
            !state.begin_warming(false),
            "no warm-up after a settings save, even for PaddleOCR-VL"
        );
        assert_eq!(
            state.status().state,
            sakatsu_core::runtime_status::EngineState::NotLoaded,
            "deferred, not loading"
        );
        state.configure(&Settings {
            preload_ocr_on_startup: true,
            ..off
        });
        assert!(state.begin_warming(false), "a save that turns it on applies it");
    }

    #[test]
    fn a_disabled_preload_still_loads_on_an_explicit_request() {
        let state = OcrState::new(
            PathBuf::new(),
            PathBuf::new(),
            &Settings {
                preload_ocr_on_startup: false,
                ..Settings::default()
            },
        );
        assert!(!state.begin_warming(true));
        state.engine().expect("a capture still loads the engine");
        assert!(state.is_ready());
    }

    /// A state over directories that hold nothing, so `engine()` falls through every
    /// `is_present` check to `StubOcr` — the same trick `tests/ipc.rs` uses.
    fn cold() -> OcrState {
        OcrState::new(PathBuf::new(), PathBuf::new(), &Settings::default())
    }

    #[test]
    fn a_new_state_is_cold_until_the_first_read_loads_something() {
        let state = cold();
        assert!(!state.is_ready(), "nothing has been loaded yet");
        state.engine().expect("the stub always loads");
        assert!(state.is_ready(), "a load has happened");
    }

    /// **The site the flag is easiest to forget.** `with_engine` is handed an engine rather
    /// than loading one, so a `false` here would leave every state built this way reporting
    /// itself permanently cold — and `tests/capture_ipc.rs` builds one holding a live stub.
    #[test]
    fn an_engine_handed_in_directly_is_already_loaded() {
        assert!(OcrState::with_engine(Arc::new(StubOcr)).is_ready());
    }

    #[test]
    fn changing_the_engine_makes_it_cold_again() {
        let state = cold();
        state.engine().expect("the stub always loads");

        state.configure(&Settings {
            ocr: OcrChoice::Paddle,
            ..Settings::default()
        });
        assert!(!state.is_ready(), "a different engine has to be loaded");
    }

    /// The other branch feeding the same `changed` flag. Preparation is not the engine, but it
    /// wraps it, so the cached object is equally out of date.
    #[test]
    fn changing_only_the_preparation_also_drops_the_engine() {
        let state = cold();
        state.engine().expect("the stub always loads");

        let mut settings = Settings::default();
        settings.preprocess.upscale = true;
        state.configure(&settings);
        assert!(!state.is_ready());
    }

    /// Saving the settings panel with nothing changed must not throw the model away — the
    /// panel saves on every keystroke it owns, and reloading PaddleOCR-VL costs seconds.
    #[test]
    fn a_save_that_changes_nothing_keeps_the_engine() {
        let state = cold();
        state.engine().expect("the stub always loads");

        state.configure(&Settings::default());
        assert!(state.is_ready());
    }

    /// **The property the whole design exists for.**
    ///
    /// A twenty-second PaddleOCR-VL load holds `loaded` for its whole duration. If `is_ready`
    /// or `chosen_name` ever start taking that mutex, the hint whose entire job is to explain
    /// the wait would itself be waiting for it — and the failure would be a frozen window
    /// rather than a wrong word.
    ///
    /// Answered from another thread against a timeout, because a regression here *hangs*
    /// instead of failing, and a hanging test says nothing until somebody kills it.
    ///
    /// **Detached deliberately — `thread::scope` would reintroduce the hang it is here to
    /// catch.** A scope joins its threads when it ends, so a probe blocked on the very mutex
    /// this is testing would keep the test alive past its own timeout, panic and all. This
    /// version was written with a scope, and the regression it was meant to catch hung it.
    /// An `Arc` and a detached thread let the assertion fail while the probe stays stuck.
    #[test]
    fn readiness_can_be_read_while_the_load_lock_is_held() {
        let state = Arc::new(cold());
        let held = state
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let (tx, rx) = std::sync::mpsc::channel();
        let probe = Arc::clone(&state);
        std::thread::spawn(move || {
            let _ = tx.send((probe.is_ready(), probe.chosen_name(), probe.engine_name()));
        });

        let answered = rx.recv_timeout(std::time::Duration::from_secs(5));
        // Released before asserting, so a failure does not leave the stuck thread holding a
        // poisoned lock for whatever runs next in this process.
        drop(held);

        assert_eq!(
            answered.expect("readiness must not queue behind the load"),
            (false, "stub".to_string(), "stub".to_string())
        );
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;

    /// **The assertion that has caught real bugs twice**: a capture reads the interior and
    /// never the frame's own chrome — its border, or the drag strip along its top.
    #[test]
    fn a_capture_never_contains_the_frames_own_chrome() {
        for scale in [1.0, 1.25, 1.5] {
            let rect = interior_rect((100, 100), (640, 200), scale)
                .unwrap_or_else(|| panic!("{scale} should be readable"));
            let border = (FRAME_BORDER * scale).round() as i32;
            let top = ((FRAME_BORDER + FRAME_GRIP) * scale).round() as i32;

            assert_eq!(rect.x, 100 + border, "at {scale}: past the left border");
            assert_eq!(
                rect.y,
                100 + top,
                "at {scale}: past the grip and the border"
            );
            assert_eq!(rect.width as i32, 640 - border * 2);
            assert_eq!(rect.height as i32, 200 - top - border);
            assert!(
                rect.y - 100 >= (FRAME_GRIP * scale) as i32,
                "at {scale}: anything less puts the grip into the OCR"
            );
        }
    }

    /// 125% is the case a dropped `* scale` once survived, because 100% multiplies by one
    /// and 150% happens to land whole.
    #[test]
    fn the_inset_scales_with_the_display() {
        let rect = interior_rect((0, 0), (800, 250), 1.25).expect("readable");
        assert_eq!(rect.x, 3, "2 x 1.25 is 2.5, rounded to 3");
        assert_eq!(rect.y, 38, "30 x 1.25 is 37.5, rounded to 38");
        assert_eq!(rect.width, 794);
    }

    #[test]
    fn a_requested_size_is_held_to_the_minimum() {
        assert_eq!(
            clamp_frame_size(1.0, 1.0),
            (FRAME_MIN_WIDTH, FRAME_MIN_HEIGHT)
        );
        // Each dimension on its own, so a clamp written for one cannot silently move both.
        assert_eq!(clamp_frame_size(10.0, 400.0), (FRAME_MIN_WIDTH, 400.0));
        assert_eq!(clamp_frame_size(400.0, 10.0), (400.0, FRAME_MIN_HEIGHT));
        assert_eq!(clamp_frame_size(640.0, 200.0), (640.0, 200.0));
    }

    /// The clamp's floor and what the window is told when it is built are one number. They
    /// were once two, in different places, and the window quietly won.
    #[test]
    fn the_clamp_floor_is_what_the_window_is_told() {
        assert_eq!(
            clamp_frame_size(0.0, 0.0),
            (FRAME_MIN_WIDTH, FRAME_MIN_HEIGHT)
        );
        assert!(
            interior_rect(
                (0, 0),
                (FRAME_MIN_WIDTH as u32, FRAME_MIN_HEIGHT as u32),
                1.0
            )
            .is_some(),
            "the smallest allowed frame must still have an interior to read"
        );
    }

    /// The height floor says what it leaves to read, and `interior_rect` agrees.
    ///
    /// The two do this arithmetic in opposite directions — the constant adds the chrome on, the
    /// function takes it off — so a change to the grip, the border or the interior floor that
    /// updates only one of them lands here rather than as a frame that will not shrink to the
    /// line somebody is aiming at.
    #[test]
    fn the_smallest_frame_leaves_exactly_the_interior_it_promises() {
        let rect = interior_rect(
            (0, 0),
            (FRAME_MIN_WIDTH as u32, FRAME_MIN_HEIGHT as u32),
            1.0,
        )
        .expect("the smallest frame is readable");

        assert_eq!(
            rect.height, FRAME_MIN_INTERIOR as u32,
            "the smallest frame should leave {FRAME_MIN_INTERIOR:.0}px to read, not {}",
            rect.height
        );
        assert_eq!(
            rect.width,
            (FRAME_MIN_WIDTH - FRAME_BORDER * 2.0) as u32,
            "width loses only its two borders"
        );
    }

    /// The limits the resize drag holds itself to are the ones `place_frame` enforces.
    ///
    /// They are reported to the UI rather than restated there, and this is what keeps that
    /// true. A drag clamping to a *different* floor than the one the command applies is how
    /// a handle stops moving while the frame keeps sliding — which is precisely the bug the
    /// report exists to fix.
    #[test]
    fn the_reported_limits_are_the_ones_that_are_enforced() {
        let limits = frame_limits();
        assert_eq!(
            clamp_frame_size(0.0, 0.0),
            (limits.min_width, limits.min_height),
            "the drag and the command must agree on the floor"
        );
    }

    #[test]
    fn a_frame_with_no_room_left_is_not_readable() {
        assert!(interior_rect((0, 0), (4, 200), 1.0).is_none());
        // Shorter than its own border and grip: nothing left to read.
        assert!(interior_rect((0, 0), (200, 30), 1.0).is_none());
    }

    /// **A monitor left of or above the primary puts the frame at a negative coordinate.**
    ///
    /// The bug this pins, from a real report: a frame at x = -313 on a portrait monitor whose
    /// origin is x = -1080 came out as a region at x = 0, and the capture read the *primary*
    /// monitor's top-left instead — so the overlay sat over Japanese and the app recognised a
    /// code editor's breadcrumb bar. `interior_rect` returned an `ocr::Rect`, whose `x` is `u32`,
    /// and a `.max(0)` threw the sign away without a word.
    ///
    /// **Every test above this one uses a positive origin**, which is why a whole suite of
    /// assertions about this function stayed green through it. The arrangement that breaks is the
    /// one nobody writes a fixture for: the second monitor is almost always placed to the left.
    #[test]
    fn a_frame_on_a_monitor_left_of_the_primary_keeps_its_negative_coordinate() {
        let rect = interior_rect((-313, 22), (268, 71), 1.0).expect("readable");
        let border = FRAME_BORDER as i32;
        let top = (FRAME_BORDER + FRAME_GRIP) as i32;

        assert_eq!(rect.x, -313 + border, "the sign has to survive");
        assert!(
            rect.x < 0,
            "and it really is negative — not clamped to the primary's edge"
        );
        assert_eq!(rect.y, 22 + top);
    }

    /// The same, vertically, for a monitor stacked above the primary.
    #[test]
    fn a_frame_above_the_primary_keeps_its_negative_y() {
        let rect = interior_rect((40, -400), (400, 300), 1.0).expect("readable");
        assert_eq!(rect.y, -400 + (FRAME_BORDER + FRAME_GRIP) as i32);
        assert!(rect.y < 0);
    }

    /// A frame whose top-left is off the monitor is *cropped*, not slid across it.
    ///
    /// This is the arithmetic in `capture::capture_desktop_rect`, restated here as the property it
    /// has to hold: the columns lost off the left edge come off the width. Clamping the origin and
    /// keeping the size reads pixels the frame was never over, which is the same class of mistake
    /// as the clamp above and was sitting in the same code path.
    #[test]
    fn a_region_off_the_left_edge_loses_width_rather_than_moving() {
        // 40px of a 264px-wide region hang off the left of the monitor holding it.
        let (x, width): (i32, u32) = (-40, 264);
        let monitor_origin: i32 = 0;
        let left_of_monitor = i64::from(x) - i64::from(monitor_origin);
        let cut = (-left_of_monitor).max(0).min(i64::from(width)) as u32;

        assert_eq!(cut, 40);
        assert_eq!(left_of_monitor.max(0) as u32, 0);
        assert_eq!(
            width - cut,
            224,
            "the visible part, not the full width shifted right"
        );
    }
}

/// Where the capture frame stops ignoring the pointer.
#[cfg(test)]
mod chrome_tests {
    use super::*;

    /// A 400x300 frame at 100%, and the same at 150%.
    fn at(x: f64, y: f64, scale: f64) -> bool {
        over_chrome(x, y, 400.0 * scale, 300.0 * scale, scale)
    }

    /// A narrow frame - a column of vertical text, or one subtitle line - is where a flat corner
    /// size stopped matching the drawn one. At 32 px wide the CSS draws a 9.6 px corner, so a
    /// point 16 px in is over the interior; the uncapped constant called it a corner and ate the
    /// click. Both axes are checked, because the cap has to apply per axis to mean anything.
    #[test]
    fn a_narrow_frame_is_only_corner_where_the_corner_is_drawn() {
        let (width, height) = (FRAME_MIN_WIDTH, 300.0);
        assert!(
            !over_chrome(width / 2.0, 283.0, width, height, 1.0),
            "a point past the drawn corner must pass through, not be absorbed by one              the interface never painted"
        );
        assert!(
            over_chrome(2.0, 297.0, width, height, 1.0),
            "and the corner that *is* drawn stays chrome, or the frame loses its resize"
        );

        let (width, height) = (300.0, FRAME_MIN_HEIGHT);
        assert!(
            over_chrome(2.0, height / 2.0, width, height, 1.0),
            "a short frame's left edge is still a handle"
        );
    }

    #[test]
    fn the_middle_is_not_chrome() {
        assert!(!at(200.0, 150.0, 1.0), "the part you read through");
        assert!(!at(300.0, 225.0, 1.5), "the same point at 150%");
    }

    /// The band across the top is the drag strip, and nothing wider.
    ///
    /// It was a toolbar; it is a grip, because the frame must be movable without a keybinding
    /// and the interior cannot be the handle — a drag region there would eat the clicks
    /// click-through exists to pass on. What this guards is that it stays a strip: every
    /// pixel of it is one the OCR does not get.
    #[test]
    fn the_drag_strip_is_chrome_and_stops_where_it_says() {
        assert!(at(200.0, 25.0, 1.0), "inside the 2px border + 28px grip");
        assert!(!at(200.0, 40.0, 1.0), "past it");
        // And it scales, or the strip stops being grabbable on a 150% display —
        // 43.5 is inside 45, and would have been outside an unscaled 30.
        assert!(at(300.0, 43.5, 1.5));
        assert!(!at(300.0, 60.0, 1.5));
    }

    #[test]
    fn every_edge_is_chrome_so_the_frame_still_resizes() {
        for (x, y) in [(2.0, 150.0), (398.0, 150.0), (200.0, 298.0), (200.0, 2.0)] {
            assert!(at(x, y, 1.0), "({x}, {y}) should be a resize handle");
        }
        // Just inside the handles, and no longer chrome.
        assert!(!at(20.0, 150.0, 1.0));
        assert!(!at(200.0, 280.0, 1.0));
    }

    /// The handles scale too, and this is the test that says so.
    ///
    /// Added after planting the bug: dropping `* scale` from the handle broke nothing,
    /// because every edge case above is at 100%.
    ///
    /// Stated against `FRAME_HANDLE` rather than against the number it currently holds. The
    /// first version used literal pixels chosen for an 8px handle, and widening the handle
    /// failed it — which read as a regression when nothing about scaling had changed. A probe
    /// one pixel outside the unscaled handle is the sharpest evidence available either way:
    /// it must miss at 100% and hit at 150%, and only `* scale` can produce both.
    #[test]
    fn the_handles_scale_with_the_display() {
        let just_past = FRAME_HANDLE + 1.0;
        assert!(!at(just_past, 150.0, 1.0), "past the handle at 100%");
        assert!(
            at(just_past, 225.0, 1.5),
            "the same point is inside it at 150%"
        );
        // And still bounded: the handle grew by half, not without limit.
        assert!(
            !at(FRAME_HANDLE * 1.5 + 1.0, 225.0, 1.5),
            "past the scaled handle"
        );
    }

    /// **The corners are drawn bigger than the rim, and this is what says the window agrees.**
    ///
    /// `--corner` is 20px against a 14px `--thickness`, so a press 18px in from the
    /// bottom-left is inside the CSS handle and was outside every term this function had:
    /// the window went transparent to the pointer, the click landed in the application
    /// behind, and the resize never started. The top corners hid it — the drag strip's band
    /// already covers them — so it only ever failed at the bottom, which is also where a
    /// frame over a column of text is most often grabbed.
    #[test]
    fn the_corners_are_chrome_all_the_way_out_to_the_drawn_handle() {
        let inside = FRAME_CORNER - 2.0;
        for (x, y) in [
            (inside, inside),                 // nw
            (400.0 - inside, inside),         // ne
            (inside, 300.0 - inside),         // sw
            (400.0 - inside, 300.0 - inside), // se
        ] {
            assert!(at(x, y, 1.0), "({x}, {y}) is inside a corner handle");
        }
        // And at 150%, where the corner is 30 physical pixels.
        let scaled = (FRAME_CORNER - 2.0) * 1.5;
        assert!(at(scaled, 450.0 - scaled, 1.5), "the sw corner at 150%");
        assert!(
            at(600.0 - scaled, 450.0 - scaled, 1.5),
            "the se corner at 150%"
        );

        // Still a corner and not a band: the same inset along one axis alone is region.
        assert!(
            !at(inside, 150.0, 1.0),
            "mid-edge is the rim's business, and it is past it"
        );
        assert!(
            !at(200.0, 300.0 - inside, 1.0),
            "the bottom edge between the corners"
        );
    }

    #[test]
    fn a_pointer_somewhere_else_entirely_is_not_chrome() {
        assert!(!at(-5.0, 150.0, 1.0));
        assert!(!at(500.0, 150.0, 1.0));
        assert!(!at(200.0, -1.0, 1.0));
    }
}

/// Change what fires a capture, taking effect immediately.
///
/// Applies before saving, and reports failure rather than swallowing it: a chord another
/// application owns cannot be taken, and the caller needs to know so it can keep showing
/// the binding that actually works. On failure the previous binding is put back — ending
/// up with nothing bound would be the worst outcome of trying to rebind.
#[tauri::command(async)]
pub fn set_capture_bind<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    store: State<'_, StoreState>,
    bind: Binding,
) -> Result<(), String> {
    rebind(&app, &store, |settings| {
        settings.capture_bind = bind.clone()
    })
}

/// Change what shows the frame and hides it while held.
#[tauri::command(async)]
pub fn set_overlay_bind<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    store: State<'_, StoreState>,
    bind: Binding,
) -> Result<(), String> {
    rebind(&app, &store, |settings| {
        settings.overlay_bind = bind.clone()
    })
}

/// Set, or clear, the hotkey that switches the overlay off and on.
///
/// `None` clears it. The only binding that may be absent, and absent is its default —
/// there is a button for this, and most people will not want a global chord for it too.
#[tauri::command(async)]
pub fn set_switch_bind<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    store: State<'_, StoreState>,
    bind: Option<Binding>,
) -> Result<(), String> {
    rebind(&app, &store, |settings| settings.switch_bind = bind.clone())
}

/// Apply one binding and save it, putting the old pair back if either step fails.
///
/// The two bindings are registered together — a chord clash is only visible when both are
/// known — so changing one means re-applying both, and rolling both back on failure.
fn apply_all<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    settings: &Settings,
) -> Result<(), String> {
    crate::binding::apply(
        app,
        &settings.capture_bind,
        &settings.overlay_bind,
        settings.switch_bind.as_ref(),
    )
    .map_err(|refusal| refusal.message)
}

fn rebind<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &StoreState,
    set: impl Fn(&mut Settings),
) -> Result<(), String> {
    let _write = crate::lifecycle::gates()
        .enter(crate::lifecycle::Gate::Store)
        .map_err(|e| e.to_string())?;

    let previous = store.settings();
    let mut settings = previous.clone();
    set(&mut settings);

    // **Refused here rather than corrected by `normalise`**, which is the difference between
    // an answer and a silent substitution. `normalise` runs inside `set_settings` and would
    // quietly put the old binding back, so the recorder would report success and then show the
    // previous binding — the shape of a control that does nothing. A message says why.
    for bind in [
        Some(&settings.capture_bind),
        Some(&settings.overlay_bind),
        settings.switch_bind.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Binding::Mouse { button } = bind {
            if !button.bindable() {
                return Err(format!(
                    "{} cannot be a binding: it already selects and marks words in the reader,                      and the hook never swallows a click. Use the middle button or Mouse 4/5.",
                    button.label()
                ));
            }
        }
    }

    if let Err(error) = apply_all(app, &settings) {
        let _ = apply_all(app, &previous);
        return Err(error);
    }

    if let Some(open) = store.store.as_ref() {
        if let Err(error) = open.set_settings(settings) {
            let _ = apply_all(app, &previous);
            return Err(error.to_string());
        }
    }
    Ok(())
}

/// The noun→category table particle senses lean on, loaded once.
///
/// Empty until `scripts/fetch-wordnet.ps1` has run, and that is a supported state rather
/// than a broken one: the structural cues still fire and the app simply abstains more
/// often. See `sakatsu_core::grammar::particles`.
pub struct ParticleState(pub Live<sakatsu_core::grammar::particles::Categories>);

impl ParticleState {
    pub fn load(path: PathBuf) -> ParticleState {
        ParticleState(Live::new(Arc::new(
            sakatsu_core::grammar::particles::Categories::load(&path),
        )))
    }

    /// Read the categories again, after the WordNet table is built.
    pub fn reload(&self) {
        self.0.set(Arc::new(
            sakatsu_core::grammar::particles::Categories::load(
                &crate::wordnet_dir().join("categories.tsv"),
            ),
        ));
    }
}

/// What the particle at `index` can mean here, best-supported reading first.
///
/// The whole token list, because a particle's sense is decided by its neighbours — the
/// noun before it and the verb after — and a single token carries none of that.
///
/// `None` means this is not a particle with senses written for it, and the caller falls
/// back to the grammar pack exactly as before.
#[tauri::command(async)]
pub fn particle_senses(
    state: State<'_, ParticleState>,
    tokens: Vec<Token>,
    index: usize,
    request: Option<RequestIdentity>,
) -> Result<Option<sakatsu_core::grammar::particles::Reading>, String> {
    let ownership = crate::work::scheduler()
        .own(request)
        .map_err(|error| error.to_string())?;
    let found = sakatsu_core::grammar::particles::senses(&tokens, index, &state.0.now());
    ownership.checkpoint().map_err(|error| error.to_string())?;
    Ok(found)
}

/// The dictionary entry for an expression the grammar layer recognised around `index`.
///
/// **Lazy, and that is the point.** Nothing computes this during passage analysis: a page of text
/// would pay a dictionary query per recognised construction for offers nobody opened. It runs when
/// a reader selects a token, which is when the answer is wanted.
///
/// `Ok(None)` is the ordinary answer and means *no offer here* — a different thing from a
/// dictionary that could not be read, which returns `Err`. The interface has to be able to tell a
/// construction that is absent from a lookup that failed, so they never share a value.
///
/// The whole token list and the passage's matches, because an expression is a relationship between
/// tokens and neither a single token nor a bare span carries it.
///
/// Ownership is taken the way [`particle_senses`] takes it, so a slow answer for a selection the
/// reader has already moved on from is discarded instead of replacing a newer one.
#[tauri::command(async)]
pub fn expression_offer(
    state: State<'_, DictionaryState>,
    tokens: Vec<Token>,
    index: usize,
    matches: Vec<sakatsu_core::grammar::Match>,
    request: Option<RequestIdentity>,
) -> Result<Option<sakatsu_core::expression::ExpressionOffer>, String> {
    let ownership = crate::work::scheduler()
        .own(request)
        .map_err(|error| error.to_string())?;
    let open = state.0.now();
    let found = sakatsu_core::expression::offered(&tokens, index, &matches, &open.dictionaries)
        .map_err(|error| error.to_string())?;
    ownership.checkpoint().map_err(|error| error.to_string())?;
    Ok(found)
}

/// Which recognition engine is in use, and whether it is loaded yet.
///
/// **The same payload the [`OCR_ENGINE`] event carries**, so the answer the interface starts
/// with and the answers it is sent later have one shape and one meaning. They used to differ:
/// this returned a bare name, readiness arrived only by event, and the interface therefore
/// began every session believing the engine was cold.
///
/// That was true while nothing loaded before a capture. It stopped being true when choosing
/// PaddleOCR-VL began loading it straight away — a warm that finishes before the webview has
/// attached its listener would leave the hint saying "Loading…" through a read that was in
/// fact instant. Asking for both facts at once cannot miss the event, because it is not
/// waiting for one.
///
/// `(async)` because this locks `loaded`, which a load holds for as long as the model takes.
/// On the IPC thread that would freeze the window; on a worker it merely waits.
#[tauri::command(async)]
pub fn ocr_engine(state: State<'_, OcrState>) -> OcrEngineState {
    OcrEngineState {
        name: state.engine_name(),
        ready: state.is_ready(),
    }
}

/// Recognize an image the user picked, in any format the decoder knows.
///
/// Bytes rather than a path: the file is chosen in the webview, and sending its
/// contents avoids granting the backend filesystem access it otherwise never needs.
#[tauri::command(async)]
pub fn recognize_image(
    state: State<'_, OcrState>,
    bytes: Vec<u8>,
    request: Option<RequestIdentity>,
) -> Result<String, String> {
    let image = capture::decode_image(&bytes)?;
    recognize_scheduled(&state, &image, request)
}

/// What is installed and what is not, for the first-run panel.
///
/// The analyzer check actually runs Ichiran rather than looking for a binary: it needs
/// PostgreSQL behind it, and a sidecar that is present but cannot answer is exactly
/// the failure this screen exists to explain.
#[tauri::command(async)]
pub fn readiness(
    analyzer: State<'_, AnalyzerState>,
    particles: State<'_, ParticleState>,
    kanji: State<'_, KanjiState>,
) -> Readiness {
    let dictionaries_dir = crate::dictionaries_dir();
    let zips = files_named(&dictionaries_dir, |name| name.ends_with(".zip"));
    let indexes = dictionary_subdirectories(&dictionaries_dir);
    let (imported, importable) = setup::dictionary_state(&zips, &indexes);

    let route = analyzer.route();
    let loaded = route.primary.clone();
    let wordnet = particles.0.now();
    let characters = kanji.0.now();
    // Reading setup status must not defeat an explicit on-demand startup preference.
    let deferred = matches!(route.readiness, crate::route::Readiness::Deferred);
    let analyzer_check = (!deferred).then(|| loaded.analyze("水"));
    let analyzer_ok = analyzer_check.as_ref().is_none_or(Result::is_ok);
    let ocr_dir = crate::model_dir();
    let paddle_dir = crate::paddle_model_dir();
    let mt_dir = crate::translation_model_dir();
    let audio_dir = crate::audio_dir();

    // Which analyzer is actually in use, and whether the better one is available.
    let engine = loaded.engine().to_string();
    let on_ichiran = engine == "ichiran";

    /// One frequency list's row. Installed or not, and named either way — the licences
    /// require attribution wherever the data is used, which is why the kanji and WordNet
    /// rows exist too.
    fn frequency_row(
        id: &'static str,
        name: &'static str,
        licence: &str,
        fix: &'static str,
    ) -> Component {
        let ready = destination(id)
            .map(|dir| dir.join("index.json").is_file())
            .unwrap_or(false);
        Component {
            name,
            catalog: Some(id),
            ready,
            detail: if ready {
                format!("{licence} — orders search results by how common a word is")
            } else {
                "not fetched — search results keep the dictionary's own order".into()
            },
            fix: Some(fix),
            required: false,
        }
    }

    let components = vec![
        Component {
            name: "Analyzer",
            catalog: Some("ichiran"),
            // Something always analyses: the built-in one is compiled into the binary.
            ready: analyzer_ok,
            detail: match (&analyzer_check, on_ichiran) {
                (None, _) => {
                    "Ichiran starts on first use; built-in analysis remains available".into()
                }
                (Some(Ok(_)), true) => {
                    "Ichiran — full conjugation chains, compounds and senses".into()
                }
                (Some(Ok(_)), false) => {
                    "built in (Lindera + UniDic) — senses come from the dictionaries, \
                     and grammar is one form per word rather than a derivation"
                        .into()
                }
                (Some(Err(error)), _) => error.to_string(),
            },
            fix: if deferred {
                None
            } else {
                match (analyzer_ok, on_ichiran) {
                    (true, true) => None,
                    (true, false) => {
                        // Optional, so this is an upgrade rather than a repair.
                        Some("Optional: use Set up Ichiran for richer local analysis")
                    }
                    (false, true) => Some(
                        "Choose Automatic or Built-in under Advanced > Analyzer and restart, \
                     or repair Ichiran with Set up Ichiran",
                    ),
                    (false, false) => Some(
                        "Restart the app; if the built-in analyzer still fails, open Diagnostics \
                     and report the error",
                    ),
                }
            },
            // The app analyses either way, so nothing here blocks it.
            required: false,
        },
        Component {
            name: "Dictionaries",
            catalog: Some("dictionaries"),
            ready: !imported.is_empty(),
            detail: if imported.is_empty() {
                format!("none imported in {}", dictionaries_dir.display())
            } else {
                format!("{} imported", imported.join(", "))
            },
            fix: Some("pwsh -File scripts/fetch-dictionaries.ps1"),
            // The analyzer's own senses still show without them.
            required: false,
        },
        // The conventional engines are complementary rather than ranked: manga-ocr for
        // manga, PP-OCRv6 for typeset text and the layout evidence Auto routes on. A
        // supported installation has both. Auto still degrades to the surviving engine if
        // an installed model is later damaged or removed; setup remains insistent until
        // that incomplete installation is repaired.
        Component {
            name: "Recognition — manga-ocr",
            catalog: Some("manga-ocr"),
            ready: MangaOcr::is_present(&ocr_dir),
            detail: if MangaOcr::is_present(&ocr_dir) {
                "ready — best for manga, vertical text and hand lettering".into()
            } else {
                format!("no model in {}", ocr_dir.display())
            },
            fix: Some("pwsh -File scripts/fetch-ocr-model.ps1"),
            required: true,
        },
        Component {
            name: "Recognition — PP-OCRv6",
            catalog: Some("paddle-ocr"),
            ready: PaddleOcr::is_present(&paddle_dir),
            detail: if PaddleOcr::is_present(&paddle_dir) {
                "ready — best for typeset text in games and menus".into()
            } else {
                format!("no models in {}", paddle_dir.display())
            },
            fix: Some("pwsh -File scripts/fetch-paddle-ocr.ps1"),
            required: true,
        },
        // **The action is back, and the three things it depended on are all un-paused.** The
        // comment here used to explain why this row pointed nowhere: `fetching.svelte.ts` hid
        // `paddle-vl`, `VlmSetup.svelte` was imported by nothing, and `Settings::normalise`
        // rewrote `OcrChoice::Vlm` back to `Auto`. Naming an action while any of those held
        // would have sent a reader to a control that did not exist.
        //
        // All three were reversed together when the corpus scored this reader best of anything
        // measured here — 0.0297 mean character error against 0.0975 for Auto. `Auto` still
        // never enters it: it is a choice, not a default.
        Component {
            name: "Recognition — PaddleOCR-VL",
            catalog: Some("paddle-vl"),
            ready: crate::vlm_setup::installed(),
            detail: if crate::vlm_setup::installed() {
                "installed — choose PaddleOCR-VL as the reader to use it".into()
            } else {
                "optional — the most accurate reader measured here, and it wants a GPU".into()
            },
            fix: if crate::vlm_setup::installed() {
                None
            } else {
                Some("Settings › Recognition › set up PaddleOCR-VL")
            },
            required: false,
        },
        // Listed for two reasons. It is genuinely optional — particle senses fall back to
        // structural cues and abstain more often without it — and its licence requires
        // attribution wherever it is used, which a comment in a fetch script does not
        // satisfy. The line names both sources whether or not the table is installed.
        Component {
            name: "Particle senses — WordNet",
            catalog: Some("wordnet"),
            ready: !wordnet.is_empty(),
            detail: if wordnet.is_empty() {
                "not fetched — senses are ranked on sentence structure alone".into()
            } else {
                "Japanese WordNet (NICT, Francis Bond — bond-lab.github.io/wnja) \
                 with Princeton WordNet 3.0"
                    .into()
            },
            fix: Some("Optional: pwsh -File scripts/fetch-wordnet.ps1"),
            required: false,
        },
        // Optional at runtime, but part of the informed standard setup. The separately linked
        // source credits live in the catalog/About surface rather than being flattened here.
        Component {
            name: "Kanji details",
            catalog: Some("kanji"),
            ready: !characters.is_empty(),
            detail: if characters.is_empty() {
                "not fetched — the reference page cannot show character readings or details".into()
            } else if update_available("kanji") {
                format!(
                    "{} KANJIDIC2 characters installed; stroke diagrams and visual components are available as an update",
                    characters.len()
                )
            } else {
                format!(
                    "{} characters with KANJIDIC2 readings, KanjiVG diagrams and KRADFILE components",
                    characters.len()
                )
            },
            fix: Some("Optional: pwsh -File scripts/fetch-kanjidic.ps1"),
            required: false,
        },
        Component {
            name: "Translation",
            catalog: Some("translation"),
            ready: FuguMt::is_present(&mt_dir),
            detail: if FuguMt::is_present(&mt_dir) {
                "FuguMT ready".into()
            } else {
                format!("no model in {}", mt_dir.display())
            },
            fix: Some("pwsh -File scripts/fetch-translation-model.ps1"),
            required: false,
        },
        Component {
            name: "Offline pronunciation",
            catalog: Some("pronunciation"),
            ready: audio_dir.join("index.tsv").is_file(),
            detail: if audio_dir.join("index.tsv").is_file() {
                "Tofugu and WaniKani recordings ready for exact local matches".into()
            } else {
                "not fetched — pronunciation falls back to the current online source".into()
            },
            fix: Some("Optional: pwsh -File scripts/fetch-pronunciation.ps1"),
            required: false,
        },
        // Both frequency lists, because `every_readiness_row_points_at_a_catalog_entry`
        // insists on it in both directions: anything the app offers to fetch must have a
        // row saying whether it is installed, or downloading it is invisible afterwards.
        // These two were added to the catalog and never given rows, and the test has been
        // failing since.
        //
        // `index.json` rather than the directory, because `unpack` writes that alongside
        // the banks and `Frequencies::load` needs it — a half-unpacked directory exists
        // without being usable.
        frequency_row(
            "frequency-jpdb",
            "Frequency — JPDB",
            "JPDB v2.2 (compiled from jpdb.io)",
            "Optional: pwsh -File scripts/fetch-frequency-jpdb.ps1",
        ),
        frequency_row(
            "frequency-bccwj",
            "Frequency — BCCWJ",
            "BCCWJ (NINJAL's Balanced Corpus of Contemporary Written Japanese)",
            "Optional: pwsh -File scripts/fetch-frequency-bccwj.ps1",
        ),
    ];

    Readiness {
        components,
        importable,
    }
}

/// Import a dictionary zip that is sitting in the dictionaries directory.
///
/// Returns immediately and works on a background thread: importing Jitendex takes over
/// a minute, and doing it on the command thread would freeze the window into something
/// indistinguishable from a crash. The result arrives as a `dictionary-imported` event.
///
/// The new dictionary is open the moment the import finishes — the same reload
/// `fetch_component` does, because a zip dropped in the folder and a zip the app downloaded
/// are the same event from the dictionaries' side. This used to say a restart was needed.
#[tauri::command]
pub fn import_dictionary<R: tauri::Runtime>(app: tauri::AppHandle<R>, file: String) {
    use tauri::Emitter;

    std::thread::spawn(move || {
        // A zip dropped in the folder writes the same containers an install does, and this took no
        // claim at all. Owned before the directory is resolved and kept across adoption, for the
        // reasons `crate::owning` sets out.
        let payload = crate::owning(
            [sakatsu_core::setup::claim::Claim::Dictionaries],
            |_owned| {
                let dir = crate::dictionaries_dir();
                // The index directory takes the zip's first word: jitendex-yomitan.zip becomes
                // jitendex, which is what `readiness` matches on.
                let name = file
                    .trim_end_matches(".zip")
                    .split(['-', '_', '.'])
                    .next()
                    .unwrap_or("dictionary")
                    .to_lowercase();

                let outcome = dictionary_archive_in(&dir, &file).and_then(|archive| {
                    Dictionary::import(&archive, &dir.join(&name))
                        .map_err(|error| error.to_string())
                });
                let payload = match outcome {
                    Ok(dictionary) => serde_json::json!({
                        "file": file,
                        "ok": true,
                        "detail": format!("{} entries", dictionary.meta().entry_count),
                    }),
                    Err(error) => serde_json::json!({
                        "file": file,
                        "ok": false,
                        "detail": error.to_string(),
                    }),
                };
                // Before the event, for the same reason as in `fetch_component`: this is what the
                // interface waits on, and reloading after it would leave a moment where the screen
                // says the dictionary is in and a lookup in it still finds nothing.
                if payload["ok"] == serde_json::json!(true) {
                    if let Some(component) = sakatsu_core::setup::catalog::find("dictionaries") {
                        adopt(&app, component);
                    }
                }
                payload
            },
        )
        .unwrap_or_else(|busy| serde_json::json!({ "file": file, "ok": false, "detail": busy }));
        let _ = app.emit("dictionary-imported", payload);
    });
}

/// Resolve only one zip that readiness could have enumerated. Canonicalising both sides
/// rejects `..`, absolute paths, alternate path prefixes, and a symlink parked in the
/// dictionaries directory that actually points elsewhere.
pub fn dictionary_archive_in(dir: &std::path::Path, file: &str) -> Result<PathBuf, String> {
    let path = std::path::Path::new(file);
    let mut components = path.components();
    let one_file = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if !one_file
        || !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Err("choose a dictionary zip listed by the setup screen".into());
    }

    let root = std::fs::canonicalize(dir)
        .map_err(|error| format!("dictionary directory could not be opened: {error}"))?;
    let archive = std::fs::canonicalize(dir.join(path))
        .map_err(|error| format!("dictionary archive could not be opened: {error}"))?;
    if archive.parent() != Some(root.as_path()) || !archive.is_file() {
        return Err("dictionary archive is outside the dictionaries directory".into());
    }
    Ok(archive)
}

fn files_named(dir: &std::path::Path, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| keep(name))
        .collect();
    found.sort();
    found
}

/// Subdirectories that hold a dictionary, in either layout.
///
/// **Not a `meta.json` marker test.** That asks where the files used to be. A
/// published dictionary keeps them inside the generation `current` names, so the marker test
/// returned nothing for exactly the installs it was meant to find — the setup panel called the
/// component missing and the backfill enumerated an empty list.
fn dictionary_subdirectories(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter(|entry| Dictionary::exists_in(&entry.path()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

/// Word recordings, with their in-memory cache.
pub struct AudioState(pub WordAudio);

impl Default for AudioState {
    fn default() -> Self {
        AudioState(WordAudio::with_pack(crate::audio_dir()))
    }
}

/// The recording for a word, as raw MP3 bytes, or `null` when there is none.
///
/// Bytes rather than a URL: the endpoint answers 200 with a placeholder clip for words
/// it has no recording of, so the *content* has to be inspected before anything is
/// played. Handing the UI a URL would make it play that placeholder for every word
/// without audio.
#[tauri::command(async)]
pub fn word_audio(
    state: State<'_, AudioState>,
    store: State<'_, StoreState>,
    expression: String,
    reading: String,
) -> Result<Option<Vec<u8>>, String> {
    let found = state
        .0
        .fetch(&expression, &reading)
        .map_err(|error| error.to_string())?;

    // **Recorded here rather than in the interface**, because this is the only place that knows
    // the answer: the endpoint replies to a word it has never recorded with a placeholder clip
    // and a 200, and `WordAudio::fetch` is what recognises it. Written down so the button stops
    // being offered for this word after a restart as well as during this session.
    //
    // A failed write is not worth failing playback over — the miss is still true, the interface
    // still hides the button, and the only cost is asking again next time.
    if found.is_none() {
        if let Some(store) = store.store.as_ref() {
            let _ = crate::lifecycle::gates()
                .enter(crate::lifecycle::Gate::Store)
                .map(|_write| store.remember_audio(WordKey::new(expression, reading), false));
        }
    }

    Ok(found)
}

/// Everything already known about which words have a recording.
///
/// Read once at startup, so a word checked in any earlier session is never asked about again.
#[tauri::command(async)]
pub fn audio_known(audio: State<'_, AudioState>, state: State<'_, StoreState>) -> Vec<AudioRecord> {
    let mut records = state
        .store
        .as_ref()
        .map(Store::audio_known)
        .unwrap_or_default();
    // A newly installed local pack can cover a word the remote source previously marked silent.
    // Upgrade that durable answer during hot adoption; otherwise the UI would trust its old `no`
    // forever and never ask the newly available exact local index.
    for record in &mut records {
        if !record.available
            && audio
                .0
                .available_locally(&record.key.term, &record.key.reading)
        {
            record.available = true;
            if let Some(store) = state.store.as_ref() {
                let _ = crate::lifecycle::gates()
                    .enter(crate::lifecycle::Gate::Store)
                    .map(|_write| store.remember_audio(record.key.clone(), true));
            }
        }
    }
    records
}

/// How many words are probed at once.
///
/// Small on purpose. This is somebody else's server and the work is only deciding whether to
/// draw a button, so it takes a thin slice of their capacity — a search's worth of words resolves
/// in a few rounds either way, because a `HEAD` costs headers and no body.
const AUDIO_PROBE_WORKERS: usize = 4;

/// Which of these words have a recording, asking only about the ones nobody has asked about yet.
///
/// **Cheap by design, and it has to be.** The interface calls this for every word it displays, so
/// the request has to be worth making: [`WordAudio::available`] answers from a `HEAD` — headers,
/// no body — and only downloads a clip when the length is ambiguous. Everything learned is written
/// to the store, so each word costs one request across the whole life of the installation rather
/// than one per run.
///
/// A word the endpoint could not be reached for is simply left out of the reply rather than
/// recorded as silent. Being offline is not evidence that a word has no recording, and writing it
/// down as such would make a network blip permanent.
#[tauri::command(async)]
pub fn audio_availability(
    state: State<'_, AudioState>,
    store: State<'_, StoreState>,
    words: Vec<WordKey>,
) -> Vec<AudioRecord> {
    let mut known: HashMap<WordKey, bool> = store
        .store
        .as_ref()
        .map(Store::audio_known)
        .unwrap_or_default()
        .into_iter()
        .map(|record| (record.key, record.available))
        .collect();

    for key in &words {
        if known.get(key) == Some(&false) && state.0.available_locally(&key.term, &key.reading) {
            known.insert(key.clone(), true);
            if let Some(store) = store.store.as_ref() {
                let _ = crate::lifecycle::gates()
                    .enter(crate::lifecycle::Gate::Store)
                    .map(|_write| store.remember_audio(key.clone(), true));
            }
        }
    }

    let unknown: Vec<WordKey> = words
        .iter()
        .filter(|key| !known.contains_key(key))
        .cloned()
        .collect();

    // Answer what is already known without asking anybody anything.
    let mut answers: Vec<AudioRecord> = words
        .iter()
        .filter_map(|key| {
            known.get(key).map(|available| AudioRecord {
                key: key.clone(),
                available: *available,
            })
        })
        .collect();

    if unknown.is_empty() {
        return answers;
    }

    // A few threads over a shared queue. `WordAudio` is behind `&self` and its own lock, so the
    // in-memory cache is shared across them and two workers asking about the same word cannot
    // both go to the network.
    let queue = Mutex::new(unknown.into_iter());
    let found = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..AUDIO_PROBE_WORKERS {
            scope.spawn(|| loop {
                let Some(key) = queue.lock().ok().and_then(|mut queue| queue.next()) else {
                    return;
                };
                if let Ok(available) = state.0.available(&key.term, &key.reading) {
                    if let Ok(mut found) = found.lock() {
                        found.push(AudioRecord { key, available });
                    }
                }
            });
        }
    });

    for record in found.into_inner().unwrap_or_default() {
        if let Some(store) = store.store.as_ref() {
            let _ = crate::lifecycle::gates()
                .enter(crate::lifecycle::Gate::Store)
                .map(|_write| store.remember_audio(record.key.clone(), record.available));
        }
        answers.push(record);
    }
    answers
}

/// Which decks Anki has, so the UI can offer them rather than guess.
///
/// Also the liveness probe: if this fails, Anki is not running.
#[tauri::command(async)]
pub fn anki_decks() -> Result<Vec<String>, String> {
    Anki::default()
        .deck_names()
        .map_err(|error| error.to_string())
}

/// The decks, with what is in each one.
///
/// Separate from [`anki_decks`] rather than replacing it, because this costs two searches a
/// deck and the liveness probe runs on every settings open. The counts are only wanted by the
/// panel that manages decks.
#[tauri::command(async)]
pub fn anki_deck_info() -> Result<Vec<DeckInfo>, String> {
    Anki::default().decks().map_err(|error| error.to_string())
}

/// Make a new, empty deck.
#[tauri::command(async)]
pub fn anki_create_deck(name: String) -> Result<(), String> {
    Anki::default()
        .create_deck(&name)
        .map_err(|error| error.to_string())
}

/// Copy a deck's structure and options under a new name. The cards do not come — see
/// [`sakatsu_core::anki::Anki::duplicate_deck`].
#[tauri::command(async)]
pub fn anki_duplicate_deck(source: String, target: String) -> Result<(), String> {
    Anki::default()
        .duplicate_deck(&source, &target)
        .map_err(|error| error.to_string())
}

/// Rename a deck and everything under it.
///
/// Refused unless the deck holds only cards this app made. The check lives in the core client
/// rather than here, so it cannot be forgotten by a second caller.
#[tauri::command(async)]
pub fn anki_rename_deck(from: String, to: String) -> Result<(), String> {
    Anki::default()
        .rename_deck(&from, &to)
        .map_err(|error| error.to_string())
}

/// Delete a deck, its subdecks, and the cards in them. Gated the same way as renaming.
#[tauri::command(async)]
pub fn anki_delete_deck(name: String) -> Result<(), String> {
    Anki::default()
        .delete_deck(&name)
        .map_err(|error| error.to_string())
}

/// Add the looked-up word to Anki, with the sentence it was met in.
///
/// The sentence travels with the word deliberately: a word without the sentence it
/// came from is the flashcard that teaches a definition and not a language.
///
/// **The tokens and an index, rather than a string.** This used to take the sentence as text and
/// the interface passed `analysis.text` — the *whole* passage — so a card made from a paragraph
/// carried the paragraph, on every card made from it. Only the tokens can say where one sentence
/// ends, and only they carry the readings; both are what [`sentence::markup`] needs to write the
/// furigana the card shows. Doing it here rather than in the interface is what makes it testable.
#[tauri::command(async)]
pub fn add_to_anki(
    deck: String,
    word: WordView,
    tokens: Vec<sakatsu_core::model::Token>,
    index: usize,
    dictionaries: State<'_, DictionaryState>,
    kanji: State<'_, KanjiState>,
) -> Result<i64, String> {
    let sentence = sentence::markup(&tokens, index, &kanji.0.now());

    // Which dictionary answered. `inline_source` is set only when the definition was promoted
    // from a secondary source, so the primary has to be named from the open list — it is the
    // first, by the same merge order the reference page shows.
    let source = word.inline_source.clone().unwrap_or_else(|| {
        dictionaries
            .0
            .now()
            .dictionaries
            .first()
            .map(|dictionary| dictionary.meta().title.clone())
            .unwrap_or_default()
    });

    let note = Note::from_word(&word, &sentence, &source);
    Anki::default()
        .add(&deck, &note)
        .map_err(|error| error.to_string())
}

/// The translation model, loaded on first use like the recognition one.
pub struct TranslatorState {
    dir: PathBuf,
    loaded: Mutex<Option<Arc<dyn Translator>>>,
    /// Translations already produced, keyed by the exact source string.
    ///
    /// The comparison strip asks the same questions over and over: ticking a word off and back on
    /// re-asks about a string that was answered a moment ago, and the sentence itself is re-asked
    /// on every single change. Decoding is around 8ms per word produced — cheap, but not cheap
    /// enough to spend on an answer already held.
    ///
    /// Cleared whenever it grows past a passage's worth, because it is a convenience across one
    /// reading session and not a store: nothing here is worth surviving a restart, and an
    /// unbounded map behind a feature driven by clicking is a leak with a slow fuse.
    memo: Mutex<HashMap<String, String>>,
    /// Admit one cache miss *per sentence* at a time, then check the cache again before decoding.
    ///
    /// Opening aligned translation and Ctrl+clicking a word can ask for the same sentence from
    /// two worker threads. A mutex around the map alone cannot prevent both from observing a miss
    /// and running the model. Keeping the slow work behind a separate gate makes identical
    /// requests coalesce without holding the memo lock during inference.
    ///
    /// **Sharded, not one gate.** A single mutex was held across decoding, so it coalesced
    /// identical requests and serialised every *different* one too: a thirty-sentence passage
    /// decoded one sentence at a time, and a Ctrl+click arriving during it queued behind all
    /// thirty before it could so much as look in the memo. Sixteen shards keyed by the sentence
    /// give the coalescing without the queue. Same shape as
    /// [`sakatsu_core::audio::WordAudio`]'s gates, and for the same reason: a fixed set is
    /// bounded and needs no lock-map cleanup, while identical keys still share a gate.
    miss_gates: [Mutex<()>; TranslatorState::MISS_GATES],
    /// The last translation this state answered, for the status display.
    last: crate::runtime_status::LastResult,
}

impl TranslatorState {
    pub fn new(dir: PathBuf) -> TranslatorState {
        TranslatorState {
            dir,
            loaded: Mutex::new(None),
            memo: Mutex::new(HashMap::new()),
            miss_gates: std::array::from_fn(|_| Mutex::new(())),
            last: Default::default(),
        }
    }

    pub fn with_engine(engine: Arc<dyn Translator>) -> TranslatorState {
        TranslatorState {
            dir: PathBuf::new(),
            loaded: Mutex::new(Some(engine)),
            memo: Mutex::new(HashMap::new()),
            miss_gates: std::array::from_fn(|_| Mutex::new(())),
            last: Default::default(),
        }
    }

    /// The last translation this state answered.
    pub fn last(&self) -> &crate::runtime_status::LastResult {
        &self.last
    }

    /// Release the loaded model from memory, keeping its files and the remembered translations.
    /// Call only with the translation gate held. The next translation loads it again.
    pub fn unload(&self) -> bool {
        self.loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .is_some()
    }

    /// The translator as the status display describes it, without waiting on a load.
    pub fn status(&self) -> sakatsu_core::runtime_status::EngineStatus {
        use sakatsu_core::runtime_status::{
            Acceleration, EngineRole, EngineState, EngineStatus, Hosting,
        };
        let installed = FuguMt::is_present(&self.dir);
        let describe = |engine: Option<&Arc<dyn Translator>>| match engine {
            Some(engine) => (EngineState::Ready, Some(engine.engine().to_string())),
            None if installed => (EngineState::NotLoaded, None),
            None => (EngineState::NotInstalled, None),
        };
        let (state, loaded) = match self.loaded.try_lock() {
            Ok(guard) => describe(guard.as_ref()),
            Err(std::sync::TryLockError::WouldBlock) => (EngineState::Loading, None),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                describe(poisoned.into_inner().as_ref())
            }
        };
        // The stub answers with an explanation, not a translation: nothing is computing.
        let computing = state == EngineState::Ready && loaded.as_deref() != Some("stub");
        EngineStatus {
            role: EngineRole::Translation,
            configured: if installed { "fugumt" } else { "none" }.to_string(),
            loaded,
            hosting: Hosting::InProcess,
            state,
            acceleration: if computing {
                sakatsu_core::onnx::acceleration()
            } else {
                Acceleration::NotRunning
            },
        }
    }

    fn engine(&self) -> Arc<dyn Translator> {
        let mut loaded = self
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(engine) = loaded.as_ref() {
            return Arc::clone(engine);
        }

        let engine: Arc<dyn Translator> = match FuguMt::is_present(&self.dir) {
            true => match FuguMt::load(&self.dir) {
                Ok(engine) => Arc::new(engine),
                // A model that will not load must not take translation down with a
                // panic; the stub reports the reason instead.
                Err(error) => {
                    eprintln!("translation model failed to load: {error}");
                    sakatsu_core::log_error!("translate", "model failed to load: {error}");
                    Arc::new(StubTranslator)
                }
            },
            false => Arc::new(StubTranslator),
        };
        *loaded = Some(Arc::clone(&engine));
        engine
    }
}

/// Translate a passage into English.
///
/// Called only when the learner opens the translation disclosure. Nothing translates
/// on its own: the point of the app is to read the Japanese, and a translation sitting
/// on screen does the reading for you (`project notes` §7).
///
/// **This is the one caller that is ever handed more than one sentence**, which is why the
/// boundary marking lives here rather than inside the translator. `translate_sentences` and
/// `translate_variants` build their input from a token range, and a range cannot span a newline
/// when a newline ends a sentence — so a line break only ever reaches the model through this
/// command. Left alone, `FuguMt::for_the_model` welds the lines and the passage comes back with
/// its sentences run together. See `translate::passage` for what was measured.
#[tauri::command(async)]
pub fn translate(
    state: State<'_, TranslatorState>,
    text: String,
    request: Option<RequestIdentity>,
) -> Result<String, String> {
    let identity = request.clone();
    let permit = crate::work::scheduler()
        .enter(crate::work::Lane::Translation, request)
        .map_err(|error| error.to_string())?;
    let marked = sakatsu_core::translate::mark_boundaries(&text);
    permit.checkpoint().map_err(|error| error.to_string())?;
    let traced = state.remembering_traced(&marked);
    permit.checkpoint().map_err(|error| error.to_string())?;
    record_translation(
        &state,
        identity.as_ref(),
        match &traced {
            Ok((_, cached)) => Ok(sakatsu_core::runtime_status::CacheUse::combine([*cached])),
            Err(error) => Err(error.into()),
        },
    );
    traced
        .map(|(english, _)| english)
        .map_err(|error| error.to_string())
}

/// One canonical sentence translation, grounded in the analysis token stream.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SentenceTranslation {
    /// The sentence's half-open token range. These names deliberately mirror [`SentenceRange`]
    /// at the UI boundary so callers cannot mistake the end for an inclusive index.
    pub first_token: usize,
    pub last_token_end: usize,
    /// The exact string sent to the translator, rebuilt without normalisation from token surfaces.
    pub japanese: String,
    pub english: String,
}

/// Translate every sentence through the same exact-input cache used by focused comparisons.
///
/// The vector is returned only after the complete batch succeeds. Answers completed before a
/// later failure remain memoized, making Retry cheap without exposing a partially translated
/// passage as though it were complete.
#[tauri::command(async)]
pub fn translate_sentences(
    state: State<'_, TranslatorState>,
    tokens: Vec<Token>,
    request: Option<RequestIdentity>,
) -> Result<Vec<SentenceTranslation>, String> {
    let identity = request.clone();
    let permit = crate::work::scheduler()
        .enter(crate::work::Lane::Translation, request)
        .map_err(|error| error.to_string())?;
    let traced = state.sentence_translations_traced(&tokens, || permit.is_current());
    record_translation(
        &state,
        identity.as_ref(),
        match &traced {
            Ok((_, cache)) => Ok(*cache),
            Err(failure) => Err(failure.code),
        },
    );
    traced
        .map(|(sentences, _)| sentences)
        .map_err(|failure| failure.message)
}

/// How many translations to keep before starting over.
///
/// A page of Japanese is a few dozen sentences and each selection asks two or three questions of
/// it, so this holds a long reading session comfortably. Dropping the lot rather than evicting one
/// at a time is deliberate: there is no useful notion of "least recently wanted" here, and the
/// cost of a miss is milliseconds.
const MEMO_LIMIT: usize = 512;

impl TranslatorState {
    /// How many independent cache-miss gates. See [`TranslatorState::miss_gates`].
    const MISS_GATES: usize = 16;

    /// Translate, remembering the answer.
    fn remembering(&self, text: &str) -> Result<String, String> {
        self.remembering_traced(text)
            .map(|(english, _)| english)
            .map_err(|error| error.to_string())
    }

    /// Translate, remembering the answer, and say whether it was already remembered.
    ///
    /// The flag is what lets the status say "from cache" truthfully: a remembered answer ran no
    /// model, and a status that called it a fresh translation would be describing work that did
    /// not happen.
    fn remembering_traced(
        &self,
        text: &str,
    ) -> Result<(String, bool), sakatsu_core::translate::TranslateError> {
        if text.trim().is_empty() {
            return Ok((String::new(), true));
        }
        if let Some(known) = self.memoized(text) {
            return Ok((known, true));
        }

        let _miss = self.miss_gates[self.gate_for(text)]
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Another worker may have filled this exact entry while this one waited at the gate.
        if let Some(known) = self.memoized(text) {
            return Ok((known, true));
        }

        let english = self.engine().translate(text)?;
        let mut memo = self.memo.lock().unwrap_or_else(|p| p.into_inner());
        if memo.len() >= MEMO_LIMIT {
            memo.clear();
        }
        memo.insert(text.to_string(), english.clone());
        Ok((english, false))
    }

    /// Which gate a sentence waits at. Identical strings must always pick the same one, or two
    /// workers asking the same question would both decode it.
    fn gate_for(&self, text: &str) -> usize {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish() as usize % Self::MISS_GATES
    }

    fn memoized(&self, text: &str) -> Option<String> {
        self.memo
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(text)
            .cloned()
    }

    #[cfg(test)]
    fn sentence_translations(&self, tokens: &[Token]) -> Result<Vec<SentenceTranslation>, String> {
        self.sentence_translations_traced(tokens, || true)
            .map(|(sentences, _)| sentences)
            .map_err(|failure| failure.message)
    }

    /// The same, saying how much came from the cache, and why it failed as a code too.
    fn sentence_translations_traced(
        &self,
        tokens: &[Token],
        current: impl Fn() -> bool,
    ) -> Result<
        (
            Vec<SentenceTranslation>,
            sakatsu_core::runtime_status::CacheUse,
        ),
        SentenceFailure,
    > {
        let mut cached = Vec::new();
        let sentences = sakatsu_core::model::sentence_ranges(tokens)
            .into_iter()
            .map(|range| {
                if !current() {
                    return Err(SentenceFailure::cancelled());
                }
                let japanese: String = tokens[range.first_token..range.last_token_end]
                    .iter()
                    .map(|token| token.surface.as_str())
                    .collect();
                let (english, was_cached) =
                    self.remembering_traced(&japanese)
                        .map_err(|error| SentenceFailure {
                            code: (&error).into(),
                            message: error.to_string(),
                        })?;
                cached.push(was_cached);
                if !current() {
                    return Err(SentenceFailure::cancelled());
                }
                Ok(SentenceTranslation {
                    first_token: range.first_token,
                    last_token_end: range.last_token_end,
                    japanese,
                    english,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((
            sentences,
            sakatsu_core::runtime_status::CacheUse::combine(cached),
        ))
    }
}

/// Why a batch of sentence translations failed: the code for the status, the message for the
/// reader. The message is exactly what the batch reported before the code existed.
struct SentenceFailure {
    code: sakatsu_core::runtime_status::FailureCode,
    message: String,
}

impl SentenceFailure {
    fn cancelled() -> Self {
        SentenceFailure {
            code: sakatsu_core::runtime_status::FailureCode::Cancelled,
            message: "request cancelled".into(),
        }
    }
}

/// Record a translation request's result, unless it was cancelled: a cancelled request answered
/// nobody, and there is nothing to say about it.
fn record_translation(
    state: &TranslatorState,
    request: Option<&RequestIdentity>,
    produced: Result<
        sakatsu_core::runtime_status::CacheUse,
        sakatsu_core::runtime_status::FailureCode,
    >,
) {
    use sakatsu_core::runtime_status::{
        CacheUse, EngineRole, FailureCode, OperationResult, Outcome,
    };
    if produced == Err(FailureCode::Cancelled) {
        return;
    }
    let status = state.status();
    state.last.record(OperationResult {
        role: EngineRole::Translation,
        request: request.cloned(),
        engine: status.loaded.clone().unwrap_or(status.configured),
        fallback: None,
        cache: produced.unwrap_or(CacheUse::Fresh),
        outcome: match produced {
            Ok(_) => Outcome::Succeeded,
            Err(code) => Outcome::Failed { code },
        },
        acceleration: status.acceleration,
    });
}

impl TranslatorState {}

/// What the sentence loses without the struck words.
///
/// **The selection is deliberately not translated on its own any more.** That reading was measured
/// by `core/tests/lens_agreement.rs` over 880 comparisons: it matched what the sentence lost 34.6%
/// of the time for ordinary words and **1.4%** for particles the app has a written sense for — the
/// exact words a learner most wants to click. `Variants::runs` still builds those strings because
/// the measurement needs them; nothing translates them.
///
/// **This is not a grammar reference and must never be presented as one.** `project notes` §7 calls
/// this model a rough gist that gets plenty wrong; a diff merely makes being wrong look like a
/// finding. The cited particle senses and the grammar packs are the app's authorities.
#[tauri::command(async)]
pub fn translate_variants(
    state: State<'_, TranslatorState>,
    tokens: Vec<Token>,
    selected: Vec<usize>,
    request: Option<RequestIdentity>,
) -> Result<Option<VariantTranslations>, String> {
    let permit = crate::work::scheduler()
        .enter(crate::work::Lane::Translation, request)
        .map_err(|error| error.to_string())?;
    let picked: std::collections::BTreeSet<usize> = selected.into_iter().collect();
    let Some(variants) =
        sakatsu_core::translate::variants(&tokens, &picked).map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    let full = state.remembering(&variants.full)?;
    permit.checkpoint().map_err(|error| error.to_string())?;
    let without = state.remembering(&variants.without)?;
    permit.checkpoint().map_err(|error| error.to_string())?;

    // Computed here rather than in the component, so the panel and `lens_agreement.rs` cannot
    // disagree about what a removal took.
    let pieces = sakatsu_core::translate::word_diff(&full, &without);
    // Same reasoning, one line later: whether the shortened reading is worth a line of its own is
    // a question about these two strings, and both of them are here.
    let result_says_more = sakatsu_core::translate::result_says_more(&pieces, &without);

    Ok(Some(VariantTranslations {
        japanese: variants.clone(),
        full,
        without,
        pieces,
        result_says_more,
    }))
}

/// The three readings, as Japanese and as English.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VariantTranslations {
    /// The strings that were asked about, so the interface can show what it compared.
    pub japanese: sakatsu_core::translate::Variants,
    pub full: String,
    /// Empty when the selection was the whole sentence.
    pub without: String,
    /// `full`, in order, with the runs that the removal took marked.
    pub pieces: Vec<sakatsu_core::translate::Piece>,
    /// Whether [`Self::without`] is worth drawing under the marked-up line, or merely repeats the
    /// words that survived it. See `translate::result_says_more`.
    pub result_says_more: bool,
}

#[cfg(test)]
mod translation_tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    use sakatsu_core::translate::TranslateError;

    use super::*;

    struct CountingTranslator {
        calls: AtomicUsize,
        fail: bool,
    }

    impl CountingTranslator {
        fn echo() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail: true,
            }
        }
    }

    impl Translator for CountingTranslator {
        fn translate(&self, text: &str) -> Result<String, TranslateError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            // Give concurrent callers time to observe the same initial miss. The assertion below
            // would reliably see several calls without the miss gate and its second cache check.
            std::thread::sleep(Duration::from_millis(10));
            if self.fail {
                Err(TranslateError::Failed("deliberate test failure".into()))
            } else {
                Ok(format!("English<{text}>"))
            }
        }

        fn engine(&self) -> &str {
            "counting"
        }
    }

    #[derive(Default)]
    struct FlakyBatchTranslator {
        calls: Mutex<HashMap<String, usize>>,
    }

    impl FlakyBatchTranslator {
        fn calls_for(&self, text: &str) -> usize {
            *self
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(text)
                .unwrap_or(&0)
        }
    }

    impl Translator for FlakyBatchTranslator {
        fn translate(&self, text: &str) -> Result<String, TranslateError> {
            let mut calls = self
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let count = calls.entry(text.into()).or_default();
            *count += 1;
            if text == "犬。" && *count == 1 {
                Err(TranslateError::Failed("fail this sentence once".into()))
            } else {
                Ok(format!("English<{text}>"))
            }
        }

        fn engine(&self) -> &str {
            "flaky-batch"
        }
    }

    fn tokens(surfaces: &[&str]) -> Vec<Token> {
        surfaces
            .iter()
            .map(|surface| Token {
                surface: (*surface).into(),
                ..Token::default()
            })
            .collect()
    }

    #[test]
    fn sentence_translations_preserve_exact_ranges_and_source_order() {
        let engine = Arc::new(CountingTranslator::echo());
        let state = TranslatorState::with_engine(engine.clone());
        let translated = state
            .sentence_translations(&tokens(&["猫", "。", "犬", "？", "まだ", "途中"]))
            .expect("the batch translates");

        assert_eq!(
            translated,
            [
                SentenceTranslation {
                    first_token: 0,
                    last_token_end: 2,
                    japanese: "猫。".into(),
                    english: "English<猫。>".into(),
                },
                SentenceTranslation {
                    first_token: 2,
                    last_token_end: 4,
                    japanese: "犬？".into(),
                    english: "English<犬？>".into(),
                },
                SentenceTranslation {
                    first_token: 4,
                    last_token_end: 6,
                    japanese: "まだ途中".into(),
                    english: "English<まだ途中>".into(),
                },
            ]
        );
        assert_eq!(engine.calls.load(Ordering::SeqCst), 3);
        assert!(state
            .sentence_translations(&[])
            .expect("empty is a complete empty batch")
            .is_empty());
    }

    #[test]
    fn comparison_baseline_reuses_the_canonical_sentence_translation() {
        let engine = Arc::new(CountingTranslator::echo());
        let state = TranslatorState::with_engine(engine.clone());
        let source = tokens(&["猫", "が", "いる", "。", "犬", "も", "いる", "。"]);
        let sentences = state
            .sentence_translations(&source)
            .expect("aligned translation succeeds");
        let picked = [1].into_iter().collect();
        let variants = sakatsu_core::translate::variants(&source, &picked)
            .expect("selection is valid")
            .expect("selection exists");

        assert_eq!(
            state.remembering(&variants.full).unwrap(),
            sentences[0].english
        );
        assert_eq!(
            engine.calls.load(Ordering::SeqCst),
            2,
            "baseline was a cache hit"
        );
    }

    #[test]
    fn concurrent_identical_misses_decode_once() {
        let engine = Arc::new(CountingTranslator::echo());
        let state = Arc::new(TranslatorState::with_engine(engine.clone()));
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let state = state.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    state.remembering("同じ文。").expect("translation succeeds")
                })
            })
            .collect();

        for worker in workers {
            assert_eq!(
                worker.join().expect("worker did not panic"),
                "English<同じ文。>"
            );
        }
        assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_results_are_not_cached() {
        let engine = Arc::new(CountingTranslator::failing());
        let state = TranslatorState::with_engine(engine.clone());

        assert!(state.remembering("失敗。").is_err());
        assert!(state.remembering("失敗。").is_err());
        assert_eq!(engine.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn retry_reuses_sentences_cached_before_a_later_batch_failure() {
        let engine = Arc::new(FlakyBatchTranslator::default());
        let state = TranslatorState::with_engine(engine.clone());
        let source = tokens(&["猫", "。", "犬", "。"]);

        assert!(state.sentence_translations(&source).is_err());
        let retry = state
            .sentence_translations(&source)
            .expect("the failed sentence succeeds on Retry");

        assert_eq!(
            retry.len(),
            2,
            "the batch becomes visible only when complete"
        );
        assert_eq!(
            engine.calls_for("猫。"),
            1,
            "the successful prefix was cached"
        );
        assert_eq!(
            engine.calls_for("犬。"),
            2,
            "only the failed sentence decoded again"
        );
    }
}

/// Take whatever is on the clipboard: text as-is, an image through the engine.
#[tauri::command(async)]
pub fn recognize_clipboard(
    state: State<'_, OcrState>,
    request: Option<RequestIdentity>,
) -> Result<String, String> {
    match capture::read_clipboard()? {
        // Already text — running it through OCR could only make it worse.
        capture::Clipboard::Text(text) => Ok(text.trim().to_string()),
        capture::Clipboard::Image(image) => recognize_scheduled(&state, &image, request),
    }
}

fn recognize_scheduled(
    ocr: &OcrState,
    image: &OcrImage,
    request: Option<RequestIdentity>,
) -> Result<String, String> {
    let lane = if ocr.chosen_name() == "vlm" {
        crate::work::Lane::Vlm
    } else {
        crate::work::Lane::Ocr
    };
    let identity = request.clone();
    let permit = crate::work::scheduler()
        .enter(lane, request)
        .map_err(|error| error.to_string())?;
    let result = recognize(ocr, image, identity.as_ref());
    permit.checkpoint().map_err(|error| error.to_string())?;
    result
}

/// Record what a recognition request used, with the acceleration the engine reports now.
fn record_recognition(
    ocr: &OcrState,
    request: Option<&RequestIdentity>,
    produced: Result<&str, sakatsu_core::runtime_status::FailureCode>,
) {
    let acceleration = ocr.status().acceleration;
    ocr.last.record(crate::runtime_status::recognition_result(
        request,
        produced,
        acceleration,
    ));
}

/// The one place every recognition goes through — frame, clipboard image and file alike.
///
/// Which makes it the one place worth timing. "OCR feels slow" and "OCR got it wrong" are
/// different reports and the log has to be able to tell them apart, so a line here carries
/// the engine, the size it was handed, how long it took and how much came back.
fn recognize(
    ocr: &OcrState,
    image: &OcrImage,
    request: Option<&RequestIdentity>,
) -> Result<String, String> {
    let engine = match ocr.engine() {
        Ok(engine) => engine,
        Err(error) => {
            record_recognition(
                ocr,
                request,
                Err(sakatsu_core::runtime_status::FailureCode::EngineUnavailable),
            );
            return Err(error);
        }
    };
    let started = std::time::Instant::now();
    // Attributed: for Auto, the model that answered rather than "auto".
    let result = engine.recognize_attributed(image);
    let elapsed = started.elapsed();

    match result {
        Ok((text, produced_by)) => {
            record_recognition(ocr, request, Ok(&produced_by));
            let text = text.trim().to_string();
            sakatsu_core::log_info!(
                "ocr",
                "{} read {}x{} in {:.2}s → {} chars",
                engine.engine(),
                image.width(),
                image.height(),
                elapsed.as_secs_f64(),
                text.chars().count()
            );
            sakatsu_core::log_text!("ocr", "{} read: {text:?}", engine.engine());
            // **The one place a passage is collected, because it is the one the switch names.**
            // Clipboard lines and reference-box lookups are reading the learner did not choose to
            // capture; a corpus of those would be a transcript. See `sakatsu_core::corpus`.
            if sakatsu_core::corpus::collect(&text) {
                sakatsu_core::log_info!("corpus", "kept a new passage");
            }
            Ok(text)
        }
        Err(error) => {
            record_recognition(ocr, request, Err((&error).into()));
            sakatsu_core::log_error!(
                "ocr",
                "{} failed on {}x{} after {:.2}s: {error}",
                engine.engine(),
                image.width(),
                image.height(),
                elapsed.as_secs_f64()
            );
            // **Only after a read has actually failed**, which is the difference between help
            // and noise: a custom endpoint that answers is nobody's problem, and warning about
            // one at load time would fire on every capture that was working fine.
            //
            // `core::ocr::vlm` reports the refusal accurately and cannot do better, because it
            // knows a URL and nothing else. This layer knows whether a managed install is
            // sitting on a different port, which is the one case where "is the server running?"
            // sends a reader looking in entirely the wrong place.
            Err(match ocr.vlm_endpoint_advice() {
                Some(advice) => format!(
                    "{error}

{advice}"
                ),
                None => error.to_string(),
            })
        }
    }
}
