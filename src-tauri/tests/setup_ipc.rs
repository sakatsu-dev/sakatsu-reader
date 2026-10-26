//! The setup commands, through the real `invoke_handler`, and the shape of what they send.
//!
//! # What only this level can check
//!
//! Generation proves the TypeScript declaration matches the Rust type. It cannot prove that the
//! command is **registered**, that its **argument names** are what the frontend passes, or that the
//! bytes serde emits are the bytes the golden says. Every setup command was outside `ipc.rs`
//! because its harness holds no `SetupState`, so none of that was covered for the largest surface
//! in the app.
//!
//! # It registers what production registers
//!
//! The handler comes from `sakatsu_desktop_lib::reader_commands!()`, the same expansion `run()`
//! uses. A harness with its own `generate_handler!` list proves that a command binds its arguments
//! and behaves correctly **and nothing about whether the app registers it**: remove it from
//! production and these stay green while the command becomes unreachable.
//! `a_command_missing_from_production_is_missing_from_the_harness` demonstrates that the sharing
//! actually bites.
//!
//! # Why a child process
//!
//! `setup_snapshot` resolves the resource directories from the environment and reports what is
//! installed. Setting those variables in-process would leak into every other test in this binary,
//! so the parent prepares an isolated profile and the child runs the session inside it — the idiom
//! `recovery_ipc.rs` and `resource_isolation.rs` already use here.
//!
//! The golden is built in the parent, because serializing a value needs no application at all.

use std::path::{Path, PathBuf};

use sakatsu_core::setup::transport::{
    FetchProgress, SetupArtifactState, SetupComponentState, SetupPhase, SetupPreflight,
    SetupProgress, SetupSnapshot, StorageSummary,
};

mod common;

const CHILD: &str = "SAKATSU_SETUP_IPC_CHILD";

fn scratch(name: &str) -> common::Scratch {
    common::Scratch::new("setup-ipc", name)
}

fn run_child(test: &str, profile: &Path) -> std::process::Output {
    let device = profile.join("Local").join("com.sakatsu.app");
    run_child_at(test, profile, &device)
}

fn run_child_at(test: &str, profile: &Path, device: &Path) -> std::process::Output {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let local = profile.join("Local");
    std::process::Command::new(&exe)
        .args([test, "--exact", "--nocapture"])
        .env(CHILD, profile)
        .env("APPDATA", profile.join("Roaming"))
        .env("LOCALAPPDATA", &local)
        .env("SAKATSU_DEVICE_DATA_DIR", device)
        // Cleared, so a stale override in whoever runs the suite cannot point this at real
        // resources and make it pass for the wrong reason.
        .env_remove("SAKATSU_KANJI_DIR")
        .env_remove("SAKATSU_DICT_DIR")
        .output()
        .expect("spawn a fresh session")
}

fn expect_success(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// An explicit analyzer choice survives a fresh session, through the registered command.
#[test]
fn analyzer_choice_is_persisted_by_the_production_command() {
    const NAME: &str = "analyzer_choice_is_persisted_by_the_production_command";
    if let Some(profile) = std::env::var_os(CHILD) {
        let state_path = PathBuf::from(profile).join("Roaming/com.sakatsu.app/setup-state.json");
        std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        std::fs::write(
            &state_path,
            br#"{"consented":true,"analyzer_setup_pending":true}"#,
        )
        .unwrap();
        let app = session();
        let before = invoke(&app, "setup_snapshot", serde_json::json!({})).unwrap();
        assert_eq!(before["analyzer_setup_pending"], true);
        invoke(&app, "setup_finish_analyzer", serde_json::json!({})).unwrap();
        drop(app);
        let reopened = session();
        let after = invoke(&reopened, "setup_snapshot", serde_json::json!({})).unwrap();
        assert_eq!(after["analyzer_setup_pending"], false);
        return;
    }
    let profile = scratch("analyzer-choice");
    let output = run_child(NAME, &profile.0);
    expect_success(&output, NAME);
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

/// A session with the real setup commands registered.
fn session() -> tauri::App<tauri::test::MockRuntime> {
    let app = tauri::test::mock_builder()
        .manage(sakatsu_desktop_lib::commands::AnalyzerState::fixed(
            sakatsu_desktop_lib::built_in_analyzer().0,
        ))
        .manage(sakatsu_desktop_lib::setup_manager::SetupState::load())
        // **Production's own registration.** Not a list of the setup commands: the whole list, so
        // that removing any of them from `run()` removes it from here too.
        .invoke_handler(sakatsu_desktop_lib::reader_commands!())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build a mock session");
    tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build a mock webview");
    app
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

/// The snapshot the panel reads arrives under the names it reads.
#[test]
fn the_setup_snapshot_crosses_ipc_with_the_fields_the_panel_reads() {
    if std::env::var_os(CHILD).is_some() {
        let app = session();
        let snapshot = invoke(&app, "setup_snapshot", serde_json::json!({})).expect("snapshot");

        for field in [
            "phase",
            "generation",
            "required_ready",
            "resumable",
            "text_only",
            "welcome_seen",
            "existing_install",
            "storage",
            "components",
        ] {
            assert!(
                snapshot.get(field).is_some(),
                "the panel reads {field}: {snapshot}"
            );
        }
        assert!(
            snapshot["phase"].is_string(),
            "the phase is a literal the store switches on, not an object: {snapshot}"
        );
        for field in [
            "target",
            "download_bytes",
            "available_bytes",
            "reserve_bytes",
        ] {
            assert!(
                snapshot["storage"].get(field).is_some(),
                "storage.{field} is missing: {snapshot}"
            );
        }
        let components = snapshot["components"]
            .as_array()
            .expect("components is a list");
        assert!(
            !components.is_empty(),
            "a fresh profile still offers every essential component: {snapshot}"
        );
        assert!(
            components
                .iter()
                .all(|component| component["artifacts"].is_array()),
            "each component carries its artifacts: {snapshot}"
        );

        let preflight = invoke(&app, "setup_preflight", serde_json::json!({})).expect("preflight");
        for field in [
            "target_location",
            "writable",
            "required_bytes",
            "enough_space",
            "supported_windows",
            "webview2",
        ] {
            assert!(
                preflight.get(field).is_some(),
                "preflight.{field} is missing: {preflight}"
            );
        }
        return;
    }

    let profile = scratch("snapshot");
    let output = run_child(
        "the_setup_snapshot_crosses_ipc_with_the_fields_the_panel_reads",
        &profile,
    );
    expect_success(&output, "the snapshot did not cross as the panel reads it");
}

/// **The argument names, which nothing else checks.**
///
/// `invoke("setup_choose_storage", { path })` compiles against a backend that renamed the
/// parameter. A rename fails here and nowhere else — the generated declarations describe types, not
/// call sites.
#[test]
fn the_setup_commands_take_the_argument_names_the_frontend_passes() {
    if let Some(profile) = std::env::var_os(CHILD) {
        let profile = PathBuf::from(profile);
        let app = session();

        let chosen = profile.join("chosen");
        std::fs::create_dir_all(&chosen).unwrap();
        invoke(
            &app,
            "setup_choose_storage",
            serde_json::json!({ "path": chosen.display().to_string() }),
        )
        .expect("setup_choose_storage takes `path`");

        // A wrong argument name must fail rather than being ignored, or the check above proves
        // only that the command exists.
        invoke(
            &app,
            "setup_choose_storage",
            serde_json::json!({ "directory": chosen.display().to_string() }),
        )
        .expect_err("an unknown argument name is refused");

        let refused = invoke(
            &app,
            "setup_restart_artifact",
            serde_json::json!({ "componentId": "a-component-that-does-not-exist" }),
        )
        .expect_err("the id is unknown, and that is the error we want to see");
        assert!(
            refused
                .as_str()
                .unwrap_or_default()
                .contains("Unknown setup component"),
            "the command was reached with its argument bound: {refused}"
        );

        // `setup_move_resources` takes two, and the second is a bool rather than a string. Pointed
        // at a directory that does not exist, so the answer comes from the move itself — which is
        // what says both arguments bound and the body ran.
        let moved = invoke(
            &app,
            "setup_move_resources",
            serde_json::json!({
                "target": profile.join("elsewhere").display().to_string(),
                "removeOld": false,
            }),
        )
        .expect("both arguments bind and the move runs");
        // Nothing is installed in this profile, so the honest answer is that it verified nothing —
        // and it names the target it was given, which is what says the argument reached the body.
        let message = moved.as_str().unwrap_or_default().to_string();
        assert!(
            message.contains("0 resource folders") && message.contains("elsewhere"),
            "the move ran against the target it was handed: {message}"
        );

        // And the second argument is genuinely required: omitting it must fail at the boundary
        // rather than defaulting to something nobody asked for.
        invoke(
            &app,
            "setup_move_resources",
            serde_json::json!({ "target": profile.join("elsewhere").display().to_string() }),
        )
        .expect_err("removeOld has no default; deleting or not deleting is not a guess");
        return;
    }

    let profile = scratch("arguments");
    let output = run_child(
        "the_setup_commands_take_the_argument_names_the_frontend_passes",
        &profile,
    );
    expect_success(&output, "an argument name did not bind");
}

/// A move asked for while a component is installing is refused **through the command**.
///
/// `core/tests/resource_coordination.rs` proves the claim refuses it; this proves the refusal
/// survives the boundary as a message somebody can read, rather than as a panic or an empty error.
#[test]
fn a_move_asked_for_during_an_install_is_refused_across_the_boundary() {
    if let Some(profile) = std::env::var_os(CHILD) {
        let profile = PathBuf::from(profile);
        let app = session();
        let target = profile.join("elsewhere");
        std::fs::create_dir_all(&target).unwrap();

        let _installing =
            sakatsu_core::setup::install::reserve_component_installs(["dictionaries"])
                .expect("nothing else is installing in a fresh session");
        let refused = invoke(
            &app,
            "setup_move_resources",
            serde_json::json!({ "target": target.display().to_string(), "removeOld": true }),
        )
        .expect_err("a move during an install must be refused");
        let message = refused.as_str().unwrap_or_default();
        assert!(
            message.contains("being installed"),
            "and it must say what is running: {refused}"
        );
        return;
    }

    let profile = scratch("busy");
    let output = run_child(
        "a_move_asked_for_during_an_install_is_refused_across_the_boundary",
        &profile,
    );
    expect_success(&output, "the refusal did not survive the boundary");
}

/// The serialized setup surface, pinned.
///
/// # What the fixtures are chosen for
///
/// A golden is only as good as the values in it, so these reach the shapes a natural snapshot would
/// not: **both states of every `Option`** — `current_component` and `current_issue` present and
/// absent, `available_bytes` known and unknown, `partial_file` and `transfer_job` both ways —
/// **an empty collection and a populated one** (a component with no artifacts beside one with two),
/// **every `Tier`**, and a `SetupPhase` that is not the first variant. The progress events appear in
/// all three of the states the shell emits: starting, mid-transfer with a speed and an estimate, and
/// working with nothing measurable.
///
/// Regenerate with `UPDATE_GOLDEN=1`, exactly as for `model.golden.json`.
#[test]
fn the_serialized_setup_surface_matches_the_golden_the_ui_checks() {
    use sakatsu_core::setup::Tier;

    let snapshot = SetupSnapshot {
        analyzer_setup_pending: false,
        phase: SetupPhase::PausedRetrying,
        generation: 7,
        required_ready: false,
        resumable: true,
        text_only: false,
        welcome_seen: true,
        existing_install: true,
        current_component: Some("dictionaries".into()),
        current_issue: None,
        storage: StorageSummary {
            target: "C:\\\\profile\\\\resources".into(),
            download_bytes: 1_234_567,
            installed_bytes: 2_345_678,
            staging_bytes: 0,
            peak_bytes: 3_456_789,
            available_bytes: None,
            reserve_bytes: 512,
        },
        components: vec![
            SetupComponentState {
                id: "dictionaries".into(),
                label: "Dictionaries".into(),
                required: true,
                installed: false,
                installed_version: None,
                transfer_state: "downloading".into(),
                downloaded_bytes: 100,
                total_bytes: 400,
                verification: "pending".into(),
                repair_eligible: false,
                artifacts: vec![
                    SetupArtifactState {
                        file: "jitendex.zip".into(),
                        version: "2026-07-09".into(),
                        immutable_source: "https://example.invalid/jitendex.zip".into(),
                        required: true,
                        expected_download_bytes: 200,
                        expected_installed_bytes: 900,
                        sha256: "0".repeat(64),
                        downloaded_bytes: 100,
                        partial_file: Some("jitendex.zip.part".into()),
                        transfer_job: Some("{4242-4242}".into()),
                        verification: "pending".into(),
                    },
                    SetupArtifactState {
                        file: "jmnedict.zip".into(),
                        version: "2026-07-29".into(),
                        immutable_source: "https://example.invalid/jmnedict.zip".into(),
                        required: false,
                        expected_download_bytes: 200,
                        expected_installed_bytes: 800,
                        sha256: "1".repeat(64),
                        downloaded_bytes: 0,
                        partial_file: None,
                        transfer_job: None,
                        verification: "verified".into(),
                    },
                ],
            },
            // No artifacts, and installed: the empty collection and the other side of every flag.
            SetupComponentState {
                id: "ichiran".into(),
                label: "Ichiran".into(),
                required: false,
                installed: true,
                installed_version: Some("2026-01-01".into()),
                transfer_state: "idle".into(),
                downloaded_bytes: 0,
                total_bytes: 0,
                verification: "verified".into(),
                repair_eligible: true,
                artifacts: Vec::new(),
            },
        ],
    };

    let preflight = SetupPreflight {
        target_location: "C:\\\\profile\\\\resources".into(),
        writable: true,
        required_bytes: 4_000_000,
        available_bytes: Some(9_000_000),
        peak_bytes: 5_000_000,
        enough_space: true,
        network_available: false,
        supported_windows: true,
        supported_architecture: true,
        webview2: true,
        blocking_issue: Some("No network connection.".into()),
    };

    let starting = FetchProgress::starting("dictionaries", "jitendex.zip", 1, 2, Some(200));
    let during = FetchProgress::during(
        "dictionaries",
        "jitendex.zip",
        1,
        2,
        &sakatsu_core::setup::Progress {
            done: 100,
            total: Some(200),
            speed_bytes_per_second: Some(1024),
            eta_seconds: Some(3),
            attempt: 2,
        },
    );
    let working = FetchProgress::working("dictionaries", "importing…");

    // Every tier, so a renamed variant fails here as well as in the generated union.
    let tiers = [Tier::Essential, Tier::Optional, Tier::Advanced];
    // Every phase, for the same reason: a snapshot only ever carries one at a time.
    let phases = [
        SetupPhase::Checking,
        SetupPhase::ReadyToInstall,
        SetupPhase::Downloading,
        SetupPhase::PausedRetrying,
        SetupPhase::Installing,
        SetupPhase::Verifying,
        SetupPhase::Complete,
        SetupPhase::NeedsAttention,
    ];

    let whole = serde_json::json!({
        "snapshot": snapshot,
        "preflight": preflight,
        "setup_progress": SetupProgress::of(7, &during),
        "fetch_progress_starting": starting,
        "fetch_progress_during": during,
        "fetch_progress_working": working,
        "tiers": tiers,
        "phases": phases,
    });

    let generated = serde_json::to_string_pretty(&whole).expect("serialize") + "\n";
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/setup.golden.json");
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&golden, &generated).expect("write golden");
    }
    assert_eq!(
        std::fs::read_to_string(&golden)
            .expect("setup golden; UPDATE_GOLDEN=1 explicitly regenerates it")
            .replace("\r\n", "\n"),
        generated,
        "\nui/src/setup.golden.json no longer matches what serde emits. Regenerate it \
         deliberately:\n  UPDATE_GOLDEN=1 cargo test -p sakatsu-desktop --test setup_ipc\n  \
         npm --prefix ui run check\n"
    );
}

/// **The sharing bites.** Every command this file exercises is registered by production.
///
/// A harness that keeps its own list can go on passing after a command is dropped from `run()`.
/// This asserts the tie the other way round, which is the direction that can be checked from a
/// test: each command name reaches a handler, and the one name that is *not* a command does not.
/// Remove `setup_snapshot` from `reader_commands!` and this fails with that name.
#[test]
fn a_command_missing_from_production_is_missing_from_the_harness() {
    if std::env::var_os(CHILD).is_some() {
        let app = session();
        for command in [
            "setup_snapshot",
            "setup_preflight",
            "setup_repair",
            "setup_use_text_only",
            "setup_finish_analyzer",
            "setup_choose_storage",
            "setup_move_resources",
            "setup_restart_artifact",
            "setup_start",
            "setup_pause",
            "setup_import_offline",
            "build_gloss_index",
            "import_dictionary",
            "fetch_component",
            "recovery_keep_current",
            "recovery_restore_previous",
            // Called with no `destination`, so it fails on the argument and never reaches the
            // browser launcher; the refusals themselves are pinned in `ipc.rs`.
            "open_creator_link",
        ] {
            let answer = invoke(&app, command, serde_json::json!({}));
            let unregistered = matches!(&answer, Err(error)
                if error.as_str().unwrap_or_default().contains("not found"));
            assert!(
                !unregistered,
                "{command} is not registered by production's handler: {answer:?}"
            );
        }

        // The control: a name nothing registers must be reported exactly that way, or the check
        // above is satisfied by every string.
        let missing = invoke(
            &app,
            "a_command_that_was_never_written",
            serde_json::json!({}),
        )
        .expect_err("an unregistered command is refused");
        assert!(
            missing.as_str().unwrap_or_default().contains("not found"),
            "and this is the wording the check above relies on: {missing}"
        );
        return;
    }

    let profile = scratch("registration");
    let output = run_child(
        "a_command_missing_from_production_is_missing_from_the_harness",
        &profile,
    );
    expect_success(&output, "a command the tests exercise is not registered");
}

/// `setup_start` and `setup_pause`, on a path where nothing **can** download.
///
/// **The refusal path, made deterministic rather than hoped for.** An earlier version of this let
/// preflight decide, and on a machine with a network that decision is "go ahead" — the test would
/// have started a real download. So the child's storage location is a directory that cannot be
/// created, because a file sits where it would go: `ensure_writable` fails, preflight reports a
/// blocking issue, and `setup_start` stops at `NeedsAttention` before any transfer begins.
///
/// What is asserted is that both commands are registered, that they answer, and that the state
/// they leave is the one the panel draws.
#[test]
fn setup_start_and_pause_answer_without_starting_a_download() {
    if std::env::var_os(CHILD).is_some() {
        let app = session();

        let before = invoke(&app, "setup_snapshot", serde_json::json!({})).expect("snapshot");
        assert_eq!(
            before["phase"],
            serde_json::json!("ready_to_install"),
            "a fresh profile starts here, so the phase below is something that happened: {}",
            before["phase"]
        );

        let preflight = invoke(&app, "setup_preflight", serde_json::json!({})).expect("preflight");
        assert_eq!(
            preflight["writable"],
            serde_json::json!(false),
            "the fixture has to make the location unwritable, or this test can download: {}",
            preflight["target_location"]
        );

        let started = invoke(&app, "setup_start", serde_json::json!({})).expect("setup_start");
        assert!(
            started.is_boolean(),
            "it answers whether it took the work on: {started}"
        );

        let after = invoke(&app, "setup_snapshot", serde_json::json!({})).expect("snapshot");
        assert_eq!(
            after["phase"],
            serde_json::json!("needs_attention"),
            "it stopped at the blocking issue rather than transferring: {}",
            after["phase"]
        );
        assert!(
            after["current_issue"]
                .as_str()
                .unwrap_or_default()
                .contains("not writable"),
            "and said which one: {}",
            after["current_issue"]
        );

        invoke(&app, "setup_pause", serde_json::json!({})).expect("setup_pause");
        let paused = invoke(&app, "setup_snapshot", serde_json::json!({})).expect("snapshot");
        assert_eq!(
            paused["phase"],
            serde_json::json!("paused_retrying"),
            "pausing says so, whatever it was doing: {}",
            paused["phase"]
        );
        assert!(
            paused["current_issue"].is_string(),
            "and gives the reader a sentence: {}",
            paused["current_issue"]
        );
        return;
    }

    let profile = scratch("start-pause");
    // A file where the storage directory would be, so creating it cannot succeed.
    let blocked = profile.join("blocked");
    std::fs::write(&blocked, b"not a directory").expect("plant the obstruction");
    let output = run_child_at(
        "setup_start_and_pause_answer_without_starting_a_download",
        &profile,
        &blocked.join("com.sakatsu.app"),
    );
    expect_success(&output, "setup_start or setup_pause did not answer");
}

/// **A closed store gate defers setup's write but keeps the change.** An update being prepared
/// closes the gate while it waits for setup; a change made in that window has to survive it in
/// memory and reach disk with the next write.
#[test]
fn a_setup_change_made_while_the_store_is_closed_is_kept_and_written_later() {
    const NAME: &str = "a_setup_change_made_while_the_store_is_closed_is_kept_and_written_later";
    if let Some(profile) = std::env::var_os(CHILD) {
        use sakatsu_desktop_lib::lifecycle::{gates, Gate};
        let state_path = PathBuf::from(profile).join("Roaming/com.sakatsu.app/setup-state.json");
        std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        std::fs::write(&state_path, r#"{"consented":true,"phase":"complete"}"#).unwrap();
        let on_disk = || -> serde_json::Value {
            serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap()
        };
        let app = session();

        let closed = gates()
            .exclusive(&[Gate::Store], std::time::Duration::ZERO)
            .expect("nothing else writes in this child");
        invoke(&app, "setup_use_text_only", serde_json::json!({})).expect("text only");
        let snapshot = invoke(&app, "setup_snapshot", serde_json::json!({})).expect("snapshot");
        assert_eq!(
            snapshot["text_only"],
            serde_json::json!(true),
            "the change was dropped from memory"
        );
        assert_ne!(
            on_disk()["text_only"],
            serde_json::json!(true),
            "the closed gate let a write through"
        );
        drop(closed);

        invoke(&app, "setup_pause", serde_json::json!({})).expect("setup_pause");
        assert_eq!(
            on_disk()["text_only"],
            serde_json::json!(true),
            "the next write did not carry the deferred change"
        );
        return;
    }

    let profile = scratch("store-closed");
    let output = run_child(NAME, &profile.0);
    expect_success(&output, NAME);
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

/// **Discarding an optional download's partial leaves required setup alone.** Starting a
/// required component over reopens setup; setting a pronunciation pack aside must not make a
/// finished install report itself incomplete.
#[test]
fn discarding_an_optional_partial_does_not_reopen_setup() {
    if let Some(profile) = std::env::var_os(CHILD) {
        let state_path = PathBuf::from(profile).join("Roaming/com.sakatsu.app/setup-state.json");
        std::fs::create_dir_all(state_path.parent().unwrap()).unwrap();
        std::fs::write(&state_path, r#"{"consented":true,"phase":"complete"}"#).unwrap();
        let app = session();
        let phase = |app: &_| {
            invoke(app, "setup_snapshot", serde_json::json!({})).expect("snapshot")["phase"].clone()
        };
        let before = phase(&app);
        assert_ne!(
            before,
            serde_json::json!("ready_to_install"),
            "the check needs a phase the reset would visibly change"
        );

        invoke(
            &app,
            "setup_restart_artifact",
            serde_json::json!({ "componentId": "pronunciation" }),
        )
        .expect("an optional component can be started over");
        assert_eq!(phase(&app), before, "an optional partial reopened setup");

        invoke(
            &app,
            "setup_restart_artifact",
            serde_json::json!({ "componentId": "manga-ocr" }),
        )
        .expect("a required component can be started over");
        assert_eq!(phase(&app), serde_json::json!("ready_to_install"));
        return;
    }

    let profile = scratch("optional-partial");
    let output = run_child(
        "discarding_an_optional_partial_does_not_reopen_setup",
        &profile,
    );
    expect_success(&output, "discarding an optional partial");
}
