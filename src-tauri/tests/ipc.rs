//! The IPC boundary: what the UI is allowed to rely on.
//!
//! These live in `tests/` rather than a `#[cfg(test)]` module because only
//! integration-test targets can receive the Common-Controls 6 manifest that
//! linking Tauri needs on Windows — see `build.rs`.

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, StubAnalyzer};
use sakatsu_core::dict::context::ContextualWordView;
use sakatsu_core::dict::TagGlossary;
use sakatsu_core::model::{
    Alternative, Analysis, Conjugation, Construction, ConstructionGrammar, ConstructionIdentity,
    ConstructionLookup, LineBreak, Prop, Sense, SourceSpan, Token,
};
use sakatsu_core::store::WordStatus;
use sakatsu_core::study::StudyView;
use sakatsu_desktop_lib::commands::{
    AnalyzerState, AudioState, Dictionaries, DictionaryState, OcrState, StoreState, TranslatorState,
};
use sakatsu_desktop_lib::live::Live;
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;

/// Send one IPC request the way the webview does, through an app built with the
/// same `invoke_handler` as `run()`. This covers what calling the command function
/// directly cannot: the command *name* and *argument names*, which the UI
/// hard-codes as strings in `invoke("analyze", { text })` and which no compiler
/// checks.
///
/// The app is managed with [`StubAnalyzer`] rather than the Ichiran analyzer `run()`
/// uses, so these tests exercise the boundary without needing a sidecar — which is
/// the point of holding the analyzer in state.
fn invoke(cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, serde_json::Value> {
    invoke_with(None, cmd, args)
}

/// As `invoke`, but with a store rooted at `store_dir`.
///
/// Each call builds its own app, so state that must survive between calls has to live
/// on disk — which is exactly what the store is for, and what these tests check.
fn invoke_with(
    store_dir: Option<&std::path::Path>,
    cmd: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    invoke_with_analyzer(store_dir, std::sync::Arc::new(StubAnalyzer), cmd, args)
}

fn invoke_with_analyzer(
    store_dir: Option<&std::path::Path>,
    analyzer: std::sync::Arc<dyn Analyzer>,
    cmd: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    let (_app, webview) = mock_app(store_dir, analyzer);
    call(&webview, cmd, args)
}

/// One app, kept alive, so a test can make several calls against the *same* managed state.
///
/// `invoke_with_analyzer` builds one per call, which is what makes it a good test of the
/// boundary and a useless one for anything that changes state and then reads it back. The
/// analyzer swap is exactly that: the whole claim is that the second call sees what the first
/// one did, in a process that never restarted.
#[allow(clippy::type_complexity)]
fn mock_app(
    store_dir: Option<&std::path::Path>,
    analyzer: std::sync::Arc<dyn Analyzer>,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri::WebviewWindow<tauri::test::MockRuntime>,
) {
    mock_app_routed(store_dir, AnalyzerState::fixed(analyzer))
}

/// The same app, on a whole analyzer route — so a test can give it a primary that fails and a
/// fallback that does not.
#[allow(clippy::type_complexity)]
fn mock_app_routed(
    store_dir: Option<&std::path::Path>,
    analyzer: AnalyzerState,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri::WebviewWindow<tauri::test::MockRuntime>,
) {
    let store = match store_dir {
        Some(dir) => sakatsu_desktop_lib::open_store(dir),
        // The degraded case: no persistence available.
        None => StoreState {
            store: None,
            error: Some("no store in this test".into()),
        },
    };

    let app = mock_builder()
        .manage(analyzer)
        // **Not the analyzer above.** `analyse_query` compares the two engines and only
        // consults this one when they differ, so registering the same instance for both would
        // silently skip the path `search_words` and `prefetch_reference` exist to take. The app
        // builds a separate built-in for the same reason — see `built_in_analyzer`.
        .manage(sakatsu_desktop_lib::built_in_analyzer())
        .manage(store)
        // `save_settings` applies the recognition choice as well as storing it, so the
        // command needs the engine state even though nothing here recognizes anything.
        .manage(OcrState::new(
            std::path::PathBuf::new(),
            std::path::PathBuf::new(),
            &sakatsu_core::store::Settings::default(),
        ))
        // No dictionaries: the commands must behave, not panic, when none are
        // imported — which is exactly the state a fresh install is in.
        .manage(DictionaryState(Live::new(std::sync::Arc::new(
            Dictionaries {
                dictionaries: Vec::new(),
                tags: TagGlossary::jmdict(),
            },
        ))))
        .manage(sakatsu_desktop_lib::commands::ParticleState::load(
            sakatsu_desktop_lib::wordnet_dir().join("categories.tsv"),
        ))
        // `search_words` takes this, so the harness has to hold one or every call to it
        // fails at runtime rather than at compile time. None installed is the state the
        // other tests here expect anyway: results keep the dictionary's order.
        .manage(sakatsu_desktop_lib::commands::FrequencyState(
            std::sync::RwLock::new(None),
        ))
        .manage(sakatsu_desktop_lib::warmup::WarmupState::default())
        .manage(sakatsu_desktop_lib::commands::KanjiState::load(
            sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv"),
        ))
        // Setup hot-adopts the optional local pronunciation pack, so the same mock that
        // proves every reloadable catalog item has an adoption path must own audio state.
        .manage(AudioState::default())
        .manage(TranslatorState::with_engine(std::sync::Arc::new(
            sakatsu_core::translate::StubTranslator,
        )))
        .invoke_handler(tauri::generate_handler![
            sakatsu_desktop_lib::commands::analyze,
            sakatsu_desktop_lib::commands::open_creator_link,
            sakatsu_desktop_lib::commands::study,
            sakatsu_desktop_lib::commands::lookup,
            sakatsu_desktop_lib::commands::lookup_in_context,
            sakatsu_desktop_lib::commands::expression_offer,
            sakatsu_desktop_lib::commands::word_status,
            sakatsu_desktop_lib::commands::prefetch_analysis,
            sakatsu_desktop_lib::commands::search_words,
            sakatsu_desktop_lib::commands::prefetch_reference,
            sakatsu_desktop_lib::commands::warmup_diagnostics,
            sakatsu_desktop_lib::commands::gloss_index_ready,
            sakatsu_desktop_lib::commands::readiness,
            sakatsu_desktop_lib::commands::catalog,
            sakatsu_desktop_lib::commands::dictionaries,
            sakatsu_desktop_lib::commands::explain_tag,
            sakatsu_desktop_lib::commands::explain_pattern,
            sakatsu_desktop_lib::commands::search_particles,
            sakatsu_desktop_lib::commands::set_word_status,
            sakatsu_desktop_lib::commands::word_counts,
            sakatsu_desktop_lib::commands::settings,
            sakatsu_desktop_lib::commands::save_settings,
            sakatsu_desktop_lib::commands::store_error,
            sakatsu_desktop_lib::commands::copy_text,
            sakatsu_desktop_lib::commands::anki_deck_info,
            sakatsu_desktop_lib::commands::anki_create_deck,
            sakatsu_desktop_lib::commands::anki_duplicate_deck,
            sakatsu_desktop_lib::commands::anki_rename_deck,
            sakatsu_desktop_lib::commands::anki_delete_deck,
            sakatsu_desktop_lib::commands::reload_analyzer,
            sakatsu_desktop_lib::commands::analyzer_engine,
            sakatsu_desktop_lib::commands::translate_sentences,
            sakatsu_desktop_lib::commands::translate,
            sakatsu_desktop_lib::commands::cancel_request,
            sakatsu_desktop_lib::commands::runtime_status,
            sakatsu_desktop_lib::commands::diagnostic_report,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build mock webview");
    (app, webview)
}

/// One call through the real IPC boundary.
fn call(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    tauri::test::get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().expect("valid url"),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| {
        body.deserialize::<serde_json::Value>()
            .expect("response deserializes")
    })
}

/// The deck panel reaches these by name, with these argument names.
///
/// The rename that renames nothing because the panel sends `to` and Rust reads `name` is exactly
/// the bug this catches, and it is one that would otherwise be found in a learner's collection.
///
/// **Every mutating call here is given a name the client refuses on its own.** A developer
/// machine is the most likely place for Anki to actually be running — it was, the first time
/// this test existed, and the earlier version of it would have created, renamed and deleted
/// decks in a real collection. An empty name never survives `check_deck_name`, which runs before
/// anything is sent, so no request leaves the process no matter what is listening on 8765.
#[test]
fn the_ui_can_reach_every_deck_command_by_name_with_its_argument_names() {
    for (command, args) in [
        ("anki_create_deck", serde_json::json!({ "name": "" })),
        (
            "anki_duplicate_deck",
            serde_json::json!({ "source": "A", "target": "" }),
        ),
        (
            "anki_rename_deck",
            serde_json::json!({ "from": "A", "to": "" }),
        ),
        ("anki_delete_deck", serde_json::json!({ "name": "" })),
    ] {
        let error = invoke(command, args).expect_err("an empty deck name is always refused");
        let message = error.as_str().unwrap_or_default();
        // The refusal proves both halves at once: Tauri deserialized the arguments under these
        // names, and the client stopped on the name rather than on the network.
        assert!(
            message.contains("that name will not work"),
            "{command} never got as far as checking the name, so it is either unregistered or \
             its arguments are spelled differently from what the panel sends: {message}"
        );
    }

    // Read-only, so it is allowed to reach a running Anki. That it is registered is the whole
    // question; whether the machine happens to have Anki open is not this test's business.
    if let Err(error) = invoke("anki_deck_info", serde_json::json!({})) {
        let message = error.as_str().unwrap_or_default();
        assert!(
            message.contains("not reachable"),
            "anki_deck_info is not registered: {message}"
        );
    }
}

#[test]
fn the_ui_can_invoke_analyze_by_name_with_a_text_argument() {
    let response =
        invoke("analyze", serde_json::json!({ "text": "水を飲む。" })).expect("analyze succeeds");

    let analysis: Analysis = serde_json::from_value(response).expect("the response is an Analysis");
    assert_eq!(analysis.text, "水を飲む。");
    assert_eq!(analysis.tokens.len(), 1);
}

#[test]
fn the_ui_can_invoke_sentence_translation_with_the_token_argument() {
    let response = invoke(
        "translate_sentences",
        serde_json::json!({ "tokens": Vec::<Token>::new() }),
    )
    .expect("an empty sentence batch needs no installed model");

    assert_eq!(response, serde_json::json!([]));
}

#[test]
fn request_identity_and_cancellation_cross_the_real_handler() {
    let (_app, webview) = mock_app(None, std::sync::Arc::new(StubAnalyzer));
    let request = serde_json::json!({
        "owner": "ipc-window",
        "operation": "translation",
        "generation": 73,
        "priority": "foreground",
    });

    call(
        &webview,
        "cancel_request",
        serde_json::json!({ "request": request.clone() }),
    )
    .expect("cancellation command and argument are registered");
    let error = call(
        &webview,
        "translate",
        serde_json::json!({ "text": "水", "request": request }),
    )
    .expect_err("cancelled work never reaches the translator");
    assert_eq!(error, serde_json::json!("request cancelled"));

    let current = serde_json::json!({
        "owner": "ipc-window",
        "operation": "translation",
        "generation": 74,
        "priority": "foreground",
    });
    let error = call(
        &webview,
        "translate",
        serde_json::json!({ "text": "水", "request": current }),
    )
    .expect_err("the fixture has no translation model");
    assert!(
        error
            .as_str()
            .is_some_and(|message| message.contains("translation unavailable")),
        "the newer request reached the engine instead of inheriting cancellation: {error}"
    );

    let lookup = serde_json::json!({
        "owner": "ipc-window",
        "operation": "lookup",
        "generation": 1,
        "priority": "foreground",
    });
    call(
        &webview,
        "cancel_request",
        serde_json::json!({ "request": lookup.clone() }),
    )
    .expect("lookup cancellation crosses the same command boundary");
    let token = serde_json::json!({ "surface": "水", "dict_form": "水" });
    let error = call(
        &webview,
        "lookup_in_context",
        serde_json::json!({ "tokens": [token], "index": 0, "request": lookup }),
    )
    .expect_err("cancelled selection work is refused before dictionary lookup");
    assert_eq!(error, serde_json::json!("request cancelled"));
}

#[test]
fn the_ui_can_invoke_copy_text_by_name_without_touching_the_clipboard_for_blank_text() {
    let error = invoke("copy_text", serde_json::json!({ "text": "" }))
        .expect_err("blank copy is rejected before opening the clipboard");
    assert!(error.to_string().contains("there is no text to copy"));
}

/// The About page reaches its two creator links by name, and nothing else gets through.
///
/// **Only refusals are sent.** `github` and `kofi` would open a real browser on the machine running
/// the suite, so the mapping is pinned by `commands::creator_links` instead and this asks only the
/// question a unit test cannot: that the command is registered under this name, with this argument
/// name, and refuses before launching anything.
#[test]
fn the_creator_link_command_is_registered_and_refuses_anything_but_its_two_names() {
    for other in ["elsewhere", "https://example.com", ""] {
        let error = invoke(
            "open_creator_link",
            serde_json::json!({ "destination": other }),
        )
        .expect_err("only github and kofi open anything");
        let message = error.as_str().unwrap_or_default();
        assert!(
            message.contains("there is no creator link called"),
            "open_creator_link is unregistered, or its argument is not `destination`, or {other:?} \
             reached the launcher: {message}"
        );
    }
}

#[test]
fn an_analyzer_error_reaches_the_ui_as_a_message_not_a_panic() {
    // The degradation path the Analyzer trait exists for: a failure arrives as a
    // rejected promise carrying readable text, never a crashed command.
    let error = invoke("analyze", serde_json::json!({ "text": "   " }))
        .expect_err("blank input is an error");
    assert_eq!(error, serde_json::json!("no text to analyze"));
}

/// An engine that always fails the same way, for the routed tests below.
struct Failing {
    engine: &'static str,
    error: sakatsu_core::analyzer::AnalyzeError,
}

impl Analyzer for Failing {
    fn analyze(
        &self,
        _: &str,
    ) -> Result<sakatsu_core::model::Analysis, sakatsu_core::analyzer::AnalyzeError> {
        Err(self.error.clone())
    }
    fn engine(&self) -> &str {
        self.engine
    }
}

fn routed(
    primary: std::sync::Arc<dyn Analyzer>,
    fallback: Option<std::sync::Arc<dyn Analyzer>>,
) -> AnalyzerState {
    AnalyzerState::new(sakatsu_desktop_lib::route::Route {
        policy: sakatsu_core::model::AnalyzerPolicy::Automatic,
        primary,
        fallback,
        readiness: sakatsu_desktop_lib::route::Readiness::Ready,
        generation: 0,
    })
}

/// **The whole command, not the core function**: a failing Ichiran is retried once on the real
/// built-in analyzer, and the view the UI receives says so.
#[test]
fn study_retries_a_failed_primary_on_the_built_in_analyzer_and_records_it() {
    let (_app, webview) = mock_app_routed(
        None,
        routed(
            std::sync::Arc::new(Failing {
                engine: "ichiran",
                error: sakatsu_core::analyzer::AnalyzeError::TimedOut("no answer".into()),
            }),
            Some(sakatsu_desktop_lib::built_in_analyzer().0),
        ),
    );
    let view = call(
        &webview,
        "study",
        serde_json::json!({ "text": "水を飲む。" }),
    )
    .expect("the retry answers");
    let execution = &view["analysis"]["execution"];
    assert_eq!(execution["requested"], "automatic");
    assert_eq!(execution["engine"], "lindera");
    assert_eq!(execution["fallback"]["reason"], "timed_out");
    assert_eq!(execution["fallback"]["scope"], "passage");
    assert!(
        view["analysis"]["tokens"]
            .as_array()
            .is_some_and(|tokens| !tokens.is_empty()),
        "the built-in analyzer's tokens reached the UI"
    );
}

#[test]
fn study_names_both_engines_when_the_retry_fails_too() {
    let (_app, webview) = mock_app_routed(
        None,
        routed(
            std::sync::Arc::new(Failing {
                engine: "ichiran",
                error: sakatsu_core::analyzer::AnalyzeError::BadOutput("unexpected JSON".into()),
            }),
            Some(std::sync::Arc::new(Failing {
                engine: "lindera",
                error: sakatsu_core::analyzer::AnalyzeError::EngineUnavailable(
                    "broken build".into(),
                ),
            })),
        ),
    );
    let error = call(
        &webview,
        "study",
        serde_json::json!({ "text": "水を飲む。" }),
    )
    .expect_err("both failed");
    let said = error.as_str().expect("a message");
    assert!(
        said.contains("ichiran could not analyze this passage")
            && said.contains("lindera could not either"),
        "{said}"
    );
}

/// A passage the primary read carries its execution too — engine named, no fallback.
#[test]
fn study_records_the_engine_that_answered() {
    let view = invoke("study", serde_json::json!({ "text": "水を飲む。" })).expect("study");
    let execution = &view["analysis"]["execution"];
    assert_eq!(execution["engine"], "stub");
    assert!(execution["fallback"].is_null());
}

#[test]
fn study_returns_one_breakdown_and_status_per_token() {
    let response = invoke("study", serde_json::json!({ "text": "水を飲む。" })).expect("study");

    let tokens = response["analysis"]["tokens"]
        .as_array()
        .expect("tokens")
        .len();
    let breakdowns = response["breakdowns"].as_array().expect("breakdowns").len();
    let statuses = response["statuses"].as_array().expect("statuses").len();
    assert_eq!(
        (tokens, tokens),
        (breakdowns, statuses),
        "the UI indexes both by token, so all three must be parallel"
    );
    assert!(response["patterns"].is_array());
    assert_eq!(
        response["sentences"],
        serde_json::json!([{ "first_token": 0, "last_token_end": tokens }]),
        "the UI receives the core's sentence partition rather than rebuilding it"
    );
    // With no store, every word is simply unmarked.
    assert_eq!(response["statuses"][0], "new");
}

/// A scratch directory that cleans itself up.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        // Keyed on the process id like every other scratch path here: two overlapping
        // `cargo test` runs share `temp_dir()`, and each one's cleanup deletes the
        // other's fixture mid-test. It surfaced as PermissionDenied from a different
        // test each run, which reads like a real fault and is not one.
        let path =
            std::env::temp_dir().join(format!("sakatsu-ipc-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Scratch(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn marking_a_word_survives_into_the_next_analysis() {
    let scratch = Scratch::new("status");
    let dir = Some(scratch.path());
    // The key is term **and** reading, so a mark only comes back for the same pair.
    // The stub analyzer supplies no reading, so neither does this — with real Ichiran
    // both sides carry みず and match just the same. That precision is deliberate:
    // 上手 read じょうず must not inherit a mark made on 上手 read うわて.
    let token = serde_json::json!({ "surface": "水", "dict_form": "水" });

    let stored = invoke_with(
        dir,
        "set_word_status",
        serde_json::json!({ "token": token, "status": "known" }),
    )
    .expect("set_word_status");
    assert_eq!(stored, "known");

    // A *different* app instance, reading the same directory: the whole point of the
    // store is that this survives.
    let view = invoke_with(dir, "study", serde_json::json!({ "text": "水" })).expect("study");
    assert_eq!(view["statuses"][0], "known");

    let counts = invoke_with(dir, "word_counts", serde_json::json!({})).expect("word_counts");
    assert_eq!(counts["known"], 1);
    assert_eq!(counts["seen"], 0);
}

#[test]
fn settings_round_trip_through_the_store() {
    let scratch = Scratch::new("settings");
    let dir = Some(scratch.path());

    let defaults = invoke_with(dir, "settings", serde_json::json!({})).expect("settings");
    assert_eq!(defaults["furigana"], true);
    assert_eq!(defaults["preload_ichiran_on_startup"], true);
    assert_eq!(defaults["preload_ocr_on_startup"], true);
    assert_eq!(defaults["mark_seen_on_focus"], true);
    assert_eq!(defaults["focus_before_open"], true);
    assert_eq!(defaults["show_frequency_source"], false);
    // The kind wheel crosses as a number the UI derives every sense colour from. It was missing
    // from the type on this side of the boundary once already — two hand-written copies of the
    // same struct — so it is asserted here rather than assumed.
    assert_eq!(defaults["theme"]["kind_spin"], 0.0);

    invoke_with(
        dir,
        "save_settings",
        serde_json::json!({
            "settings": {
                "furigana": false,
                "preload_ichiran_on_startup": false,
                "preload_ocr_on_startup": false,
                "mark_seen_on_focus": false,
                "focus_before_open": false,
                "split_ratio": 0.6,
                "show_frequency_source": true,
                "theme": { "mode": "dark", "kind_spin": 97.0 }
            }
        }),
    )
    .expect("save_settings");

    let saved = invoke_with(dir, "settings", serde_json::json!({})).expect("settings");
    assert_eq!(saved["furigana"], false);
    assert_eq!(saved["preload_ichiran_on_startup"], false);
    assert_eq!(saved["preload_ocr_on_startup"], false);
    assert_eq!(saved["mark_seen_on_focus"], false);
    assert_eq!(saved["focus_before_open"], false);
    assert_eq!(saved["split_ratio"], 0.6);
    assert_eq!(saved["theme"]["kind_spin"], 97.0);
    assert_eq!(saved["show_frequency_source"], true);
}

/// A rejected save must not repaint windows or reconfigure engines before saying it failed.
/// Observe events on this app rather than process-global hook settings, which other IPC tests
/// also change. The successful retry proves the listener is live, not merely silent.
#[test]
fn failed_settings_do_not_apply_runtime_changes() {
    use std::sync::{Arc, Mutex};
    use tauri::Listener;

    let scratch = Scratch::new("settings-effects");
    std::fs::create_dir_all(scratch.path()).unwrap();
    let original = r#"{"anki_deck":"Kept"}"#;
    std::fs::write(scratch.path().join("settings.json"), original).unwrap();
    let backup = scratch.path().join("settings.json.v0");
    std::fs::create_dir(&backup).unwrap();
    let (_app, webview) = mock_app(Some(scratch.path()), Arc::new(StubAnalyzer));
    let observed = Arc::new(Mutex::new(Vec::new()));
    for name in [
        sakatsu_desktop_lib::commands::THEME_CHANGED,
        sakatsu_desktop_lib::commands::FRAME_TINT,
    ] {
        let observed = Arc::clone(&observed);
        webview.listen(name, move |event| {
            observed.lock().unwrap().push(event.payload().to_owned());
        });
    }
    let args = serde_json::json!({"settings": {"theme": {"mode":"dark"}, "frame_tint":0.2}});
    let error = call(&webview, "save_settings", args.clone()).expect_err("backup refused");
    assert_eq!(error["code"], "settings_read_only");
    assert_eq!(
        std::fs::read_to_string(scratch.path().join("settings.json")).unwrap(),
        original
    );
    assert!(
        observed.lock().unwrap().is_empty(),
        "a rejected save applied runtime settings"
    );

    std::fs::remove_dir(&backup).unwrap();
    call(&webview, "save_settings", args).expect("backup now succeeds");
    assert_eq!(
        observed.lock().unwrap().len(),
        2,
        "successful saves still update the windows"
    );
}

/// **What the store corrected has to come back, or the drawer shows a lie.**
///
/// `Settings::normalise` forces `hide_during_capture` on the moment a tint is set — a wash the
/// frame is still wearing when the picture is taken is a wash the recognizer reads. Nothing
/// reported that, so the checkbox stayed unticked while every capture flickered, and
/// unticking it did nothing: the next save sent the same stale `false` back.
#[test]
fn saving_returns_what_the_store_actually_kept() {
    let scratch = Scratch::new("normalised");
    let dir = Some(scratch.path());

    let echoed = invoke_with(
        dir,
        "save_settings",
        serde_json::json!({
            "settings": { "frame_tint": 0.2, "hide_during_capture": false }
        }),
    )
    .expect("save_settings");

    assert_eq!(
        echoed["hide_during_capture"], true,
        "the tint's precondition was applied but not reported: {echoed}"
    );
    // And it is the same thing the store will hand out next time it is asked.
    let stored = invoke_with(dir, "settings", serde_json::json!({})).expect("settings");
    assert_eq!(stored["hide_during_capture"], true);

    // Clamped on the way out too — the frame's tint event carries this number.
    let wild = invoke_with(
        dir,
        "save_settings",
        serde_json::json!({ "settings": { "frame_tint": 9.0 } }),
    )
    .expect("save_settings");
    assert_eq!(wild["frame_tint"], 0.5, "past a light wash it is a curtain");
}

#[test]
fn without_a_store_marking_a_word_says_so_rather_than_silently_failing() {
    // The learner must never believe a mark was saved when it was not.
    let token = serde_json::json!({ "surface": "水", "dict_form": "水" });
    let error = invoke(
        "set_word_status",
        serde_json::json!({ "token": token, "status": "known" }),
    )
    .expect_err("should refuse");

    let reported = invoke("store_error", serde_json::json!({})).expect("store_error");
    assert!(reported.is_string(), "the UI needs a reason to show");

    // **A structured rejection, and this is the transport contract.** The command used to reject
    // with a bare string, and the interface told "this failed for the reason already on screen"
    // apart from "this is something new" by comparing that string against the standing warning.
    // That worked and made behaviour depend on prose: rewording the sentence in Rust would have
    // silently changed what the frontend drew.
    //
    // So both halves are pinned. The `code` is what the interface branches on and must be exactly
    // this string; the `message` is what it prints, and is the store's own sentence passed through
    // whole — the same one `store_error` reports, which is what lets the standing warning and the
    // refusal be recognised as one fact without either being parsed.
    assert_eq!(
        error["code"], "store_unavailable",
        "the code is the contract; got {error}"
    );
    assert_eq!(
        error["message"], reported,
        "the message is the store's own sentence, not a wrapper around it"
    );
}

/// A store that opened and declined **this file** is a different answer from no store at all.
///
/// The interface has to draw them differently: `store_unavailable` is already explained by a
/// standing warning and gets a short line, while this one is on screen nowhere else and must keep
/// its own message. Deciding that by reading the prose is what the codes replaced, so the
/// distinction is asserted here at the boundary where the frontend actually meets it.
#[test]
fn a_settings_file_from_a_newer_build_is_refused_with_its_own_code() {
    let scratch = Scratch::new("settings-read-only");
    std::fs::create_dir_all(scratch.path()).expect("scratch");
    // Readable, and newer. `Store::open` reads what it can and marks the settings read-only; it is
    // the *save* that must refuse, which is the half a naive implementation gets wrong.
    let future = format!(
        r#"{{"version": {}, "settings": {{"anki_deck": "From Tomorrow"}}}}"#,
        sakatsu_core::store::settings_file::SETTINGS_VERSION + 7
    );
    std::fs::write(scratch.path().join("settings.json"), &future).expect("write settings");

    let error = invoke_with(
        Some(scratch.path()),
        "save_settings",
        serde_json::json!({ "settings": sakatsu_core::store::Settings::default() }),
    )
    .expect_err("a future-version file must not be overwritten");

    assert_eq!(
        error["code"], "settings_read_only",
        "not store_unavailable: the store opened. got {error}"
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("newer version"),
        "the reason has to reach the reader, since nothing else on screen says it: {error}"
    );

    // And the file is untouched, which is the point of refusing.
    assert_eq!(
        std::fs::read_to_string(scratch.path().join("settings.json")).expect("read back"),
        future,
        "a refused save must leave the original byte-identical"
    );

    // The standing warning is *not* set for this: the store opened. An interface that showed one
    // would be telling the reader nothing can be saved when only this file is protected.
    let reported = invoke_with(Some(scratch.path()), "store_error", serde_json::json!({}))
        .expect("store_error");
    assert!(
        reported.is_string(),
        "the note explaining the newer file is still surfaced"
    );
}

/// A store that opened saves, and the reply is the status now stored rather than the one asked for.
///
/// The success shape is part of what the transport change must not disturb: the command still
/// answers with a bare `WordStatus`, not with an envelope.
#[test]
fn a_store_that_opened_saves_and_answers_with_what_it_holds() {
    let scratch = Scratch::new("typed-success");
    let token = serde_json::json!({ "surface": "水", "dict_form": "水" });
    let stored = invoke_with(
        Some(scratch.path()),
        "set_word_status",
        serde_json::json!({ "token": token, "status": "known" }),
    )
    .expect("a store that opened saves");

    assert_eq!(
        stored,
        serde_json::json!("known"),
        "the success shape is unchanged by the typed rejection"
    );
}

#[test]
fn lookup_with_no_dictionaries_is_an_empty_view_not_an_error() {
    // The state a fresh install is in, before anything has been fetched.
    let token = serde_json::json!({ "surface": "水", "dict_form": "水" });
    let view = invoke("lookup", serde_json::json!({ "token": token })).expect("lookup");

    assert_eq!(view["headword"], "水");
    assert_eq!(view["inline"].as_array().map(Vec::len), Some(0));
    // snake_case, not camelCase: serde emits the Rust field names verbatim, and the
    // whole TS mirror plus the golden contract are built on that.
    assert_eq!(view["also_in"].as_array().map(Vec::len), Some(0));
    assert!(view["inline_source"].is_null());

    assert_eq!(
        invoke("dictionaries", serde_json::json!({})).expect("dictionaries"),
        serde_json::json!([])
    );
}

#[test]
fn contextual_lookup_crosses_ipc_losslessly_on_a_fresh_install() {
    let token = serde_json::json!({ "surface": "水", "dict_form": "水" });
    let view = invoke(
        "lookup_in_context",
        serde_json::json!({ "tokens": [token], "index": 0 }),
    )
    .expect("contextual lookup");

    assert_eq!(view["word"]["headword"], "水");
    assert_eq!(view["word"]["inline"].as_array().map(Vec::len), Some(0));
    assert_eq!(view["ranking"]["senses"].as_array().map(Vec::len), Some(0));
    assert!(view["ranking"]["favoured"].is_null());
}

#[test]
fn the_ui_can_ask_what_a_tag_code_and_a_pattern_mean() {
    // The analyzer emits codes like v5m; the UI has to be able to render them.
    let meaning = invoke("explain_tag", serde_json::json!({ "code": "v5m" })).expect("explain_tag");
    assert_eq!(meaning, serde_json::json!("godan verb ending in む"));

    let unknown = invoke("explain_tag", serde_json::json!({ "code": "zzz" })).expect("explain_tag");
    assert!(unknown.is_null(), "an unknown code is null, not an error");

    let pattern =
        invoke("explain_pattern", serde_json::json!({ "id": "te-kuru" })).expect("explain_pattern");
    assert_eq!(pattern["pattern"], "〜てくる");
    assert!(!pattern["explanation"].as_str().unwrap_or("").is_empty());
}

#[test]
fn the_ui_can_search_particles_without_a_sentence_to_rank_them() {
    // Typed, and typed in romaji. Both have to work: the reader who needs to be told what
    // に does is often the one who cannot yet type it.
    for query in ["に", "ni", "NI"] {
        let found = invoke("search_particles", serde_json::json!({ "query": query }))
            .expect("search_particles");
        let first = &found[0];
        assert_eq!(first["surface"], "に", "{query:?} should lead with に");
        assert!(
            first["senses"].as_array().is_some_and(|s| s.len() > 1),
            "every sense is returned, because nothing here can rank them"
        );
        assert!(
            first["senses"][0]["cues"].is_null(),
            "the scoring cues are the engine's workings and must not reach the interface"
        );
    }

    // An ordinary English query must not drag a particle to the top of the results.
    let none = invoke("search_particles", serde_json::json!({ "query": "eat" }))
        .expect("search_particles");
    assert_eq!(none.as_array().map(Vec::len), Some(0));
}

/// A value of every type in the model, with every field set **explicitly** (no
/// `..Default::default()`) and every collection non-empty, so that:
///
/// - adding a field to the model breaks this fixture and forces a decision, and
/// - the serialized JSON exposes every nested type for the UI to check against.
///
/// The values are illustrative shape data — realistic, but not verified analyzer
/// output. Phase 1's golden fixtures come from real `ichiran-cli` runs.
fn every_field_populated() -> Analysis {
    Analysis {
        // Populated, fallback included, so the golden exposes `Execution` and `Fallback` for the
        // contract check rather than a bare `null`.
        execution: Some(sakatsu_core::model::Execution {
            requested: sakatsu_core::model::AnalyzerPolicy::Automatic,
            engine: "lindera".into(),
            fallback: Some(sakatsu_core::model::Fallback {
                reason: sakatsu_core::model::FallbackReason::TimedOut,
                scope: sakatsu_core::model::FallbackScope::Passage,
                detail: "ichiran: the sidecar did not answer within the budget".into(),
            }),
            // Shape data only: a real fallback passage has no repairs, because the built-in
            // analyzer composes contractions itself. Populated so the golden exposes `RepairNote`.
            repairs: vec![sakatsu_core::model::RepairNote {
                start: 0,
                end: 15,
                family: sakatsu_core::model::ContractionFamily::TeOru,
                nasal: false,
                lemma: "間違う".into(),
                lemma_reading: "まちがう".into(),
                reading: "まちがっとる".into(),
                sequence: None,
                stem: sakatsu_core::model::StemEvidence::Headword,
                replaced: 2,
            }],
        }),
        text: "伝わってきます".into(),
        tokens: vec![
            Token {
                // A stub passage is one line.
                line_break: Default::default(),
                surface: "伝わってきます".into(),
                dict_form: "伝わってくる".into(),
                reading: "つたわってきます".into(),
                lemma_reading: "つたわってくる".into(),
                senses: vec![Sense {
                    pos: "[v5r,vi]".into(),
                    gloss: "to be conveyed; to be transmitted".into(),
                    sequence: None,
                }],
                conjugation: Some(Conjugation {
                    lemma: "伝わる".into(),
                    reading: "つたわる".into(),
                    props: vec![Prop {
                        pos: "v5r".into(),
                        form: "Conjunctive (~te)".into(),
                        neg: false,
                        fml: false,
                        via_depth: 0,
                        base_reading: "つたわる".into(),
                        is_base: true,
                    }],
                }),
                selectable: true,
                components: vec![
                    Token {
                        line_break: Default::default(),
                        surface: "伝わって".into(),
                        dict_form: "伝わる".into(),
                        reading: "つたわって".into(),
                        lemma_reading: "つたわる".into(),
                        senses: vec![Sense {
                            pos: "[v5r,vi]".into(),
                            gloss: "to be conveyed".into(),
                            sequence: None,
                        }],
                        conjugation: Some(Conjugation {
                            lemma: "伝わる".into(),
                            reading: "つたわる".into(),
                            props: vec![Prop {
                                pos: "v5r".into(),
                                form: "Conjunctive (~te)".into(),
                                neg: false,
                                fml: false,
                                via_depth: 0,
                                base_reading: "つたわる".into(),
                                is_base: true,
                            }],
                        }),
                        selectable: true,
                        components: vec![],
                        constructions: vec![],
                        lexical_role: None,
                        alternatives: vec![],
                        start: Some(0),
                        end: Some(12),
                        confidence: Some(128),
                        romaji: Some("tsutawatte".into()),
                        reading_unverified: false,
                        reading_ambiguous: false,
                    },
                    Token {
                        line_break: Default::default(),
                        surface: "きます".into(),
                        dict_form: "来る".into(),
                        reading: "きます".into(),
                        lemma_reading: "くる".into(),
                        senses: vec![Sense {
                            pos: "[vk,vi]".into(),
                            gloss: "to come".into(),
                            sequence: None,
                        }],
                        conjugation: Some(Conjugation {
                            lemma: "来る".into(),
                            // Normalized to hiragana — Ichiran writes this as 【クる】.
                            reading: "くる".into(),
                            props: vec![Prop {
                                pos: "vk".into(),
                                form: "Non-past".into(),
                                neg: false,
                                fml: true,
                                via_depth: 0,
                                base_reading: "くる".into(),
                                is_base: true,
                            }],
                        }),
                        selectable: true,
                        components: vec![],
                        constructions: vec![],
                        lexical_role: None,
                        alternatives: vec![],
                        start: Some(12),
                        end: Some(21),
                        confidence: Some(64),
                        romaji: Some("kimasu".into()),
                        reading_unverified: false,
                        reading_ambiguous: true,
                    },
                ],
                constructions: vec![
                    Construction {
                        identity: ConstructionIdentity::ContractedMotion,
                        source: SourceSpan { start: 0, end: 21 },
                        components: vec![0, 1],
                        lookup: ConstructionLookup::Headword,
                        grammar: ConstructionGrammar::Auxiliary,
                        grammar_pattern: "te-iku".into(),
                    },
                    Construction {
                        identity: ConstructionIdentity::ExplanatoryNan,
                        source: SourceSpan { start: 0, end: 21 },
                        components: vec![0, 1],
                        lookup: ConstructionLookup::GrammarPattern,
                        grammar: ConstructionGrammar::Sentence,
                        grammar_pattern: "explanatory-na-n".into(),
                    },
                    Construction {
                        identity: ConstructionIdentity::CopulaRecall,
                        source: SourceSpan { start: 0, end: 21 },
                        components: vec![0, 1],
                        lookup: ConstructionLookup::Headword,
                        grammar: ConstructionGrammar::Sentence,
                        grammar_pattern: "hanabira-n3-kke".into(),
                    },
                ],
                lexical_role: Some(sakatsu_core::model::LexicalRole::Pronoun),
                alternatives: vec![Alternative {
                    lemma: "伝わる".into(),
                    reading: "つたわる".into(),
                    form: "Continuative (stem)".into(),
                    senses: vec![Sense {
                        pos: "[v5r,vi]".into(),
                        gloss: "to be conveyed".into(),
                        sequence: None,
                    }],
                }],
                start: Some(0),
                end: Some(21),
                confidence: Some(384),
                romaji: Some("tsutawatte kimasu".into()),
                reading_unverified: true,
                reading_ambiguous: false,
            },
            // **The bare token, and it is here on purpose.** Every collection above is non-empty,
            // which exposes each element type to the contract check but leaves the *empty* case
            // uncovered — and empty is the common case in real output: most tokens have no
            // components, no alternatives, and punctuation has no senses at all. A golden whose
            // collections are always populated says nothing about what the UI does with `[]`.
            //
            // It also carries the `Option`s in their `None` state, which the populated token cannot.
            Token {
                line_break: LineBreak::Wrap,
                surface: "。".into(),
                dict_form: String::new(),
                reading: String::new(),
                lemma_reading: String::new(),
                senses: vec![],
                conjugation: None,
                selectable: false,
                components: vec![],
                constructions: vec![],
                lexical_role: None,
                alternatives: vec![],
                start: None,
                end: None,
                confidence: None,
                romaji: None,
                reading_unverified: false,
                reading_ambiguous: false,
            },
            // A third, so `LineBreak` reaches its last variant and `statuses` has room for the
            // vocabulary states a reader actually sees. An enum whose variants are never all
            // serialised is an enum the contract check has only partly seen.
            Token {
                line_break: LineBreak::Break,
                surface: "水".into(),
                dict_form: "水".into(),
                reading: "みず".into(),
                lemma_reading: "みず".into(),
                senses: vec![Sense {
                    pos: "[n]".into(),
                    gloss: "water".into(),
                    sequence: Some(1_589_530),
                }],
                conjugation: None,
                selectable: true,
                components: vec![],
                constructions: vec![],
                lexical_role: Some(sakatsu_core::model::LexicalRole::Suffix),
                alternatives: vec![],
                start: Some(21),
                end: Some(24),
                confidence: Some(256),
                romaji: Some("mizu".into()),
                reading_unverified: false,
                reading_ambiguous: false,
            },
        ],
    }
}

/// The whole study response, which is what actually crosses the boundary.
///
/// # Why this is not hand-built all the way down
///
/// The `Analysis` above is hand-built so that adding a field to the model breaks the fixture and
/// forces a decision. The derivations are not: `breakdowns`, `patterns` and `sentences` come from
/// the **real** functions over that analysis, because a hand-written `Breakdown` would assert a
/// shape `grammar::breakdown` might never produce — a contract check against a value the code
/// cannot emit is worse than none.
///
/// `statuses` and `ruby` are supplied here rather than derived, because deriving them needs a
/// store and a kanji table this test has no business installing. They are chosen to cover what
/// derivation would not: every `WordStatus` variant, and a ruby entry both with and without a
/// reading.
///
/// The assertions in `the_serialized_model_matches_the_golden_the_ui_checks` require this coverage
/// rather than trusting it, because a fixture that quietly stops exercising a nested type takes
/// the contract check down with it and says nothing.
fn the_whole_study_view() -> StudyView {
    let analysis = every_field_populated();
    let breakdowns = analysis
        .tokens
        .iter()
        .map(sakatsu_core::grammar::breakdown)
        .collect::<Vec<_>>();
    // **Hand-built, and unlike `breakdowns` that is safe here.** `grammar::patterns` finds nothing
    // in this fixture — the tokens do not form a pattern the core pack recognises — so deriving it
    // would leave `Match` unexposed and `model.ts`'s declaration of it checked against nothing.
    //
    // `Match` is a shallow DTO: an id, four indices, and one span per rule position, with no
    // derivation to get wrong — which is exactly what made hand-building `Breakdown` a bad idea
    // and makes this one fine. The id is a real entry in `core/src/grammar/packs/core.json`, so
    // it is not a shape that could never occur. The assertion below requires this to stay
    // non-empty.
    //
    // The element span is `particle-wa`'s single rule position, covering what the match covers.
    // Its **suffix fields are `None` on purpose**: that rule is `{surface: は}`, which has no
    // ending sitting inside a token, and inventing one here would pin a shape the matcher cannot
    // produce. What those fields hold when there *is* an ending is the matcher's own tests; what
    // the golden pins is that they exist and how they serialize when absent.
    let patterns = vec![sakatsu_core::grammar::Match {
        pattern_id: "particle-wa".into(),
        first_token: 0,
        last_token_end: 2,
        start: Some(0),
        end: Some(21),
        elements: vec![sakatsu_core::grammar::ElementSpan {
            element: 0,
            first_token: 0,
            last_token_end: 2,
            start: Some(0),
            end: Some(21),
            suffix_start: None,
            suffix_end: None,
        }],
    }];
    let sentences = sakatsu_core::model::sentence_ranges(&analysis.tokens);

    // Parallel to the tokens, and every variant represented.
    let statuses = vec![WordStatus::Known, WordStatus::New, WordStatus::Seen];
    assert_eq!(statuses.len(), analysis.tokens.len());

    let ruby = vec![
        // A word set over its kanji, plus a kana tail with no reading of its own.
        vec![
            sakatsu_core::dict::Ruby {
                text: "伝".into(),
                reading: Some("つた".into()),
            },
            sakatsu_core::dict::Ruby {
                text: "わってきます".into(),
                reading: None,
            },
        ],
        // Punctuation: one run, no reading.
        vec![sakatsu_core::dict::Ruby {
            text: "。".into(),
            reading: None,
        }],
        // A single character with a reading over it.
        vec![sakatsu_core::dict::Ruby {
            text: "水".into(),
            reading: Some("みず".into()),
        }],
    ];

    StudyView {
        analysis,
        sentences,
        breakdowns,
        patterns,
        statuses,
        ruby,
    }
}

/// The wire format is a contract with the UI, which mirrors it by hand in
/// `ui/src/model.ts`. This pins the serialized shape into `ui/src/model.golden.json`;
/// `ui/src/model.contract.ts` type-checks that mirror against the golden, so
/// `npm run check` catches drift in both directions. Serde emits every field of
/// these types (nothing is skipped), so the golden always lists the complete wire
/// format — including any field added to the model.
#[test]
fn the_serialized_model_matches_the_golden_the_ui_checks() {
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/model.golden.json");
    let view = the_whole_study_view();

    // **The fixture's coverage is asserted, not assumed.** A golden is a serialized *example*, so
    // it only checks the shapes it happens to contain — and a fixture that drifts into containing
    // fewer takes the contract check down with it silently. These say what it must reach.
    assert!(
        view.breakdowns
            .iter()
            .any(|breakdown| !breakdown.steps.is_empty()),
        "no breakdown has steps, so Step and Form are unexposed and model.ts's declarations of \
         them are checked against nothing"
    );
    assert!(
        view.breakdowns
            .iter()
            .any(|breakdown| breakdown.steps.is_empty()),
        "every breakdown has steps, so the empty case the UI meets on punctuation is uncovered"
    );
    assert!(
        view.analysis
            .tokens
            .iter()
            .any(|token| token.senses.is_empty())
            && view
                .analysis
                .tokens
                .iter()
                .any(|token| !token.senses.is_empty()),
        "both an empty and a populated collection must appear"
    );
    assert!(
        view.analysis
            .tokens
            .iter()
            .any(|token| token.conjugation.is_none())
            && view
                .analysis
                .tokens
                .iter()
                .any(|token| token.conjugation.is_some()),
        "an Option must appear in both states, or null is untested"
    );
    assert!(
        view.analysis
            .tokens
            .iter()
            .any(|token| !token.components.is_empty()),
        "the recursive token type must be exposed at least once"
    );
    for identity in [
        ConstructionIdentity::ExplanatoryNan,
        ConstructionIdentity::ContractedMotion,
        ConstructionIdentity::CopulaRecall,
    ] {
        assert!(
            view.analysis.tokens.iter().any(|token| token
                .constructions
                .iter()
                .any(|construction| construction.identity == identity)),
            "ConstructionIdentity::{identity:?} never reaches the wire"
        );
    }
    let constructions = view
        .analysis
        .tokens
        .iter()
        .flat_map(|token| &token.constructions)
        .collect::<Vec<_>>();
    for lookup in [
        ConstructionLookup::Headword,
        ConstructionLookup::GrammarPattern,
    ] {
        assert!(
            constructions
                .iter()
                .any(|construction| construction.lookup == lookup),
            "ConstructionLookup::{lookup:?} never reaches the wire"
        );
    }
    for grammar in [
        ConstructionGrammar::Sentence,
        ConstructionGrammar::Auxiliary,
    ] {
        assert!(
            constructions
                .iter()
                .any(|construction| construction.grammar == grammar),
            "ConstructionGrammar::{grammar:?} never reaches the wire"
        );
    }
    assert!(
        constructions.iter().all(|construction| {
            construction.source.start < construction.source.end
                && !construction.components.is_empty()
                && !construction.grammar_pattern.is_empty()
        }),
        "the nested construction evidence must be populated"
    );
    for variant in [WordStatus::New, WordStatus::Seen, WordStatus::Known] {
        assert!(
            view.statuses.contains(&variant),
            "WordStatus::{variant:?} never reaches the wire, so its name is unchecked"
        );
    }
    for variant in [LineBreak::None, LineBreak::Wrap, LineBreak::Break] {
        assert!(
            view.analysis
                .tokens
                .iter()
                .any(|token| token.line_break == variant),
            "LineBreak::{variant:?} never reaches the wire, so its name is unchecked"
        );
    }
    assert!(
        !view.patterns.is_empty(),
        "no grammar pattern reaches the wire, so Match is unexposed and model.ts's declaration of          it is checked against nothing"
    );
    assert!(
        view.patterns.iter().any(|matched| matched.start.is_some()),
        "a pattern must carry its byte span at least once"
    );
    assert!(
        view.ruby
            .iter()
            .flatten()
            .any(|ruby| ruby.reading.is_some())
            && view
                .ruby
                .iter()
                .flatten()
                .any(|ruby| ruby.reading.is_none()),
        "ruby must appear both with and without a reading"
    );

    let generated = serde_json::to_string_pretty(&view).expect("serialize") + "\n";

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &generated).expect("write golden");
        return;
    }

    let on_disk = std::fs::read_to_string(&golden)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", golden.display()));

    // Compare newline-insensitively: git may check the file out with CRLF.
    assert_eq!(
        on_disk.replace("\r\n", "\n"),
        generated,
        "\nThe IPC wire format changed. Regenerate the golden the UI checks against:\n  \
         PowerShell: $env:UPDATE_GOLDEN=1; cargo test -p sakatsu-desktop; \
         Remove-Item env:UPDATE_GOLDEN\n  \
         bash:       UPDATE_GOLDEN=1 cargo test -p sakatsu-desktop\n\
         Then run `npm --prefix ui run check` and fix ui/src/model.ts where it points.\n"
    );
}

/// The lookup contract, as three serialized examples.
///
/// # Why three, and why a map rather than one value
///
/// The study golden is a single `StudyView`, and it reaches both states of an `Option` because it
/// holds three tokens. One `ContextualWordView` holds one word, so a single example could only
/// ever show `reading`, `inline_source`, `name` and `romaji` in whichever state that word happened
/// to be in — and `null` would go untested for the rest. Three named cases cover them between
/// them, and each value is still exactly what the command puts on the wire for one word.
///
/// # Why these are hand-built
///
/// `WordView` is produced by a real merge over real dictionaries, and hand-building something with
/// derivation behind it is what `the_whole_study_view` warns against for `Breakdown`. The division
/// here is deliberate: **`core/tests/lookup_identity.rs` covers the derivation** — every token of
/// six passages, against an imported dictionary, compared field for field against the pre-move
/// code. What this covers is the *wire shape*, which is a different question and the only one a
/// golden can answer. Hand-building also keeps it hermetic: a golden that needed a dictionary
/// installed would be a contract check that silently stopped running on a fresh machine.
fn the_whole_lookup_contract() -> std::collections::BTreeMap<&'static str, ContextualWordView> {
    use sakatsu_core::dict::context::{RankedSense, VocabularyRanking};
    use sakatsu_core::dict::merge::{
        AlsoIn, MergedExample as DictExample, NameHit, Retrieved, Sense as MergedSense, WordView,
    };

    let ordinary = ContextualWordView {
        word: WordView {
            headword: "伝わる".into(),
            reading: Some("つたわる".into()),
            inline: vec![
                MergedSense {
                    tags: vec!["v5r".into(), "vi".into()],
                    glosses: vec!["to be handed down".into(), "to be passed down".into()],
                    // A sequence present, so the field is not only ever null.
                    sequence: Some(1_234_567),
                },
                MergedSense {
                    // A sense with no tags at all: the empty collection the chip row must cope
                    // with, beside a populated one.
                    tags: Vec::new(),
                    glosses: vec!["to spread".into()],
                    sequence: None,
                },
            ],
            inline_source: None,
            inline_retrieved: Retrieved::Component {
                query: "伝".into()
            },
            also_in: vec![AlsoIn {
                title: "Jitendex.org".into(),
                retrieved: Retrieved::Whole {
                    query: "伝わる".into(),
                },
                senses: vec![MergedSense {
                    tags: vec!["col".into()],
                    glosses: vec!["to get through".into()],
                    sequence: None,
                }],
            }],
            examples: vec![DictExample {
                japanese: "気持ちが伝わってきます。".into(),
                english: "The feeling comes across.".into(),
                retrieved: Retrieved::Component {
                    query: "伝".into()
                },
            }],
            name: None,
            likely_name: false,
            romaji: None,
        },
        ranking: VocabularyRanking {
            senses: vec![
                RankedSense {
                    original_index: 1,
                    score: 3,
                    reasons: vec!["the verb is intransitive here".into()],
                },
                RankedSense {
                    original_index: 0,
                    score: 0,
                    // The empty-reasons case: an abstention, which the panel must not draw as
                    // evidence. Beside a populated one, so both are covered.
                    reasons: Vec::new(),
                },
            ],
            favoured: Some(0),
        },
    };

    let named = ContextualWordView {
        word: WordView {
            headword: "セイア".into(),
            reading: Some("せいあ".into()),
            inline: Vec::new(),
            // A promoted definition, which is the only case `inline_source` is `Some` — and the
            // reason the field exists, so a golden without it checks the field against nothing.
            inline_source: Some("JMnedict".into()),
            inline_retrieved: Retrieved::Unknown,
            also_in: Vec::new(),
            examples: Vec::new(),
            name: Some(NameHit {
                glosses: vec!["Seia".into()],
                tags: vec!["fem".into(), "given".into()],
                source: "JMnedict".into(),
                leads: true,
                retrieved: Retrieved::Whole {
                    query: "セイア".into(),
                },
            }),
            likely_name: false,
            romaji: None,
        },
        ranking: VocabularyRanking {
            senses: Vec::new(),
            // The abstention, which is what `favoured` is null for.
            favoured: None,
        },
    };

    let unknown = ContextualWordView {
        word: WordView {
            headword: "シロコ".into(),
            // Nothing agreed on a reading, which is what `null` means here rather than "no
            // reading exists".
            reading: None,
            inline: Vec::new(),
            inline_source: None,
            inline_retrieved: Retrieved::Unknown,
            also_in: Vec::new(),
            examples: Vec::new(),
            name: None,
            likely_name: true,
            romaji: Some("shiroko".into()),
        },
        ranking: VocabularyRanking {
            senses: Vec::new(),
            favoured: None,
        },
    };

    [
        ("ordinary", ordinary),
        ("name", named),
        ("unknown_katakana", unknown),
    ]
    .into_iter()
    .collect()
}

/// The lookup wire format, pinned the way the study view is.
///
/// Regenerate with `UPDATE_GOLDEN=1` exactly as for `model.golden.json`; the message says how.
#[test]
fn the_serialized_lookup_matches_the_golden_the_ui_checks() {
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/lookup.golden.json");
    let cases = the_whole_lookup_contract();

    // **The fixture's coverage is asserted, not assumed** — the same discipline the study golden
    // uses, and for the same reason: a golden only checks the shapes it happens to contain, and a
    // fixture that drifts into containing fewer takes the check down with it silently.
    let words: Vec<_> = cases.values().map(|case| &case.word).collect();
    assert!(
        words.iter().any(|word| word.reading.is_some())
            && words.iter().any(|word| word.reading.is_none()),
        "reading must appear in both states, or null is untested"
    );
    assert!(
        words.iter().any(|word| word.inline_source.is_some())
            && words.iter().any(|word| word.inline_source.is_none()),
        "inline_source must appear in both states"
    );
    assert!(
        words.iter().any(|word| word.name.is_some())
            && words.iter().any(|word| word.name.is_none()),
        "NameHit must be exposed at least once, and absent at least once"
    );
    assert!(
        words.iter().any(|word| word.romaji.is_some())
            && words.iter().any(|word| word.romaji.is_none()),
        "romaji must appear in both states"
    );
    assert!(
        words.iter().any(|word| !word.inline.is_empty())
            && words.iter().any(|word| word.inline.is_empty()),
        "both an empty and a populated sense list must appear"
    );
    assert!(
        words.iter().any(|word| !word.also_in.is_empty()),
        "AlsoIn is never exposed, so its declaration is checked against nothing"
    );
    assert!(
        words.iter().any(|word| !word.examples.is_empty()),
        "DictExample is never exposed, so its declaration is checked against nothing"
    );
    assert!(
        words
            .iter()
            .flat_map(|word| word.inline.iter())
            .any(|sense| sense.sequence.is_some())
            && words
                .iter()
                .flat_map(|word| word.inline.iter())
                .any(|sense| sense.sequence.is_none()),
        "a sense sequence must appear in both states — it is the field that turned into a bigint"
    );
    assert!(
        cases.values().any(|case| !case.ranking.senses.is_empty()),
        "VocabularySenseRank is never exposed"
    );
    assert!(
        cases.values().any(|case| case.ranking.favoured.is_some())
            && cases.values().any(|case| case.ranking.favoured.is_none()),
        "favoured must appear in both states — the abstention is the common case"
    );

    let generated = serde_json::to_string_pretty(&cases).expect("serialize") + "\n";

    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &generated).expect("write golden");
        return;
    }

    let on_disk = std::fs::read_to_string(&golden)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", golden.display()));

    assert_eq!(
        on_disk.replace("\r\n", "\n"),
        generated,
        "\nThe lookup wire format changed. Regenerate the golden the UI checks against:\n  \
         PowerShell: $env:UPDATE_GOLDEN=1; cargo test -p sakatsu-desktop; \
         Remove-Item env:UPDATE_GOLDEN\n  \
         bash:       UPDATE_GOLDEN=1 cargo test -p sakatsu-desktop\n\
         Then run `npm --prefix ui run check` and fix ui/src/model.contract.ts where it points.\n"
    );
}

/// The reference page reaches these by name, with these argument names.
///
/// Run against a mock app with **no dictionaries imported** — a fresh install — because
/// that is the state in which a search is most likely to panic rather than answer.
#[test]
fn the_ui_can_search_words_by_name_with_a_query_argument() {
    // English, with nothing installed to search.
    let response =
        invoke("search_words", serde_json::json!({ "query": "eat" })).expect("search succeeds");
    assert_eq!(response["words"].as_array().map(Vec::len), Some(0));
    // False, not true: there is no dictionary to index, so offering to build an index
    // would send the learner after a fix for a problem they do not have.
    assert_eq!(response["needs_gloss_index"], serde_json::json!(false));
    // `kanji` and `needs_kanji_table` are gone: the characters of a result travel on the
    // result, under the word they belong to, so there is no shared pool left to report.
    assert!(
        response["kanji"].is_null(),
        "the shared kanji pool should be gone"
    );
    assert!(response["needs_kanji_table"].is_null());

    // Japanese takes the other path — through the analyzer, then the ordinary lookup.
    let japanese =
        invoke("search_words", serde_json::json!({ "query": "水" })).expect("search succeeds");
    assert!(japanese["words"].is_array());

    // An empty query is a question with no answer, not an error.
    let blank =
        invoke("search_words", serde_json::json!({ "query": "   " })).expect("search succeeds");
    assert_eq!(blank["words"].as_array().map(Vec::len), Some(0));
}

#[test]
fn speculative_warmup_uses_the_same_ipc_contract_and_answers_as_foreground_work() {
    let (_app, webview) = mock_app(None, std::sync::Arc::new(StubAnalyzer));
    let study = call(
        &webview,
        "study",
        serde_json::json!({ "text": "水を飲む。" }),
    )
    .expect("study succeeds");
    let tokens = study["analysis"]["tokens"]
        .as_array()
        .expect("study returns tokens");

    let prepared = call(
        &webview,
        "prefetch_analysis",
        serde_json::json!({ "tokens": tokens, "indices": [0] }),
    )
    .expect("analysis prefetch is registered with the UI's argument names");
    assert_eq!(prepared, serde_json::json!(1));
    let contextual = call(
        &webview,
        "lookup_in_context",
        serde_json::json!({ "tokens": tokens, "index": 0 }),
    )
    .expect("the prefetched answer remains a normal foreground answer");
    assert!(contextual["word"].is_object());
    assert!(contextual["ranking"]["senses"].is_array());

    let prepared = call(
        &webview,
        "prefetch_reference",
        serde_json::json!({ "query": "水" }),
    )
    .expect("reference prefetch is registered with the UI's argument name");
    assert_eq!(prepared, serde_json::json!(true));
    let found = call(
        &webview,
        "search_words",
        serde_json::json!({ "query": "水" }),
    )
    .expect("the prefetched Reference result remains a normal search result");
    assert!(found["words"].is_array());

    let diagnostics = call(&webview, "warmup_diagnostics", serde_json::json!({}))
        .expect("diagnostics are reachable by name");
    assert_eq!(diagnostics["analysis_entries"], serde_json::json!(1));
    assert_eq!(diagnostics["reference_entries"], serde_json::json!(1));
    assert_eq!(diagnostics["analysis_hits"], serde_json::json!(1));
    assert_eq!(diagnostics["reference_hits"], serde_json::json!(1));
}

#[test]
fn english_search_reports_itself_unavailable_when_no_dictionary_is_indexed() {
    let response = invoke("gloss_index_ready", serde_json::json!({})).expect("ready succeeds");
    assert_eq!(response, serde_json::json!(false));
}

/// Adding a component to the catalog without telling the shell where its files go would
/// produce a Get button that silently does nothing. The catalog cannot know the answer —
/// the directories carry environment overrides the shell owns — so this is the seam, and
/// it is the kind that stays broken quietly.
#[test]
fn every_fetchable_component_has_somewhere_to_put_its_files() {
    for component in sakatsu_core::setup::catalog::all() {
        let destination = sakatsu_desktop_lib::commands::destination(component.id);
        if component.fetchable() {
            assert!(
                destination.is_some(),
                "{} can be fetched but the shell has no directory for it",
                component.id
            );
        } else {
            // Ichiran: nothing is downloaded, so a directory would be a claim that
            // something is.
            assert!(destination.is_none(), "{} needs no directory", component.id);
        }
    }
}

/// The setup panel joins its rows to the catalog by id, so every row must name a real one.
///
/// Written after the first version joined them on their labels and matched four of seven:
/// the same component can use a capability-oriented name in one list and an engine name in
/// the other. The rows that missed simply had no Get button, which looks exactly like a row
/// that is not fetchable.
#[test]
fn every_readiness_row_points_at_a_catalog_entry_that_exists() {
    let response = invoke("readiness", serde_json::json!({})).expect("readiness");
    let components = response["components"].as_array().expect("components");
    assert!(!components.is_empty());

    for component in components {
        let name = component["name"].as_str().unwrap_or("?");
        let id = component["catalog"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} names no catalog entry"));
        assert!(
            sakatsu_core::setup::catalog::find(id).is_some(),
            "{name} names {id}, which is not in the catalog"
        );
    }

    // And the other direction: something offered for download that no row reports on
    // would be invisible once installed.
    let named: Vec<&str> = components
        .iter()
        .filter_map(|component| component["catalog"].as_str())
        .collect();
    for entry in sakatsu_core::setup::catalog::all() {
        assert!(
            named.contains(&entry.id),
            "{} can be fetched but no readiness row says whether it is installed",
            entry.id
        );
    }
}

/// Auto's supported setup is the pair, not whichever conventional model happened to be
/// downloaded first. Keep this at the IPC boundary: that is the shape `Setup.svelte` uses to
/// decide whether a missing row may be dismissed.
#[test]
fn both_conventional_ocr_engines_are_required_setup_components() {
    let response = invoke("readiness", serde_json::json!({})).expect("readiness");
    let components = response["components"].as_array().expect("components");

    for name in ["Recognition — manga-ocr", "Recognition — PP-OCRv6"] {
        let component = components
            .iter()
            .find(|component| component["name"] == serde_json::json!(name))
            .unwrap_or_else(|| panic!("missing {name} readiness row"));
        assert_eq!(component["required"], true, "{name} must be required");
    }

    let vlm = components
        .iter()
        .find(|component| component["name"] == serde_json::json!("Recognition — PaddleOCR-VL"))
        .expect("VLM readiness row");
    assert_eq!(vlm["required"], false, "the hidden VLM remains optional");
}

/// A selected Ichiran sidecar can be installed but temporarily unhealthy. Setup must
/// offer an escape hatch rather than report an error with no action the reader can take.
#[test]
fn an_unavailable_analyzer_has_recovery_guidance() {
    let analyzer = IchiranAnalyzer::with_command("definitely-not-a-program", ["-f"]);
    let response = invoke_with_analyzer(
        None,
        std::sync::Arc::new(analyzer),
        "readiness",
        serde_json::json!({}),
    )
    .expect("readiness");
    let component = response["components"]
        .as_array()
        .expect("components")
        .iter()
        .find(|component| component["name"] == serde_json::json!("Analyzer"))
        .expect("Analyzer row");

    assert_eq!(component["ready"], false);
    let fix = component["fix"].as_str().expect("recovery guidance");
    assert!(fix.contains("Automatic or Built-in"), "got {fix}");
}

/// Archives must land where the scripts put them, or the cross-check silently stops.
///
/// `core/tests/setup_build.rs` compares the Rust and PowerShell table builders and finds
/// its inputs under `<component>/work/`. When the app wrote them one directory up, someone
/// who set up entirely through the app had the archives on disk and that test printed
/// "SKIP" and passed — which is worse than failing, because the duplication it exists to
/// police was left unpoliced with a green tick over it.
#[test]
fn downloaded_archives_go_where_the_scripts_and_the_cross_check_expect() {
    for id in ["kanji", "wordnet", "dictionaries"] {
        let dir = sakatsu_desktop_lib::commands::destination(id).expect("a directory");
        let archives = sakatsu_desktop_lib::commands::archives_in(&dir);
        assert_eq!(
            archives,
            dir.join("work"),
            "{id}: setup_build.rs looks under work/"
        );
    }
}

/// **Changing the analyzer changes the analyzer, in a process that never restarted.**
///
/// The setting used to be read once at startup, and the settings row said so — "changing this
/// needs a restart", under a dropdown that had already changed. That is the worst shape a
/// setting can have: it saves, nothing happens, and there is no way to tell from outside
/// whether you are looking at a setting that needs a restart or one that does not work.
///
/// Three claims, and the third is the one that actually matters:
///
/// 1. the command answers with the engine it landed on,
/// 2. a *separate* call afterwards agrees, so the swap reached the managed state rather than
///    just the return value,
/// 3. and an analysis run after it is done by the new engine — which is the only claim a
///    reader can see.
///
/// `builtin` throughout: it is the one choice that resolves without asking WSL anything, so
/// this is a test of the swap rather than of whether Ichiran happens to be installed.
#[test]
fn the_analyzer_can_be_swapped_without_restarting() {
    let (_app, webview) = mock_app(None, std::sync::Arc::new(StubAnalyzer));

    let before = call(&webview, "analyzer_engine", serde_json::json!({})).expect("engine");
    assert_eq!(before, "stub", "the harness starts on the stub");

    let landed = call(
        &webview,
        "reload_analyzer",
        serde_json::json!({ "choice": "builtin" }),
    )
    .expect("reload_analyzer");
    assert_eq!(landed, "lindera", "the command says what it switched to");

    let after = call(&webview, "analyzer_engine", serde_json::json!({})).expect("engine");
    assert_eq!(
        after, "lindera",
        "and a later call agrees — the swap reached the state, not just the reply"
    );

    // The visible half. `StubAnalyzer` returns the whole input as one token; Lindera segments.
    // Analysing 水を飲む through the swapped-in engine has to give more than one token, which
    // the stub never would.
    let analysis = call(
        &webview,
        "analyze",
        serde_json::json!({ "text": "水を飲む" }),
    )
    .expect("analyze");
    let tokens = analysis["tokens"].as_array().expect("tokens");
    assert!(
        tokens.len() > 1,
        "the new engine did the analysis, not the old one — got {tokens:?}"
    );
}

/// The choice the interface sends is the choice Rust accepts.
///
/// `AnalyzerChoice` is `rename_all = "lowercase"`, so `BuiltIn` arrives as `builtin` — and the
/// `<option value>`s in `Settings.svelte` are written by hand against that. A rename on either
/// side silently becomes "this analyzer cannot be selected", which looks like the old bug this
/// whole change was made to remove.
#[test]
fn every_analyzer_the_settings_offer_is_a_choice_rust_takes() {
    for choice in ["auto", "ichiran", "builtin"] {
        let parsed: Result<sakatsu_core::store::AnalyzerChoice, _> =
            serde_json::from_value(serde_json::json!(choice));
        assert!(
            parsed.is_ok(),
            "{choice} is offered in the settings but Rust will not take it"
        );
    }
}

/// **What says it needs reloading is what actually gets reloaded.**
///
/// `Component::needs_reload` and `commands::adopt` are two lists of the same three things,
/// written in two files. If they drift, the failure is silent and nasty in exactly one
/// direction: a component that answers `true` and has no arm in `adopt` reports itself ready
/// the moment its fetch finishes, while every lookup in it goes on finding nothing until the
/// app is restarted — which is the bug this whole change removed, reintroduced for one
/// component instead of all of them.
///
/// Everything here is read-only: `adopt` re-reads tables from disk and swaps them into state.
#[test]
fn everything_that_asks_to_be_reloaded_is() {
    let (app, _webview) = mock_app(None, std::sync::Arc::new(StubAnalyzer));
    let handle = tauri::Manager::app_handle(&app);

    for component in sakatsu_core::setup::catalog::all() {
        let reloaded = sakatsu_desktop_lib::commands::adopt(handle, component);
        assert_eq!(
            component.needs_reload(),
            reloaded.is_some(),
            "{} says needs_reload = {} but adopt {} it",
            component.id,
            component.needs_reload(),
            if reloaded.is_some() {
                "handled"
            } else {
                "has no arm for"
            }
        );
    }
}

/// Every catalog entry has to give a real answer about whether it is already installed.
///
/// Both frequency lists were simply missing from `commands::installed`'s match, so they fell to
/// its default and reported not-installed for ever: the setup screen kept offering "Get 6 MB" to
/// somebody who had had the list on disk for weeks, and nothing in the interface could say
/// otherwise. Nothing failed, because a `false` there is indistinguishable from the truth.
///
/// This runs `catalog`, which asks that question once per entry. The default arm now carries a
/// `debug_assert`, and tests are a debug build, so an id with no rule fails here rather than
/// shipping a button whose label never changes. `fresh_install` covers the same ground but is
/// `#[ignore]`d — it wants an empty `APPDATA` and the internet — so the guard needed a home in
/// the ordinary suite.
#[test]
fn every_catalog_entry_reports_a_real_installed_state() {
    let response = invoke("catalog", serde_json::json!({})).expect("catalog");
    let entries = response.as_array().expect("catalog entries");

    assert_eq!(
        entries.len(),
        sakatsu_core::setup::catalog::all().len(),
        "the command must offer exactly what the catalog holds"
    );
    for entry in entries {
        let id = entry["id"].as_str().unwrap_or("?");
        assert!(
            entry["ready"].is_boolean(),
            "{id} must report an installed state"
        );
        assert!(
            !entry["label"].as_str().unwrap_or_default().is_empty(),
            "{id} must have something to call itself on the setup screen"
        );
    }
}

/// The reader works under **both** store conditions, and they are different conditions.
///
/// # Why this is not one test
///
/// The desktop checklist for this slice asks for an empty store and an unavailable one, and the
/// obvious way to produce the second — rename the store directory — produces the first.
/// `Store::open` begins with `fs::create_dir_all`, so a missing directory is created and the app
/// gets a working, empty store. `StoreState.store` is `None` only when `open` returns `Err`, which
/// needs a path that cannot be a directory at all.
///
/// Both must leave the reader working. What differs is whether the learner is warned that nothing
/// can be saved — and warning about a store that is fine, or staying silent about one that is not,
/// are both worse than the failure they describe.
#[test]
fn the_reader_works_with_an_empty_store_and_says_nothing() {
    let scratch = Scratch::new("store-empty");
    let dir = scratch.path().join("store");
    // Deliberately absent: `Store::open` creates it, which is the whole point of this case.
    assert!(!dir.exists());

    let view =
        invoke_with(Some(&dir), "study", serde_json::json!({ "text": "水" })).expect("study");
    assert!(
        !view["analysis"]["tokens"].as_array().unwrap().is_empty(),
        "the reader must read"
    );
    assert_eq!(
        view["statuses"][0], "new",
        "an empty store marks nothing, rather than failing to answer"
    );
    assert!(
        dir.is_dir(),
        "and the store was created, not merely tolerated"
    );

    let error = invoke_with(Some(&dir), "store_error", serde_json::json!({})).expect("store_error");
    assert!(
        error.is_null(),
        "an empty store is not a problem and must not be reported as one: {error:?}"
    );
}

#[test]
fn the_reader_works_with_an_unavailable_store_and_says_so() {
    let scratch = Scratch::new("store-unavailable");
    std::fs::create_dir_all(scratch.path()).expect("scratch");
    let dir = scratch.path().join("store");
    // A **file** where the directory must go: `create_dir_all` cannot proceed, so `Store::open`
    // returns `Err` and the shell degrades to no persistence. Renaming a directory would not do
    // this; it would produce the empty-store case above.
    std::fs::write(&dir, b"not a directory").expect("occupy the store path");

    let store = sakatsu_desktop_lib::open_store(&dir);
    assert!(
        store.store.is_none(),
        "a file where the store directory belongs must leave no store"
    );
    assert!(
        store.error.is_some(),
        "and the reason must be kept, so the interface can say nothing can be saved"
    );

    // The reader still reads. `mock_app` takes `None` for exactly this state.
    let view = invoke_with(None, "study", serde_json::json!({ "text": "水" })).expect("study");
    assert!(
        !view["analysis"]["tokens"].as_array().unwrap().is_empty(),
        "losing the ability to save is not a reason to lose the ability to read"
    );
    assert_eq!(
        view["statuses"][0], "new",
        "every word is unmarked, which is the honest answer when nothing is stored"
    );

    let error = invoke_with(None, "store_error", serde_json::json!({})).expect("store_error");
    assert!(
        error.is_string(),
        "and the learner is told, rather than marking words that go nowhere: {error:?}"
    );
}

/// Search extends the lookup wire object by flattening it, then adds forms and characters.
/// These cases cover populated and absent enrichment rather than asserting only an empty search.
#[test]
fn the_serialized_search_matches_the_golden_the_ui_checks() {
    use sakatsu_core::dict::Kanji;
    use sakatsu_core::search::{kanji_view, Form, Found, WordHit};
    let mut lookups = the_whole_lookup_contract();
    let ordinary = lookups.remove("ordinary").expect("ordinary fixture");
    let unknown = lookups.remove("unknown_katakana").expect("unknown fixture");
    let kanji = Kanji {
        on: vec!["\u{30b7}\u{30b8}\u{30e7}\u{30a6}".into()],
        kun: vec!["\u{3044}\u{3061}".into()],
        meanings: vec!["market".into()],
        strokes: 5,
        grade: Some(2),
        nanori: vec!["name reading".into()],
        components: vec!["part".into()],
    };
    let cases = Found {
        words: vec![
            WordHit {
                word: ordinary.word,
                forms: vec![Form {
                    text: "alternate".into(),
                    rare: true,
                }],
                chars: vec![kanji_view(
                    '\u{5e02}',
                    &kanji,
                    Some("\u{3057}\u{3058}\u{3087}\u{3046}"),
                    false,
                )],
                ranking: Some(ordinary.ranking),
                surface: Some("query".into()),
            },
            WordHit {
                word: unknown.word,
                forms: vec![],
                chars: vec![kanji_view('\u{5e02}', &Kanji::default(), None, true)],
                ranking: None,
                surface: None,
            },
        ],
        japanese: true,
        ranked_by: Some("fixture".into()),
        needs_gloss_index: false,
        capped: false,
    };
    assert!(cases.words[0].chars[0].on[0].here);
    assert!(!cases.words[0].chars[0].kun[0].here);
    assert!(cases.words[1].chars[0].grade.is_none());
    let generated = serde_json::to_string_pretty(&cases).expect("serialize") + "\n";
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/search.golden.json");
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&golden, &generated).expect("write golden");
    }
    assert_eq!(
        std::fs::read_to_string(golden)
            .expect("search golden; UPDATE_GOLDEN=1 explicitly regenerates it")
            // Git may check JSON out as CRLF on Windows. Preserve every other wire byte.
            .replace("\r\n", "\n"),
        generated
    );
}

/// A part's own status crosses the boundary by **name and argument name**, which nothing else
/// checks — the panel hard-codes both as strings.
///
/// The panel asks this for a *component* of a grouped word, because the study response carries
/// statuses parallel to the top-level tokens and a component is not among them. With no store
/// managed in this app the honest answer is `New`: nobody has marked it, which is a value rather
/// than an error, and the panel draws an unmarked word.
#[test]
fn the_ui_can_ask_for_one_words_status_by_name() {
    let scratch = Scratch::new("word-status");
    let dir = scratch.path().join("store");
    let part = serde_json::json!({
        "surface": "かっこ",
        "dict_form": "かっこ",
        "reading": "かっこ",
        "lemma_reading": "かっこ",
        "start": 0,
        "end": 9,
    });
    // The grouped word the part belongs to. A different key, and that is the point.
    let whole = serde_json::json!({
        "surface": "かっこいい",
        "dict_form": "格好いい",
        "reading": "かっこいい",
        "lemma_reading": "かっこいい",
        "start": 0,
        "end": 15,
    });

    let unmarked = invoke_with(
        Some(&dir),
        "word_status",
        serde_json::json!({ "token": part.clone() }),
    )
    .expect("word_status answers");
    assert_eq!(
        unmarked,
        serde_json::json!("new"),
        "an unmarked word is New, and the command is reachable under this exact name"
    );

    invoke_with(
        Some(&dir),
        "set_word_status",
        serde_json::json!({ "token": part.clone(), "status": "known" }),
    )
    .expect("the part marks");

    // **Across two app builds**, which is what makes this about the store rather than a cache.
    assert_eq!(
        invoke_with(
            Some(&dir),
            "word_status",
            serde_json::json!({ "token": part })
        )
        .expect("word_status answers"),
        serde_json::json!("known")
    );
    assert_eq!(
        invoke_with(
            Some(&dir),
            "word_status",
            serde_json::json!({ "token": whole })
        )
        .expect("word_status answers"),
        serde_json::json!("new"),
        "marking a part must not mark the word it is part of"
    );
}

/// The expression offer crosses the boundary by **name and argument names**, which nothing else
/// checks — the UI hard-codes them as strings.
///
/// With no dictionaries managed in this app, the honest answer is `null`: there is no entry to
/// offer. That is the distinction the command exists to keep — *no offer* is a value, not an error.
#[test]
fn the_ui_can_ask_for_an_expression_offer_by_name() {
    let token = |surface: &str, start: usize| {
        serde_json::json!({
            "surface": surface,
            "dict_form": surface,
            "start": start,
            "end": start + surface.len(),
        })
    };
    let answer = invoke(
        "expression_offer",
        serde_json::json!({
            "tokens": [token("可愛い", 0), token("の", 9), token("に", 12)],
            "index": 1,
            "matches": [{
                "pattern_id": "noni",
                "first_token": 1,
                "last_token_end": 3,
                "start": 9,
                "end": 15,
            }],
        }),
    )
    .expect("the command answers rather than failing");
    assert_eq!(
        answer,
        serde_json::Value::Null,
        "with no dictionary open there is no entry to offer, and that is an answer"
    );
}

/// A selection the reader has already left cannot be answered over the top of a newer one.
///
/// The same ownership `lookup_in_context` uses: a cancelled request is refused **before** any
/// dictionary work, so a slow offer for an abandoned token never lands.
#[test]
fn a_cancelled_selection_does_not_receive_a_late_expression_offer() {
    let (_app, webview) = mock_app(None, std::sync::Arc::new(StubAnalyzer));
    let request = serde_json::json!({
        "owner": "ipc-window",
        "operation": "expression_offer",
        "generation": 1,
        "priority": "foreground",
    });
    call(
        &webview,
        "cancel_request",
        serde_json::json!({ "request": request.clone() }),
    )
    .expect("cancellation crosses the same boundary");

    let error = call(
        &webview,
        "expression_offer",
        serde_json::json!({
            "tokens": [{ "surface": "の", "dict_form": "の", "start": 0, "end": 3 }],
            "index": 0,
            "matches": [],
            "request": request,
        }),
    )
    .expect_err("a cancelled selection is refused before any lookup");
    assert_eq!(error, serde_json::json!("request cancelled"));
}

/// What each operation used crosses the real handler, each result under its own request.
///
/// The owner is this test's own: the scheduler is shared by every test in the process, and a
/// shared owner would let another test's request supersede these.
#[test]
fn runtime_status_reports_each_operation_under_its_own_request() {
    let (_app, webview) = mock_app(None, std::sync::Arc::new(StubAnalyzer));
    let study = serde_json::json!({
        "owner": "runtime-status-ipc",
        "operation": "study",
        "generation": 1,
        "priority": "foreground",
    });
    let translation = serde_json::json!({
        "owner": "runtime-status-ipc",
        "operation": "translate_passage",
        "generation": 1,
        "priority": "foreground",
    });
    call(
        &webview,
        "study",
        serde_json::json!({ "text": "水を飲む。", "request": study.clone() }),
    )
    .expect("the stub analyzer answers");
    call(
        &webview,
        "translate",
        serde_json::json!({ "text": "水", "request": translation.clone() }),
    )
    .expect_err("the fixture has no translation model");

    let status = call(&webview, "runtime_status", serde_json::json!({})).expect("registered");
    assert_eq!(
        status["analysis"]["request"], study,
        "the analysis is filed under its request"
    );
    assert_eq!(
        status["analysis"]["outcome"],
        serde_json::json!({ "kind": "succeeded" })
    );
    assert_eq!(
        status["analysis"]["cache"], "fresh",
        "an analysis is never served from a cache"
    );
    assert_eq!(status["translation"]["request"], translation);
    assert_eq!(
        status["translation"]["outcome"],
        serde_json::json!({ "kind": "failed", "code": "engine_unavailable" })
    );
    assert_eq!(
        status["recognition"],
        serde_json::Value::Null,
        "nothing was recognized, so nothing is claimed"
    );
    let roles: Vec<serde_json::Value> = status["engines"]
        .as_array()
        .expect("engines")
        .iter()
        .map(|engine| engine["role"].clone())
        .collect();
    assert_eq!(
        roles,
        vec![
            serde_json::json!("recognition"),
            serde_json::json!("analysis"),
            serde_json::json!("translation")
        ]
    );
}

/// **The report is built from facts, not from the settings.** Private settings and an error
/// message naming a path are put where a careless report would find them; none comes out.
#[test]
fn the_diagnostic_report_carries_nothing_from_the_settings_or_an_error() {
    let scratch = Scratch::new("diagnostic-report");
    let (_app, webview) = mock_app_routed(
        Some(scratch.path()),
        routed(
            std::sync::Arc::new(Failing {
                engine: "ichiran",
                error: sakatsu_core::analyzer::AnalyzeError::EngineUnavailable(
                    r"C:\Users\someone\ichiran\ichiran-cli.exe is missing".into(),
                ),
            }),
            Some(std::sync::Arc::new(Failing {
                engine: "lindera",
                error: sakatsu_core::analyzer::AnalyzeError::EngineUnavailable(
                    "/home/someone/lindera broke".into(),
                ),
            })),
        ),
    );

    let mut settings = call(&webview, "settings", serde_json::json!({})).expect("settings");
    settings["anki_deck"] = serde_json::json!("Private::日本語 secret deck");
    settings["vlm_endpoint"] = serde_json::json!("http://192.168.1.10:8140/v1");
    settings["vlm_model"] = serde_json::json!(r"D:\models\private-model.gguf");
    settings["log"] = serde_json::json!(true);
    call(
        &webview,
        "save_settings",
        serde_json::json!({ "settings": settings }),
    )
    .expect("the private settings are saved");
    call(
        &webview,
        "study",
        serde_json::json!({
            "text": "秘密の文章です。",
            "request": {
                "owner": "diagnostic-report-ipc",
                "operation": "study",
                "generation": 1,
                "priority": "foreground",
            },
        }),
    )
    .expect_err("both analyzers fail");

    let report = call(&webview, "diagnostic_report", serde_json::json!({})).expect("registered");
    let report = report.as_str().expect("the report is text");
    for private in [
        "secret deck",
        "日本語",
        "192.168.1.10",
        "private-model",
        "someone",
        "ichiran-cli.exe",
        "秘密",
        "diagnostic-report-ipc",
        r"C:\",
        "://",
    ] {
        assert!(
            !report.contains(private),
            "the report carries {private:?}:\n{report}"
        );
    }
    assert!(
        report.contains("Logging: on"),
        "the report read the store:\n{report}"
    );
    assert!(
        report.contains("Last analysis:") && report.contains("failed (engine_failed)"),
        "the failure is reported as a code:\n{report}"
    );
}
