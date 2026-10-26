//! The recovery commands, through the real `invoke_handler`.
//!
//! What only this level can check: the command **names** and **argument names** the panel hard-codes
//! as strings, the serialized shape of the report, and that a stale action changes nothing on disk.
//!
//! Every test drives a scratch dictionary directory through `SAKATSU_DICT_DIR`, in a child process,
//! because the commands resolve that path from the environment and setting it in-process would leak
//! into every other test in this binary.

use std::path::{Path, PathBuf};

mod common;
use common::Scratch;

const CHILD: &str = "SAKATSU_RECOVERY_IPC_CHILD";

fn scratch(name: &str) -> Scratch {
    Scratch::new("recovery-ipc", name)
}

/// A dictionary archive with one word.
fn archive(path: &Path, word: &str) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, value) in [
        (
            "index.json",
            serde_json::json!({"title":"Recovery IPC fixture","format":3}),
        ),
        (
            "term_bank_1.json",
            serde_json::json!([[word, word, "n", "", 0, [word], 1, ""]]),
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(value.to_string().as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

/// Leave one dictionary container with an unresolved transaction outstanding.
fn plant_unresolved(dictionaries: &Path) {
    use sakatsu_core::dict::Dictionary;

    let work = dictionaries.parent().unwrap().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let container = dictionaries.join("fixture");

    archive(&work.join("first.zip"), "dog");
    drop(Dictionary::import(&work.join("first.zip"), &container).unwrap());

    archive(&work.join("second.zip"), "cat");
    let mut prepared = Dictionary::prepare(&work.join("second.zip"), &container).unwrap();
    prepared.activate().unwrap();
    // Dropped without settling or rolling back: the shape an install that failed after this
    // container had switched leaves behind.
    std::mem::forget(prepared);
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

fn session() -> tauri::App<tauri::test::MockRuntime> {
    use sakatsu_desktop_lib::commands::DictionaryState;
    use sakatsu_desktop_lib::live::Live;

    // The dictionaries this session is serving, discovered the way the app discovers them. Held so
    // a restoration has something to rebuild, and so a stale handle has something to stay pinned to.
    let opened = discovered();
    let app = tauri::test::mock_builder()
        .manage(DictionaryState(Live::new(opened)))
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

/// The report reaches the panel by name, with the states it branches on.
#[test]
fn the_recovery_report_crosses_ipc_with_the_state_the_panel_branches_on() {
    if let Some(dir) = std::env::var_os(CHILD) {
        let app = session();
        let report = invoke(&app, "recovery_report", serde_json::json!({})).expect("report");

        let dictionaries = report["dictionaries"].as_array().expect("an array");
        assert_eq!(dictionaries.len(), 1, "{report}");
        let entry = &dictionaries[0];
        assert_eq!(entry["name"], "fixture");
        assert_eq!(entry["state"], "unresolved", "the state the panel keys off");
        assert!(
            entry["transaction"].is_string() && entry["published"].is_string(),
            "an action needs both halves of the identity: {entry}"
        );
        assert_eq!(entry["already_tried"], serde_json::json!(false));
        let _ = dir;
        return;
    }

    let root = scratch("report");
    let dictionaries = root.join("dictionaries");
    plant_unresolved(&dictionaries);
    let output = run_child(
        "the_recovery_report_crosses_ipc_with_the_state_the_panel_branches_on",
        &dictionaries,
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// **A stale action fails and changes nothing.**
///
/// The panel is a picture of a moment. An action naming a transaction that is no longer outstanding
/// must be refused — and refused *without* touching the record, so the refresh it prompts shows the
/// same state it would have shown before the button was pressed.
#[test]
fn a_stale_action_is_refused_and_leaves_the_record_alone() {
    if let Some(dir) = std::env::var_os(CHILD) {
        let dir = PathBuf::from(dir);
        let container = dir.join("fixture");
        let app = session();

        let before = std::fs::read_to_string(dir.join(".fixture.recovery.json")).expect("a record");

        for command in ["recovery_keep_current", "recovery_restore_previous"] {
            let error = invoke(
                &app,
                command,
                serde_json::json!({
                    "container": "fixture",
                    "transaction": "a-transaction-that-was-never-outstanding",
                    "published": "g-000000000002",
                }),
            )
            .expect_err("a stale identity must be refused");
            assert_eq!(
                error["code"], "stale_recovery",
                "{command} must say the screen is out of date, not that something broke: {error}"
            );
            assert!(
                error["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("out of date"),
                "and say so in words a person can act on: {error}"
            );
        }

        assert_eq!(
            std::fs::read_to_string(dir.join(".fixture.recovery.json")).expect("still a record"),
            before,
            "a refused action must not have touched the record"
        );
        assert!(
            container.join("current").is_file(),
            "nor the container it describes"
        );
        return;
    }

    let root = scratch("stale");
    let dictionaries = root.join("dictionaries");
    plant_unresolved(&dictionaries);
    let output = run_child(
        "a_stale_action_is_refused_and_leaves_the_record_alone",
        &dictionaries,
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A correct action resolves, and the next report reflects it.
///
/// The refresh half matters as much as the action: a panel that acted successfully and went on
/// describing the old state would be as wrong as one that acted on stale information.
#[test]
fn a_matching_action_resolves_and_the_next_report_is_empty() {
    if let Some(dir) = std::env::var_os(CHILD) {
        let dir = PathBuf::from(dir);
        let app = session();
        let report = invoke(&app, "recovery_report", serde_json::json!({})).expect("report");
        let entry = &report["dictionaries"][0];

        invoke(
            &app,
            "recovery_restore_previous",
            serde_json::json!({
                "container": entry["name"],
                "transaction": entry["transaction"],
                "published": entry["published"],
            }),
        )
        .expect("the identity matches what the report showed");

        let after = invoke(&app, "recovery_report", serde_json::json!({})).expect("report");
        assert_eq!(
            after["dictionaries"].as_array().map(Vec::len),
            Some(0),
            "the panel must see it resolved on the next look: {after}"
        );
        assert!(
            !dir.join(".fixture.recovery.json").exists(),
            "and the record is gone"
        );

        // And a second attempt with the same identity is now stale rather than a silent success.
        let error = invoke(
            &app,
            "recovery_restore_previous",
            serde_json::json!({
                "container": entry["name"],
                "transaction": entry["transaction"],
                "published": entry["published"],
            }),
        )
        .expect_err("there is nothing outstanding any more");
        assert_eq!(error["code"], "stale_recovery", "{error}");
        return;
    }

    let root = scratch("resolve");
    let dictionaries = root.join("dictionaries");
    plant_unresolved(&dictionaries);
    let output = run_child(
        "a_matching_action_resolves_and_the_next_report_is_empty",
        &dictionaries,
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A restoration that cannot complete is reported as a failure, and can be retried.
#[test]
fn a_failed_restoration_is_reported_and_the_retry_succeeds() {
    if let Some(dir) = std::env::var_os(CHILD) {
        let dir = PathBuf::from(dir);
        let container = dir.join("fixture");
        let app = session();
        let report = invoke(&app, "recovery_report", serde_json::json!({})).expect("report");
        let entry = report["dictionaries"][0].clone();

        // Block the pointer write: a directory where the file has to be renamed into place.
        let pointer = container.join("current");
        std::fs::remove_file(&pointer).unwrap();
        std::fs::create_dir(&pointer).unwrap();

        let error = invoke(
            &app,
            "recovery_restore_previous",
            serde_json::json!({
                "container": entry["name"],
                "transaction": entry["transaction"],
                "published": entry["published"],
            }),
        )
        .expect_err("the pointer cannot be written");
        assert_ne!(
            error["code"], "stale_recovery",
            "a real failure must not be reported as an out-of-date screen: {error}"
        );

        // Unblock and retry with the same identity, which is still the outstanding one.
        std::fs::remove_dir(&pointer).unwrap();
        std::fs::write(&pointer, entry["published"].as_str().unwrap().as_bytes()).unwrap();
        invoke(
            &app,
            "recovery_restore_previous",
            serde_json::json!({
                "container": entry["name"],
                "transaction": entry["transaction"],
                "published": entry["published"],
            }),
        )
        .expect("the retry finishes what the failure left");
        assert!(!dir.join(".fixture.recovery.json").exists());
        return;
    }

    let root = scratch("retry");
    let dictionaries = root.join("dictionaries");
    plant_unresolved(&dictionaries);
    let output = run_child(
        "a_failed_restoration_is_reported_and_the_retry_succeeds",
        &dictionaries,
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The dictionaries as a session holds them.
fn discovered() -> std::sync::Arc<sakatsu_desktop_lib::commands::Dictionaries> {
    sakatsu_desktop_lib::discover_dictionaries(&sakatsu_desktop_lib::dictionaries_dir())
        .0
        .now()
}

/// What this session would answer for `word` if a command asked now.
fn answers(app: &tauri::App<tauri::test::MockRuntime>, word: &str) -> usize {
    use tauri::Manager;
    let open = app
        .state::<sakatsu_desktop_lib::commands::DictionaryState>()
        .0
        .now();
    open.dictionaries
        .iter()
        .filter_map(|dictionary| dictionary.lookup(word).ok())
        .map(|hits| hits.len())
        .sum()
}

/// **Restoring has to reach the session, not only the disk.**
///
/// A `Dictionary` resolves its generation once and keeps it — deliberately, so a lookup in flight
/// cannot have its bytes replaced underneath it. So a restoration that moved the pointer and
/// stopped there would leave this session serving the very version the reader had just asked to
/// undo, while the panel said the restoration had worked. That is the shape of bug the fix for the
/// kanji table was: files correct, session wrong, screen claiming success.
///
/// The session starts on the newer generation and must answer from the restored one afterwards —
/// **without a restart**. A handle taken before the restoration keeps its own generation, which is
/// the lease doing its job rather than a failure to reload.
#[test]
fn restoring_reaches_this_session_without_a_restart() {
    if let Some(dir) = std::env::var_os(CHILD) {
        let dir = PathBuf::from(dir);
        let app = session();

        // A handle somebody is already using, taken before anything is restored.
        let already_open = discovered();

        assert_eq!(
            answers(&app, "cat"),
            1,
            "the session starts on the newer generation"
        );
        assert_eq!(
            answers(&app, "dog"),
            0,
            "which does not have the older word"
        );

        let report = invoke(&app, "recovery_report", serde_json::json!({})).expect("report");
        let entry = &report["dictionaries"][0];
        invoke(
            &app,
            "recovery_restore_previous",
            serde_json::json!({
                "container": entry["name"],
                "transaction": entry["transaction"],
                "published": entry["published"],
            }),
        )
        .expect("the identity matches what the report showed");

        assert_eq!(
            answers(&app, "dog"),
            1,
            "a lookup in this session must find the restored contents"
        );
        assert_eq!(
            answers(&app, "cat"),
            0,
            "and stop finding the version that was undone"
        );

        // The pin, which is a guarantee rather than an oversight: this handle was resolved before
        // the pointer moved and goes on answering from the generation it opened.
        let held: usize = already_open
            .dictionaries
            .iter()
            .filter_map(|dictionary| dictionary.lookup("cat").ok())
            .map(|hits| hits.len())
            .sum();
        assert_eq!(
            held, 1,
            "a reader already holding a generation keeps it; what changes is what the next lookup \
             opens"
        );

        assert!(
            !dir.join(".fixture.recovery.json").exists(),
            "and the record is gone"
        );
        return;
    }

    let root = scratch("session");
    let dictionaries = root.join("dictionaries");
    plant_unresolved(&dictionaries);
    let output = run_child(
        "restoring_reaches_this_session_without_a_restart",
        &dictionaries,
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
