//! Every production command that writes resources owns them first — asked of the commands.
//!
//! # Why this file exists beside the core coordination tests
//!
//! `core/tests/resource_coordination.rs` asks what the claim mechanism requires and what it costs
//! when a write reaches the tree without one. Neither question says anything about **callers**: a
//! policy nothing calls is a policy that does not exist, and an audit found four production writers
//! outside it — the manual dictionary import, both recovery actions, the artifact restart, and the
//! command that changes where everything is stored.
//!
//! So each entry point here is driven with a claim already held, through the real
//! `invoke_handler`, and required to refuse. A refusal is observed the way the reader would meet
//! it: the returned error, or the event the interface waits on.
//!
//! # And ownership has to begin before the paths
//!
//! Claiming is not enough on its own — see `core::setup::claim`. The second half of this file
//! establishes the ordering the other way round: after a relocation, a writer that starts fresh
//! lands in the **new** location, because it resolves inside its claim rather than before it.
//!
//! # Why a child process
//!
//! Resource directories are resolved from the environment, and setting those variables in-process
//! would leak into every other test in this binary.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use sakatsu_core::setup::claim::{claim, Claim, Claimed};

mod common;

const CHILD: &str = "SAKATSU_WRITER_OWNERSHIP_CHILD";

fn scratch(name: &str) -> common::Scratch {
    let scratch = common::Scratch::new("writer-ownership", name);
    std::fs::create_dir_all(scratch.join("Local").join("com.sakatsu.app")).expect("profile");
    scratch
}

fn run_child(test: &str, profile: &Path) -> std::process::Output {
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let local = profile.join("Local");
    std::process::Command::new(&exe)
        .args([test, "--exact", "--nocapture"])
        .env(CHILD, profile)
        .env("APPDATA", profile.join("Roaming"))
        .env("LOCALAPPDATA", &local)
        .env("SAKATSU_DEVICE_DATA_DIR", local.join("com.sakatsu.app"))
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

/// A session holding what the writing commands need, registered the way production registers.
fn session() -> tauri::App<tauri::test::MockRuntime> {
    use sakatsu_desktop_lib::commands::{DictionaryState, KanjiState};
    use sakatsu_desktop_lib::live::Live;

    let app = tauri::test::mock_builder()
        .manage(sakatsu_desktop_lib::commands::AnalyzerState::fixed(
            sakatsu_desktop_lib::built_in_analyzer().0,
        ))
        .manage(sakatsu_desktop_lib::built_in_analyzer())
        .manage(DictionaryState(Live::new(std::sync::Arc::new(
            sakatsu_desktop_lib::commands::Dictionaries {
                dictionaries: Vec::new(),
                tags: sakatsu_core::dict::tags::TagGlossary::jmdict(),
            },
        ))))
        .manage(KanjiState::load(
            sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv"),
        ))
        .manage(sakatsu_desktop_lib::warmup::WarmupState::default())
        .manage(sakatsu_desktop_lib::setup_manager::SetupState::load())
        .manage(sakatsu_desktop_lib::commands::FetchState::default())
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

/// Wait for one event the interface waits on, or say which one never arrived.
fn await_event(
    app: &tauri::App<tauri::test::MockRuntime>,
    name: &'static str,
    trigger: impl FnOnce(),
) -> serde_json::Value {
    use tauri::Listener;

    let (sender, receiver) = mpsc::channel();
    let handle = app.listen(name, move |event| {
        let _ = sender.send(event.payload().to_string());
    });
    trigger();
    let payload = receiver
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap_or_else(|_| panic!("{name} never arrived"));
    app.unlisten(handle);
    serde_json::from_str(&payload).expect("the payload is JSON")
}

/// Everything, as a move holds it.
fn a_move_in_progress() -> Claimed {
    claim([Claim::Everything]).expect("nothing else is running in a fresh session")
}

/// A dictionary zip sitting in the dictionaries directory, ready for `import_dictionary`.
fn plant_a_zip(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).expect("dictionaries directory");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(dir.join(name)).unwrap());
    for (file, value) in [
        (
            "index.json",
            serde_json::json!({"title":"Ownership fixture","format":3}),
        ),
        (
            "term_bank_1.json",
            serde_json::json!([["dog", "dog", "n", "", 0, ["a dog"], 1, ""]]),
        ),
    ] {
        zip.start_file(file, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(value.to_string().as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

/// **A dictionary dropped in the folder is an install too.**
///
/// It writes the same containers `fetch_component` does and took no claim at all, so a reader could
/// import a zip in the middle of a move. The refusal reaches them the way success would: on the
/// `dictionary-imported` event the screen is waiting for.
#[test]
fn importing_a_dictionary_is_refused_while_the_resources_are_being_moved() {
    if let Some(profile) = std::env::var_os(CHILD) {
        let profile = PathBuf::from(profile);
        let dictionaries = sakatsu_desktop_lib::dictionaries_dir();
        plant_a_zip(&dictionaries, "fixture-yomitan.zip");
        let app = session();

        let moving = a_move_in_progress();
        let payload = await_event(&app, "dictionary-imported", || {
            invoke(
                &app,
                "import_dictionary",
                serde_json::json!({ "file": "fixture-yomitan.zip" }),
            )
            .expect("the command returns immediately and answers by event");
        });
        drop(moving);

        assert_eq!(payload["ok"], serde_json::json!(false), "{payload}");
        assert!(
            payload["detail"]
                .as_str()
                .unwrap_or_default()
                .contains("being moved"),
            "and says what is running, in a sentence: {payload}"
        );
        assert!(
            !dictionaries.join("fixture").exists(),
            "and nothing was imported"
        );
        let _ = profile;
        return;
    }

    let profile = scratch("import");
    let output = run_child(
        "importing_a_dictionary_is_refused_while_the_resources_are_being_moved",
        &profile,
    );
    expect_success(&output, "the import was not refused");
}

/// **The backfill refusal, on the event the interface listens to.**
///
/// The claim was already asserted in core; what was never observed is a reader being told. This is
/// the whole path: the command, the thread it spawns, the refusal, and `gloss-index-built`.
#[test]
fn the_gloss_backfill_reports_its_refusal_on_the_event_the_screen_waits_for() {
    if std::env::var_os(CHILD).is_some() {
        let app = session();

        let moving = a_move_in_progress();
        let payload = await_event(&app, "gloss-index-built", || {
            invoke(&app, "build_gloss_index", serde_json::json!({}))
                .expect("the command returns immediately and answers by event");
        });
        drop(moving);

        assert_eq!(payload["ok"], serde_json::json!(false), "{payload}");
        assert!(
            payload["detail"]
                .as_str()
                .unwrap_or_default()
                .contains("being moved"),
            "the reason names the move rather than an indexing failure: {payload}"
        );
        return;
    }

    let profile = scratch("backfill");
    let output = run_child(
        "the_gloss_backfill_reports_its_refusal_on_the_event_the_screen_waits_for",
        &profile,
    );
    expect_success(&output, "the backfill refusal did not reach the event");
}

/// The manual recovery actions publish generations, and were outside the policy entirely.
#[test]
fn the_recovery_actions_are_refused_while_the_resources_are_being_moved() {
    if std::env::var_os(CHILD).is_some() {
        let app = session();
        let moving = a_move_in_progress();
        for command in ["recovery_keep_current", "recovery_restore_previous"] {
            let refused = invoke(
                &app,
                command,
                serde_json::json!({
                    "container": "fixture",
                    "transaction": "t",
                    "published": "g-000000000002",
                }),
            )
            .expect_err("a move is in progress");
            assert!(
                refused["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("being moved"),
                "{command} must say what is running rather than what it could not read: {refused}"
            );
        }
        drop(moving);
        return;
    }

    let profile = scratch("recovery");
    let output = run_child(
        "the_recovery_actions_are_refused_while_the_resources_are_being_moved",
        &profile,
    );
    expect_success(&output, "a recovery action was not refused");
}

/// `setup_restart_artifact` deletes a download's work files, and only ever checked its own screen.
///
/// `SetupState.running` is the onboarding worker's flag. The compatibility `fetch_component`
/// command installs the same files and never sets it, so pausing setup said nothing about whether a
/// download was in flight — and this deletes exactly that download's partial files.
#[test]
fn restarting_an_artifact_is_refused_while_that_component_is_being_installed() {
    if std::env::var_os(CHILD).is_some() {
        let app = session();
        let installing = claim([Claim::Component("kanji")]).expect("nothing else is running");
        let refused = invoke(
            &app,
            "setup_restart_artifact",
            serde_json::json!({ "componentId": "kanji" }),
        )
        .expect_err("that component is being installed by the other screen");
        assert!(
            refused
                .as_str()
                .unwrap_or_default()
                .contains("already being installed"),
            "{refused}"
        );
        drop(installing);

        // And with nothing running it goes through, so the refusal above is about the claim.
        invoke(
            &app,
            "setup_restart_artifact",
            serde_json::json!({ "componentId": "kanji" }),
        )
        .expect("nothing is installing now");
        return;
    }

    let profile = scratch("restart");
    let output = run_child(
        "restarting_an_artifact_is_refused_while_that_component_is_being_installed",
        &profile,
    );
    expect_success(&output, "the artifact restart was not refused");
}

/// Changing where everything is stored is a write to the whole tree, even though no bytes move.
#[test]
fn choosing_storage_is_refused_while_a_component_is_being_installed() {
    if let Some(profile) = std::env::var_os(CHILD) {
        let profile = PathBuf::from(profile);
        let elsewhere = profile.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let app = session();

        let installing =
            claim([Claim::Component("dictionaries")]).expect("nothing else is running");
        let refused = invoke(
            &app,
            "setup_choose_storage",
            serde_json::json!({ "path": elsewhere.display().to_string() }),
        )
        .expect_err("an install is publishing into the location this would change");
        assert!(
            refused
                .as_str()
                .unwrap_or_default()
                .contains("already being installed"),
            "{refused}"
        );
        drop(installing);
        return;
    }

    let profile = scratch("storage");
    let output = run_child(
        "choosing_storage_is_refused_while_a_component_is_being_installed",
        &profile,
    );
    expect_success(&output, "choosing storage was not refused");
}

/// A fetch asked for during a move is refused **before it resolves anywhere to put the files**.
///
/// The belt and the braces: the profile's storage location is a path that cannot be created, so a
/// regression that let this through would fail on the destination rather than start a download.
#[test]
fn fetching_a_component_is_refused_while_the_resources_are_being_moved() {
    if std::env::var_os(CHILD).is_some() {
        let app = session();
        let moving = a_move_in_progress();
        let payload = await_event(&app, "fetch-done", || {
            let accepted = invoke(
                &app,
                "fetch_component",
                serde_json::json!({ "id": "kanji" }),
            )
            .expect("the command answers whether it took the job on");
            assert_eq!(
                accepted,
                serde_json::json!(true),
                "it accepts the job and reports the refusal by event, as it does every failure"
            );
        });
        drop(moving);

        assert_eq!(payload["ok"], serde_json::json!(false), "{payload}");
        assert!(
            payload["detail"]
                .as_str()
                .unwrap_or_default()
                .contains("being moved"),
            "refused for the move, not for anything it found on disk: {payload}"
        );
        return;
    }

    let profile = scratch("fetch");
    // A file where the storage directory would be. If ownership ever stopped covering this path,
    // the fetch would fail here instead of reaching the network.
    let blocked = profile.join("blocked");
    std::fs::write(&blocked, b"not a directory").expect("plant the obstruction");
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let output = std::process::Command::new(&exe)
        .args([
            "fetching_a_component_is_refused_while_the_resources_are_being_moved",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, &*profile)
        .env("APPDATA", profile.join("Roaming"))
        .env("LOCALAPPDATA", profile.join("Local"))
        .env("SAKATSU_DEVICE_DATA_DIR", blocked.join("com.sakatsu.app"))
        .env_remove("SAKATSU_KANJI_DIR")
        .env_remove("SAKATSU_DICT_DIR")
        .output()
        .expect("spawn a fresh session");
    expect_success(&output, "the fetch was not refused");
}

/// **Ownership begins before the paths, shown from the other side.**
///
/// A writer that resolved where the dictionaries were and *then* claimed would import into the
/// directory the move abandoned — that is what `core/tests/resource_coordination.rs` reproduces.
/// This asks the production command the question: after a completed move, does an import land where
/// the app now reads? It can only do so by resolving inside its claim.
#[test]
fn an_import_after_a_move_lands_where_the_app_now_reads() {
    if let Some(profile) = std::env::var_os(CHILD) {
        let profile = PathBuf::from(profile);
        let before = sakatsu_desktop_lib::dictionaries_dir();
        plant_a_zip(&before, "fixture-yomitan.zip");

        // A real move, through the production command, with everything it owns.
        let elsewhere = profile.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let app = session();
        invoke(
            &app,
            "setup_move_resources",
            serde_json::json!({ "target": elsewhere.display().to_string(), "removeOld": false }),
        )
        .expect("the move runs");

        let after = sakatsu_desktop_lib::dictionaries_dir();
        assert_ne!(
            after, before,
            "the move has to change where the app looks, or this test proves nothing"
        );
        plant_a_zip(&after, "second-yomitan.zip");

        let payload = await_event(&app, "dictionary-imported", || {
            invoke(
                &app,
                "import_dictionary",
                serde_json::json!({ "file": "second-yomitan.zip" }),
            )
            .expect("the command returns immediately");
        });
        assert_eq!(payload["ok"], serde_json::json!(true), "{payload}");
        assert!(
            after.join("second").exists(),
            "it imported into the new location"
        );
        assert!(
            !before.join("second").exists(),
            "and not into the one the move abandoned"
        );
        return;
    }

    let profile = scratch("after-move");
    // **No `SAKATSU_DEVICE_DATA_DIR` here.** That override wins over the location a move writes —
    // it exists so a developer can force a path — so a child that sets it could not observe the
    // move changing anything. The profile is isolated by `APPDATA`/`LOCALAPPDATA` alone, and the
    // move's activation writes the location file the app then reads.
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let output = std::process::Command::new(&exe)
        .args([
            "an_import_after_a_move_lands_where_the_app_now_reads",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, &*profile)
        .env("APPDATA", profile.join("Roaming"))
        .env("LOCALAPPDATA", profile.join("Local"))
        .env_remove("SAKATSU_DEVICE_DATA_DIR")
        .env_remove("SAKATSU_KANJI_DIR")
        .env_remove("SAKATSU_DICT_DIR")
        .output()
        .expect("spawn a fresh session");
    expect_success(&output, "the import did not follow the move");
}

/// The progress event, watched through a real listener.
///
/// **What this does and does not establish.** It pins the event name and the payload the frontend
/// destructures, through the emitter production uses. It is not a production *trigger*: every path
/// that emits during a transfer needs either a network download or artifacts pinned by hash, so
/// there is nothing offline to drive. That gap is recorded rather than papered over.
#[test]
fn the_progress_event_carries_the_fields_the_frontend_reads() {
    if std::env::var_os(CHILD).is_some() {
        let app = session();
        let payload = await_event(&app, "fetch-progress", || {
            sakatsu_desktop_lib::commands::say(app.handle(), "dictionaries", "importing…");
        });

        assert_eq!(payload["id"], serde_json::json!("dictionaries"));
        assert_eq!(payload["file"], serde_json::json!("importing…"));
        // The three the handwritten interface never declared, and the two it made optional.
        for field in ["step", "steps", "done", "total", "speed", "eta", "attempt"] {
            assert!(
                payload.get(field).is_some(),
                "{field} is missing from the wire: {payload}"
            );
        }
        assert_eq!(
            payload["step"],
            serde_json::json!(1),
            "one step of one, said out loud rather than left to a default: {payload}"
        );
        assert!(payload["total"].is_null(), "nothing measurable: {payload}");
        return;
    }

    let profile = scratch("progress");
    let output = run_child(
        "the_progress_event_carries_the_fields_the_frontend_reads",
        &profile,
    );
    expect_success(&output, "the progress event did not carry its fields");
}

/// The offline package path participates in ownership too.
///
/// It is the one install path the online wrapper does not cover: `setup_import_offline` reserves
/// every essential component itself and adopts inside that reservation. A *complete* package cannot
/// be synthesised — a valid one needs artifacts pinned by byte count and hash — but the reservation
/// is taken as soon as the manifest is readable and before any payload is, so a manifest that
/// passes the version checks and nothing else reaches exactly the point this is about. With a move
/// in progress the reservation cannot be taken, and the screen is told why rather than being told
/// about the package.
#[test]
fn the_offline_import_is_refused_while_the_resources_are_being_moved() {
    if let Some(profile) = std::env::var_os(CHILD) {
        // Readable, current, and empty of components: enough to get past the version checks and
        // reach the reservation, and not enough to install anything if it ever got past that.
        let package = PathBuf::from(&profile).join("essentials.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            serde_json::json!({
                "package_version": "ownership-fixture",
                "compatible_app_versions": [env!("CARGO_PKG_VERSION")],
                "components": [],
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
        zip.finish().unwrap();

        let app = session();
        let moving = a_move_in_progress();
        let accepted = invoke(
            &app,
            "setup_import_offline",
            serde_json::json!({ "packagePath": package.display().to_string() }),
        )
        .expect("the command answers whether it took the job on");
        assert_eq!(accepted, serde_json::json!(true), "it takes the job on");

        // The worker reports through the setup state rather than an event of its own.
        let mut issue = String::new();
        for _ in 0..300 {
            let snapshot = invoke(&app, "setup_snapshot", serde_json::json!({})).expect("snapshot");
            if snapshot["phase"] == serde_json::json!("needs_attention") {
                issue = snapshot["current_issue"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(moving);
        assert!(
            issue.contains("being moved"),
            "it must stop for the move rather than for the missing package: {issue:?}"
        );
        return;
    }

    let profile = scratch("offline");
    let output = run_child(
        "the_offline_import_is_refused_while_the_resources_are_being_moved",
        &profile,
    );
    expect_success(&output, "the offline import was not refused");
}
