//! The index upgrade, through the handler a reader's click actually reaches.
//!
//! # What only this level can check
//!
//! `core/tests/cross_reference_upgrade.rs` proved the repair works on the installed data by
//! calling `Dictionary::index_secondary` — the indexing primitive. A test that then opens its own
//! fresh handles proves nothing about the command: deleting the reader replacement, or the cache
//! invalidation, would leave it passing. What a reader invokes is the command, and every step
//! between it and the primitive is a way the repair could be built and still not arrive.
//!
//! So this drives the **production handler** — `reader_commands!()`, the same registration `run()`
//! uses — waits for the command's **own completion event**, and then asks the **same session**,
//! whose `DictionaryState` the command was responsible for replacing.
//!
//! # The state being reproduced
//!
//! A dictionary whose English search already works and whose sequence bucket is missing. Not a
//! contrivance: it is what was installed on the machine this was found on, and it is precisely the
//! state in which the old interface said nothing, because the only prompt was gated on
//! `needs_gloss_index` and glosses were present.
//!
//! # Isolation
//!
//! In a child process, with `SAKATSU_DICT_DIR` pointing at a scratch directory — the commands
//! resolve that from the environment, and setting it in-process would leak into every other test
//! in this binary. The reader's own store is never opened, claimed or republished. `Scratch`
//! removes the directory on drop, so a panic cleans up too.

use std::path::Path;

mod common;
use common::Scratch;

const CHILD: &str = "SAKATSU_UPKEEP_IPC_CHILD";

/// A Yomitan term row: `[term, reading, tags, rules, score, glosses, sequence, ""]`.
fn row(term: &str, reading: &str, glosses: &[&str], sequence: i64) -> serde_json::Value {
    serde_json::json!([term, reading, "n", "", 0, glosses, sequence, ""])
}

/// Import a dictionary, then strip the sequence bucket so it matches an index built before that
/// bucket existed. The gloss bucket is left alone — English search must still work, which is the
/// whole point of the case.
fn plant_old_format(installed: &Path) {
    use std::io::Write as _;
    let archive = installed.with_extension("zip");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).expect("fixture"));
    for (member, contents) in [
        (
            "index.json",
            serde_json::json!({ "title": "Upkeep Fixture", "format": 3 }).to_string(),
        ),
        (
            "term_bank_1.json",
            serde_json::Value::Array(vec![
                row(
                    "抜きとる",
                    "ぬきとる",
                    &["⟶ 抜 ぬ き 取 と る", "抜き取る redirected from 抜きとる"],
                    -1478150,
                ),
                row(
                    "抜き取る",
                    "ぬきとる",
                    &["to pull out; to extract", "to pick out"],
                    1478150,
                ),
            ])
            .to_string(),
        ),
    ] {
        writer
            .start_file(member, zip::write::SimpleFileOptions::default())
            .expect("member");
        writer.write_all(contents.as_bytes()).expect("write");
    }
    writer.finish().expect("finish");

    std::fs::create_dir_all(installed).expect("install dir");
    let out = installed.join("upkeep");
    sakatsu_core::dict::index::Dictionary::import(&archive, &out).expect("import");
    strip_sequence(&out);
}

fn strip_sequence(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|name| name == "sequence") {
            std::fs::remove_dir_all(&path).expect("strip the sequence bucket");
        } else {
            strip_sequence(&path);
        }
    }
}

fn run_child(test: &str, dictionaries: &Path) -> std::process::Output {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    std::process::Command::new(&exe)
        .args([test, "--exact", "--nocapture"])
        .env(CHILD, dictionaries)
        .env("SAKATSU_DICT_DIR", dictionaries)
        .output()
        .expect("spawn a fresh session")
}

fn invoke(
    app: &tauri::App<tauri::test::MockRuntime>,
    cmd: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    use tauri::Manager;
    let webview = app
        .get_webview_window("main")
        .expect("the mock window exists");
    tauri::test::get_ipc_response(
        &webview,
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        },
    )
    .map(|value| {
        value
            .deserialize::<serde_json::Value>()
            .expect("response deserializes")
    })
}

/// A session built the way the application builds one, serving whatever is installed.
fn session() -> tauri::App<tauri::test::MockRuntime> {
    use sakatsu_desktop_lib::commands::DictionaryState;
    use sakatsu_desktop_lib::live::Live;

    let opened =
        sakatsu_desktop_lib::discover_dictionaries(&sakatsu_desktop_lib::dictionaries_dir());
    let app = tauri::test::mock_builder()
        .manage(DictionaryState(Live::new(opened.0.now())))
        .manage(sakatsu_desktop_lib::warmup::WarmupState::default())
        // Production's own registration, so a command removed from `run()` is removed from here.
        .invoke_handler(sakatsu_desktop_lib::reader_commands!())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build a mock session");
    tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build a mock webview");
    app
}

fn token(surface: &str) -> serde_json::Value {
    serde_json::json!({
        "surface": surface,
        "dict_form": surface,
        "reading": "",
        "lemma_reading": "",
        "start": 0,
        "end": surface.len(),
        "senses": [],
        "components": [],
    })
}

fn senses_of(view: &serde_json::Value) -> Vec<String> {
    view["inline"]
        .as_array()
        .map(|senses| {
            senses
                .iter()
                .flat_map(|sense| {
                    sense["glosses"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|gloss| gloss.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_pointer(gloss: &str) -> bool {
    gloss.contains(" redirected from ") || gloss.trim_start().starts_with('\u{27f6}')
}

/// **The whole path: status, lookup, upgrade, event, and the same session answering again.**
#[test]
fn the_upgrade_reaches_the_session_that_asked_for_it() {
    if std::env::var_os(CHILD).is_some() {
        use tauri::{Listener, Manager};
        let app = session();

        // --- what the panel is told -------------------------------------------------------
        let upkeep = invoke(&app, "dictionary_upkeep", serde_json::json!({})).expect("upkeep");
        let entries = upkeep.as_array().expect("an array");
        assert_eq!(entries.len(), 1, "{upkeep}");
        assert_eq!(
            entries[0]["has_glosses"],
            serde_json::json!(true),
            "English search already works"
        );
        assert_eq!(
            entries[0]["has_sequence"],
            serde_json::json!(false),
            "the bucket is absent"
        );
        assert_eq!(
            entries[0]["needs_indexes"],
            serde_json::json!(true),
            "so a rebuild is offered"
        );
        assert!(
            entries[0]["id"].as_str().is_some_and(|id| !id.is_empty()),
            "the panel keys on an identity, not a title: {upkeep}"
        );

        // --- what the reader sees, through the command the panel uses ----------------------
        let before = invoke(
            &app,
            "lookup",
            serde_json::json!({ "token": token("抜きとる") }),
        )
        .expect("lookup before");
        let before = senses_of(&before);
        assert!(
            before.iter().any(|gloss| is_pointer(gloss)),
            "a pointer is all there is before the upgrade: {before:?}"
        );

        // --- and through the cached path the reader's selection actually takes -------------
        //
        // `lookup` reads `DictionaryState` and nothing else. `lookup_in_context` goes through
        // `WarmupState`, keyed by passage digest and index — so an upgrade that replaced the
        // readers but left the cache alone would answer this from the pointer it stored before.
        // Asked twice, so the second is demonstrably a *hit* and the cache is known to be live;
        // otherwise a later miss would prove nothing about invalidation.
        let contextual = |app: &tauri::App<tauri::test::MockRuntime>| {
            invoke(
                app,
                "lookup_in_context",
                serde_json::json!({ "tokens": [token("抜きとる")], "index": 0 }),
            )
            .expect("contextual lookup")
        };
        let cold = contextual(&app);
        let warm = contextual(&app);
        assert_eq!(cold, warm, "the same passage answers the same way");
        let hits_before = {
            let warmup = app.state::<sakatsu_desktop_lib::warmup::WarmupState>();
            let seen = warmup.diagnostics();
            assert!(
                seen.analysis_hits >= 1,
                "the second request must be served from the cache, or invalidation is untested:                  {seen:?}"
            );
            seen
        };
        let before_contextual = senses_of(&cold["word"]);
        assert!(
            before_contextual.iter().any(|gloss| is_pointer(gloss)),
            "the cached answer is the pointer: {before_contextual:?}"
        );

        // --- the upgrade, through the command, awaiting its own event ----------------------
        let (tx, rx) = std::sync::mpsc::channel::<serde_json::Value>();
        app.listen("gloss-index-built", move |event| {
            let _ = tx.send(serde_json::from_str(event.payload()).unwrap_or_default());
        });
        invoke(&app, "build_gloss_index", serde_json::json!({})).expect("build");
        let payload = rx
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("the command reports by event");
        assert_eq!(payload["ok"], serde_json::json!(true), "{payload}");

        // --- the same session, without rebuilding anything here ---------------------------
        //
        // This is the step the old test could not make. `lookup` reads `DictionaryState`, and the
        // command is what had to replace it: a handle kept across the publication stays on its own
        // generation, so a command that published an index and left the session's readers alone
        // would pass every check above and still show the reader an arrow.
        let after = invoke(
            &app,
            "lookup",
            serde_json::json!({ "token": token("抜きとる") }),
        )
        .expect("lookup after");
        let after = senses_of(&after);
        assert!(
            after.iter().any(|gloss| gloss.contains("to pull out")),
            "the session that asked now has the definition: {after:?}"
        );
        assert!(
            !after.iter().any(|gloss| is_pointer(gloss)),
            "and no pointer survives: {after:?}"
        );

        let upkeep = invoke(&app, "dictionary_upkeep", serde_json::json!({})).expect("upkeep");
        assert_eq!(
            upkeep[0]["needs_indexes"],
            serde_json::json!(false),
            "and the offer stops being made: {upkeep}"
        );

        // --- the cached path too, which is the one a selection takes -----------------------
        //
        // **A separate failure from the reader replacement.** Leaving the cache alone would let
        // every assertion above pass and still answer the reader from the pointer it stored.
        let after_upgrade = contextual(&app);
        let after_contextual = senses_of(&after_upgrade["word"]);
        assert!(
            after_contextual
                .iter()
                .any(|gloss| gloss.contains("to pull out")),
            "the contextual answer is recomputed after the upgrade: {after_contextual:?}"
        );
        assert!(
            !after_contextual.iter().any(|gloss| is_pointer(gloss)),
            "and no pointer survives it: {after_contextual:?}"
        );
        let seen = app
            .state::<sakatsu_desktop_lib::warmup::WarmupState>()
            .diagnostics();
        assert!(
            seen.invalidations >= 1,
            "the command must drop the cached answers: {seen:?}"
        );
        assert!(
            seen.analysis_misses > hits_before.analysis_misses,
            "and the request after it must miss rather than be served the stale answer: {seen:?}"
        );
        let _ = app.webview_windows();
        return;
    }

    let root = Scratch::new("upkeep-ipc", "session");
    let installed = root.join("dictionaries");
    plant_old_format(&installed);
    let output = run_child(
        "the_upgrade_reaches_the_session_that_asked_for_it",
        &installed,
    );
    // The child's own report, always — a green parent that spawned a child which quietly ran no
    // assertions is exactly the failure this whole file exists to avoid.
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("{stdout}");
    assert!(
        stdout.contains("1 passed"),
        "the child ran no assertions: {stdout}
{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// **A handle held across the publication keeps its own generation.**
///
/// The control for the test above: it is *because* of this that the command must replace the
/// session's readers, and it is what the command's opening comment used to deny. Pinned so a
/// change that drops the rebuild fails here rather than quietly delivering nothing.
#[test]
fn a_handle_held_across_the_upgrade_keeps_its_own_generation() {
    use sakatsu_core::dict::index::Dictionary;

    let root = Scratch::new("upkeep-ipc", "generation");
    let installed = root.join("dictionaries");
    plant_old_format(&installed);

    let held: Vec<Dictionary> = Dictionary::discover(&installed)
        .iter()
        .map(|path| Dictionary::open(path).expect("open"))
        .collect();
    assert!(
        !held[0].has_sequence(),
        "the bucket is absent to begin with"
    );

    let owned =
        sakatsu_core::setup::claim::claim([sakatsu_core::setup::claim::Claim::Dictionaries])
            .expect("claim");
    let outcome = sakatsu_core::setup::backfill::rebuild_gloss_indexes(&installed, &owned);
    assert!(outcome.failed.is_none(), "{:?}", outcome.failed);
    drop(owned);

    assert!(
        !held[0].has_sequence(),
        "the handle from before the publication still points at its own generation"
    );
    let reopened: Vec<Dictionary> = Dictionary::discover(&installed)
        .iter()
        .map(|path| Dictionary::open(path).expect("open"))
        .collect();
    assert!(
        reopened[0].has_sequence(),
        "and a freshly resolved handle does see it"
    );
}
