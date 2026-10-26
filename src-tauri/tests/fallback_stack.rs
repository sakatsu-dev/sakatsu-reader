//! The app on the built-in analyzer, with no Ichiran at all.
//!
//! This is the configuration a fresh install runs in, so it is worth exercising
//! directly rather than inferring: tokenizing, grammar, status and — the part that
//! carries the most weight — senses arriving from the dictionaries instead of the
//! analyzer, via the promotion rule the merge policy already had.
//!
//! Needs the dictionaries imported. It needs no Ichiran **by construction**: the
//! analyzer is constructed here rather than chosen, so the sidecar is not consulted.

use sakatsu_core::analyzer::LinderaAnalyzer;
use sakatsu_desktop_lib::commands::{AnalyzerState, KanjiState, StoreState};
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;

fn app() -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(AnalyzerState::fixed(std::sync::Arc::new(
            LinderaAnalyzer::new().expect("the dictionary is compiled in"),
        )))
        .manage(sakatsu_desktop_lib::discover_dictionaries(
            &sakatsu_desktop_lib::dictionaries_dir(),
        ))
        .manage(StoreState {
            store: None,
            error: Some("no store in this test".into()),
        })
        // `study` computes ruby in the same round trip. An empty table is a supported
        // first-run state, but it must still be managed or Tauri rejects the command
        // before the fallback analyzer gets a chance to run.
        .manage(KanjiState::load(
            sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv"),
        ))
        .invoke_handler(tauri::generate_handler![
            sakatsu_desktop_lib::commands::study,
            sakatsu_desktop_lib::commands::lookup,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app")
}

fn invoke(
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

#[test]
#[ignore = "needs imported dictionaries"]
fn a_fresh_install_reads_japanese_without_ichiran() {
    let owned = app();
    let webview = tauri::WebviewWindowBuilder::new(&owned, "main", tauri::WebviewUrl::default())
        .build()
        .expect("webview");

    let view = invoke(&webview, "study", serde_json::json!({ "text": "水を飲む" })).expect("study");
    let tokens = view["analysis"]["tokens"].as_array().expect("tokens");
    let surfaces: Vec<&str> = tokens
        .iter()
        .map(|token| token["surface"].as_str().unwrap_or("?"))
        .collect();
    println!("tokens = {surfaces:?}");
    assert_eq!(surfaces, ["水", "を", "飲む"]);

    // The parallel arrays the UI indexes by token still line up.
    assert_eq!(
        view["breakdowns"].as_array().map(Vec::len),
        Some(tokens.len())
    );
    assert_eq!(
        view["statuses"].as_array().map(Vec::len),
        Some(tokens.len())
    );

    // Particles are still matched: the grammar matcher works off tokens, not off
    // anything Ichiran-specific.
    let patterns = view["patterns"].as_array().expect("patterns");
    println!(
        "patterns = {:?}",
        patterns
            .iter()
            .map(|found| found["pattern_id"].as_str().unwrap_or("?"))
            .collect::<Vec<_>>()
    );
    assert!(
        patterns
            .iter()
            .any(|found| found["pattern_id"] == "particle-o"),
        "を should still be found"
    );
}

#[test]
#[ignore = "needs imported dictionaries"]
fn definitions_come_from_the_dictionaries_when_the_analyzer_has_none() {
    // The rule that makes the built-in analyzer adequate: UniDic has no senses, so the
    // merge policy promotes a dictionary and names it (project notes §6). Without this the
    // vocabulary panel would be empty for every word.
    let owned = app();
    let webview = tauri::WebviewWindowBuilder::new(&owned, "main", tauri::WebviewUrl::default())
        .build()
        .expect("webview");

    let view = invoke(&webview, "study", serde_json::json!({ "text": "水を飲む" })).expect("study");
    let token = view["analysis"]["tokens"][0].clone();
    assert_eq!(token["surface"], "水");
    assert!(
        token["senses"].as_array().expect("senses").is_empty(),
        "the built-in analyzer supplies no senses"
    );

    let word = invoke(&webview, "lookup", serde_json::json!({ "token": token })).expect("lookup");
    println!("水 → {}", word["inline"]);
    println!("   from {}", word["inline_source"]);

    assert!(
        !word["inline"].as_array().expect("inline").is_empty(),
        "a dictionary should have been promoted"
    );
    // Promoted, so it must be attributed — the learner is not reading JMdict-via-Ichiran
    // any more and deserves to know whose definition this is.
    assert!(
        word["inline_source"].is_string(),
        "a promoted definition must name its source"
    );
    let glosses = word["inline"][0]["glosses"].to_string();
    assert!(glosses.contains("water"), "got {glosses}");
}
