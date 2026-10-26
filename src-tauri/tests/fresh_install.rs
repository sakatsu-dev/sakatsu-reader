//! What someone handed the installer actually gets.
//!
//! ```powershell
//! $env:APPDATA = "$env:TEMP\sakatsu-fresh"
//! cargo test -p sakatsu-desktop --test fresh_install -- --ignored --nocapture
//! ```
//!
//! # Why this exists
//!
//! Every other test runs on a machine where everything is already installed, which is the
//! one state a new user is never in. The bug this feature was written for — a setup panel
//! naming scripts that are not on the disk — could only have been found by looking at a
//! fresh install, and was not, for months.
//!
//! `app_data_dir()` reads `APPDATA`, so pointing that at an empty directory is a complete
//! simulation: dictionaries, models, kanji table, particle categories and the store all
//! move with it. Nothing touches the real installation.
//!
//! The test refuses to run against a real one, because a test that quietly measures the
//! wrong thing is worse than one that does not run.

use sakatsu_core::analyzer::StubAnalyzer;
use sakatsu_core::store::Settings;
use sakatsu_desktop_lib::commands::{
    AnalyzerState, KanjiState, OcrState, ParticleState, StoreState,
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;

const INVOKE_KEY: &str = "__invoke-key__";

/// Build the app exactly as `run()` does, from whatever `APPDATA` currently says.
fn app() -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(AnalyzerState::fixed(std::sync::Arc::new(StubAnalyzer)))
        // Whatever discovery finds, whole. This used to rebuild the state by hand with
        // `TagGlossary::jmdict()`, which on the install these tests describe is the same
        // table `discover_dictionaries` ends up with anyway — it starts from jmdict and
        // merges each dictionary's own bank over it, and there are no dictionaries here.
        .manage(sakatsu_desktop_lib::discover_dictionaries(
            &sakatsu_desktop_lib::dictionaries_dir(),
        ))
        .manage(StoreState {
            store: None,
            error: None,
        })
        .manage(OcrState::new(
            sakatsu_desktop_lib::model_dir(),
            sakatsu_desktop_lib::paddle_model_dir(),
            &Settings::default(),
        ))
        .manage(ParticleState::load(
            sakatsu_desktop_lib::wordnet_dir().join("categories.tsv"),
        ))
        .manage(KanjiState::load(
            sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv"),
        ))
        .invoke_handler(tauri::generate_handler![
            sakatsu_desktop_lib::commands::readiness,
            sakatsu_desktop_lib::commands::catalog,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app")
}

/// One webview for the whole test: labels are unique, and a second "main" is an error
/// rather than a second window.
fn ask(webview: &tauri::WebviewWindow<tauri::test::MockRuntime>, cmd: &str) -> serde_json::Value {
    tauri::test::get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().expect("valid url"),
            body: tauri::ipc::InvokeBody::Json(serde_json::json!({})),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| {
        body.deserialize::<serde_json::Value>()
            .expect("deserializes")
    })
    .unwrap_or_else(|error| panic!("{cmd}: {error}"))
}

/// Whether `APPDATA` really points somewhere with no installation in it.
fn is_fresh() -> bool {
    let root = sakatsu_desktop_lib::dictionaries_dir();
    !root.exists()
        || std::fs::read_dir(&root)
            .map(|entries| entries.count() == 0)
            .unwrap_or(true)
}

#[test]
#[ignore = "set APPDATA to an empty directory first; downloads from the internet"]
fn a_fresh_install_can_set_itself_up() {
    if !is_fresh() {
        println!(
            "SKIP: {} is not empty — point APPDATA at a scratch directory",
            sakatsu_desktop_lib::dictionaries_dir().display()
        );
        return;
    }
    println!("app data: {}", sakatsu_desktop_lib::kanji_dir().display());

    let app = app();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build mock webview");

    // 1. The app knows what it is missing, and every row can be acted on.
    let readiness = ask(&webview, "readiness");
    let components = readiness["components"].as_array().expect("components");
    println!("\nwhat a new install reports:");
    for component in components {
        println!(
            "  {:<28} {}",
            component["name"].as_str().unwrap_or("?"),
            if component["ready"].as_bool().unwrap_or(false) {
                "ready"
            } else {
                "MISSING"
            }
        );
    }
    // Pasted text works before a download, but the supported capture setup is not complete
    // until both complementary OCR engines are present.
    let missing_required: Vec<&str> = components
        .iter()
        .filter(|component| {
            component["required"].as_bool().unwrap_or(false)
                && !component["ready"].as_bool().unwrap_or(false)
        })
        .filter_map(|component| component["name"].as_str())
        .collect();
    assert_eq!(
        missing_required,
        ["Recognition — manga-ocr", "Recognition — PP-OCRv6"]
    );

    // 2. The catalog offers everything, and admits none of it is here.
    let catalog = ask(&webview, "catalog");
    let entries = catalog.as_array().expect("catalog");
    println!("\nwhat it offers:");
    for entry in entries {
        println!(
            "  {:<18} {:>8}  {}",
            entry["label"].as_str().unwrap_or("?"),
            entry["size"].as_str().unwrap_or("?"),
            if entry["fetchable"].as_bool().unwrap_or(false) {
                "button"
            } else {
                "instructions"
            }
        );
    }
    assert!(
        entries
            .iter()
            .filter(|entry| entry["id"] != serde_json::json!("ichiran"))
            .all(|entry| !entry["ready"].as_bool().unwrap_or(true)),
        "nothing should be installed yet"
    );

    // 3. Ichiran is not misrepresented as an ordinary artifact download. Its catalog
    // entry is handled by the separate guided setup action.
    let ichiran = entries
        .iter()
        .find(|entry| entry["id"] == serde_json::json!("ichiran"))
        .expect("ichiran is listed");
    assert_eq!(ichiran["fetchable"], serde_json::json!(false));

    // 4. And a fetch actually works, end to end, into the directory the app reads.
    //
    // The kanji table, because it is 1.4 MB rather than 461 and exercises the whole path:
    // download, decompress, parse, write, and then be seen by the state the app loads.
    let destination = sakatsu_desktop_lib::commands::destination("kanji").expect("a directory");
    let component = sakatsu_core::setup::catalog::find("kanji").expect("in the catalog");
    let artifact = &component.artifacts[0];
    let archive = destination.join(artifact.file);

    println!("\nfetching {} …", component.label);
    let started = std::time::Instant::now();
    sakatsu_core::setup::download(
        artifact.url,
        &archive,
        artifact.min_bytes,
        &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        &mut |_| {},
    )
    .expect("downloads");
    let count = sakatsu_core::setup::kanji_table(&archive, &destination.join("kanjidic.tsv"))
        .expect("builds the table");
    println!(
        "  {count} characters in {:.1}s",
        started.elapsed().as_secs_f64()
    );

    // **In the same session.**
    //
    // This is the assertion that was missing, and its absence is why a review found the
    // bug instead of this test. `installed()` used to answer from the state loaded at
    // startup, so a component that had just been fetched still reported missing: the
    // setup screen offered its 95 MB button again and pressing it re-downloaded and
    // re-imported the lot. Only asserting that a *restarted* app sees the file agreed
    // with the bug rather than catching it.
    let after = ask(&webview, "catalog");
    let kanji = after
        .as_array()
        .expect("catalog")
        .iter()
        .find(|entry| entry["id"] == serde_json::json!("kanji"))
        .expect("kanji is listed");
    assert_eq!(
        kanji["ready"],
        serde_json::json!(true),
        "a fetched component must report installed without restarting, or the setup          screen offers to fetch it all over again"
    );

    // And a *running* app can use it. This used to reopen the table to prove a restarted
    // app could read it, because the tables were chosen at startup and deliberately not
    // swapped mid-session. `KanjiState::reload` is what `commands::adopt` calls the moment
    // the fetch finishes, so the claim is now the stronger one: no restart is involved.
    let live = KanjiState::load(std::path::PathBuf::from("nothing-is-here.tsv"));
    assert!(live.0.now().is_empty(), "starts with no table at all");
    live.reload();
    assert!(
        !live.0.now().is_empty(),
        "and picks one up without being rebuilt"
    );
    assert!(
        live.0.now().get('食').is_some(),
        "食 is readable after setup, in the app that fetched it"
    );
    println!("  catalog says ready, and the running app can read 食");
}
