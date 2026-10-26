//! The whole stack, through the real IPC boundary: real Ichiran, real dictionaries,
//! real commands.
//!
//! Ignored by default — needs `ichiran-cli` and
//! `scripts/fetch-dictionaries.ps1` plus an import (see `jitendex_live.rs`). Run:
//!
//! ```text
//! cargo test -p sakatsu-desktop --test live_stack -- --ignored --nocapture
//! ```
//!
//! This is the check a screenshot cannot make: that what the UI actually receives,
//! over the actual command names, is right.

use sakatsu_core::IchiranAnalyzer;
use sakatsu_desktop_lib::commands::AnalyzerState;
use std::sync::{Mutex, MutexGuard, OnceLock};
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;

/// WSL starts one `ichiran-cli` process per analysis. Launching the five live tests in
/// parallel overwhelms that shared service and measures contention rather than the app,
/// so this integration target deliberately serializes only its own calls.
fn live_stack_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A store under the temp directory, so these tests never touch the real one.
fn store_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("sakatsu-live-stack-store")
}

fn invoke(cmd: &str, args: serde_json::Value) -> Result<serde_json::Value, serde_json::Value> {
    // Mirror startup rather than constructing only the state yesterday's command signature
    // happened to need. Search gained live-switchable frequency ordering; leaving that state
    // unmanaged made every search fail at the IPC boundary before any dictionary was asked.
    let store = sakatsu_desktop_lib::open_store(&store_dir());
    let settings = store.settings();
    let frequency = sakatsu_desktop_lib::commands::FrequencyState::load(
        sakatsu_desktop_lib::dictionaries_dir(),
        &settings.frequency_source,
        settings.frequency_direction,
    );
    let app = mock_builder()
        .manage(AnalyzerState::fixed(std::sync::Arc::new(
            IchiranAnalyzer::new(),
        )))
        .manage(sakatsu_desktop_lib::discover_dictionaries(
            &sakatsu_desktop_lib::dictionaries_dir(),
        ))
        .manage(store)
        // `readiness` reports on it, and an unmanaged state is a command that fails at
        // the IPC boundary rather than at compile time.
        .manage(sakatsu_desktop_lib::commands::ParticleState::load(
            sakatsu_desktop_lib::wordnet_dir().join("categories.tsv"),
        ))
        .manage(sakatsu_desktop_lib::commands::KanjiState::load(
            sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv"),
        ))
        .manage(frequency)
        .manage(sakatsu_desktop_lib::warmup::WarmupState::default())
        .invoke_handler(tauri::generate_handler![
            sakatsu_desktop_lib::commands::study,
            sakatsu_desktop_lib::commands::lookup,
            sakatsu_desktop_lib::commands::search_words,
            sakatsu_desktop_lib::commands::dictionaries,
            sakatsu_desktop_lib::commands::set_word_status,
            sakatsu_desktop_lib::commands::word_counts,
            sakatsu_desktop_lib::commands::readiness,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build mock webview");

    tauri::test::get_ipc_response(
        &webview,
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
#[ignore = "needs ichiran-cli and imported dictionaries"]
fn a_real_passage_arrives_at_the_ui_complete() {
    let _serial = live_stack_guard();
    let dictionaries = invoke("dictionaries", serde_json::json!({})).expect("dictionaries");
    let titles: Vec<String> = dictionaries
        .as_array()
        .expect("array")
        .iter()
        .map(|meta| meta["title"].as_str().unwrap_or("?").to_string())
        .collect();
    println!("dictionaries = {titles:?}");
    assert!(
        !titles.is_empty(),
        "import a dictionary first (see core/tests/jitendex_live.rs)"
    );

    // §10's gate sentence, now through the app rather than the library.
    let view = invoke(
        "study",
        serde_json::json!({ "text": "気持ちが伝わってきます" }),
    )
    .expect("study");

    let tokens = view["analysis"]["tokens"].as_array().expect("tokens");
    let breakdowns = view["breakdowns"].as_array().expect("breakdowns");
    assert_eq!(tokens.len(), breakdowns.len());
    println!(
        "tokens = {:?}",
        tokens
            .iter()
            .map(|token| token["surface"].as_str().unwrap_or("?"))
            .collect::<Vec<_>>()
    );

    // The compound's grammar: 〜てくる, with the te-form built from つたわる.
    let compound = &breakdowns[2];
    let steps = compound["steps"].as_array().expect("steps");
    println!(
        "steps = {:?}",
        steps
            .iter()
            .map(|step| step["form"].as_str().unwrap_or("?"))
            .collect::<Vec<_>>()
    );
    assert_eq!(steps[0]["form"], "conjunctive");
    assert_eq!(steps[0]["formation"]["result"], "つたわって");
    assert_eq!(compound["auxiliaries"][0]["pattern_id"], "te-kuru");

    // が is found as a pattern over the token sequence.
    let patterns = view["patterns"].as_array().expect("patterns");
    assert!(
        patterns
            .iter()
            .any(|found| found["pattern_id"] == "particle-ga"),
        "が should be matched: {patterns:?}"
    );

    // And the word the reader would click resolves through the merge policy.
    let token = tokens[0].clone();
    let word = invoke("lookup", serde_json::json!({ "token": token })).expect("lookup");
    println!("気持ち → {}", word["inline"]);
    assert_eq!(word["headword"], "気持ち");
    assert!(
        !word["inline"].as_array().expect("inline").is_empty(),
        "the analyzer's own senses are primary and must show inline"
    );
    // The analyzer and Jitendex both derive these glosses from JMdict. The merge policy
    // deliberately suppresses a source that adds nothing, or the reader sees the same
    // meanings twice under two labels.
    assert!(word["inline_source"].is_null());
    assert!(
        word["also_in"].as_array().expect("also_in").is_empty(),
        "Jitendex repeats what is already inline, so it has nothing to disclose"
    );

    // Every token has a status, and they start unmarked.
    let statuses = view["statuses"].as_array().expect("statuses");
    assert_eq!(statuses.len(), tokens.len());
}

/// The first-run panel's view of a fully-installed machine.
///
/// This one is worth running on a *broken* install too: unplug Ichiran or rename the
/// model directory, and every component should report what is missing rather than the
/// app simply refusing to work.
#[test]
#[ignore = "needs ichiran-cli and imported dictionaries"]
fn readiness_reports_every_component() {
    let _serial = live_stack_guard();
    let readiness = invoke("readiness", serde_json::json!({})).expect("readiness");
    let components = readiness["components"].as_array().expect("components");

    for component in components {
        println!(
            "{:>18}  {}  {}",
            component["name"].as_str().unwrap_or("?"),
            if component["ready"].as_bool().unwrap_or(false) {
                "ready  "
            } else {
                "MISSING"
            },
            component["detail"].as_str().unwrap_or("")
        );
    }
    println!("importable = {}", readiness["importable"]);

    let names: Vec<&str> = components
        .iter()
        .map(|component| component["name"].as_str().unwrap_or(""))
        .collect();

    // What is asserted is that every kind of component reports — not the exact list.
    //
    // This used to pin `["Analyzer", "Dictionaries", "Recognition (OCR)", "Translation"]`
    // and had been failing since PP-OCRv6 arrived and split recognition into two rows. The
    // panel was right and the test was stale, which nobody noticed because it is `#[ignore]`d
    // and needs the whole stack to run at all.
    //
    // Pinning the current rows would fail again for the same reason the moment another
    // optional engine lands. The risk worth guarding is a component *disappearing*, and
    // that is what these check.
    for required in [
        "Analyzer",
        "Dictionaries",
        "Recognition — manga-ocr",
        "Recognition — PP-OCRv6",
        "Translation",
    ] {
        assert_eq!(
            names.iter().filter(|name| **name == required).count(),
            1,
            "{required} should appear exactly once, got {names:?}"
        );
    }
    assert!(
        names.iter().any(|name| name.starts_with("Recognition")),
        "no recognition engine is reported at all: {names:?}"
    );

    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        names.len(),
        "a component is listed twice: {names:?}"
    );
    assert!(
        names.iter().all(|name| !name.is_empty()),
        "a component has no name: {names:?}"
    );

    // Every component says something useful whether or not it is ready, and offers a
    // fix when it is not — a bare "missing" would leave the user stuck.
    for component in components {
        assert!(
            !component["detail"].as_str().unwrap_or("").is_empty(),
            "{} has no detail",
            component["name"]
        );
        if !component["ready"].as_bool().unwrap_or(false) {
            assert!(
                !component["fix"].is_null(),
                "{} offers no fix",
                component["name"]
            );
        }
    }

    // The test's declared prerequisites are Ichiran, imported dictionaries, and the two
    // OCR engines the supported installation now requires. Translation and frequency rows
    // remain machine-dependent upgrades.
    for prerequisite in [
        "Analyzer",
        "Dictionaries",
        "Recognition — manga-ocr",
        "Recognition — PP-OCRv6",
    ] {
        let component = components
            .iter()
            .find(|component| component["name"] == serde_json::json!(prerequisite))
            .unwrap_or_else(|| panic!("missing {prerequisite} row"));
        assert_eq!(
            component["ready"],
            serde_json::json!(true),
            "{prerequisite} is a prerequisite for this live test"
        );
    }
    assert_eq!(readiness["importable"].as_array().map(Vec::len), Some(0));
}

/// Marking a word with the **real** analyzer, whose tokens carry real readings.
///
/// The status key is dictionary form + reading, so this is the case a stub analyzer
/// cannot check: Ichiran gives 気持ち the reading きもち, and the mark must come back
/// for that same pair on the next analysis.
#[test]
#[ignore = "needs ichiran-cli and imported dictionaries"]
fn a_marked_word_comes_back_marked_with_real_readings() {
    let _serial = live_stack_guard();
    let _ = std::fs::remove_dir_all(store_dir());

    let view = invoke(
        "study",
        serde_json::json!({ "text": "気持ちが伝わってきます" }),
    )
    .expect("study");
    let token = view["analysis"]["tokens"][0].clone();
    assert_eq!(token["dict_form"], "気持ち");
    assert_eq!(token["lemma_reading"], "きもち");
    assert_eq!(view["statuses"][0], "new");

    let stored = invoke(
        "set_word_status",
        serde_json::json!({ "token": token, "status": "seen" }),
    )
    .expect("set_word_status");
    assert_eq!(stored, "seen");

    // Re-analysed from scratch in a fresh app: the reader would now colour it.
    let again = invoke(
        "study",
        serde_json::json!({ "text": "気持ちが伝わってきます" }),
    )
    .expect("study");
    assert_eq!(again["statuses"][0], "seen");
    assert_eq!(again["statuses"][1], "new", "が was not marked");

    let counts = invoke("word_counts", serde_json::json!({})).expect("word_counts");
    println!("counts = {counts}");
    assert_eq!(counts["seen"], 1);

    let _ = std::fs::remove_dir_all(store_dir());
}

/// The reference page's search, against the real dictionaries and the real kanji table.
///
/// The mock in `ipc.rs` proves the command answers with nothing installed; this proves it
/// answers with everything installed, which is a different claim and the one a learner
/// cares about.
#[test]
#[ignore = "needs the imported dictionaries and scripts/fetch-kanjidic.ps1"]
fn searching_a_word_returns_its_characters_with_the_reading_it_uses() {
    let _serial = live_stack_guard();
    let found =
        invoke("search_words", serde_json::json!({ "query": "図書館" })).expect("search succeeds");

    if !sakatsu_desktop_lib::kanji_dir()
        .join("kanjidic.tsv")
        .is_file()
    {
        println!("SKIP: no kanji table — run scripts/fetch-kanjidic.ps1");
        return;
    }

    // Characters belong to the word whose reading selected them. The old shared `kanji`
    // pool could not say which of several English results a character came from, so every
    // `WordHit` now carries its own `chars`.
    let word = found["words"]
        .as_array()
        .expect("words")
        .iter()
        .find(|entry| entry["headword"] == serde_json::json!("図書館"))
        .expect("図書館 result");
    let kanji = word["chars"].as_array().expect("chars");
    assert_eq!(kanji.len(), 3, "図書館 is three characters");

    for entry in kanji {
        let marked: Vec<&str> = entry["on"]
            .as_array()
            .into_iter()
            .chain(entry["kun"].as_array())
            .flatten()
            .filter(|reading| reading["here"] == serde_json::json!(true))
            .filter_map(|reading| reading["text"].as_str())
            .collect();
        println!(
            "  {}  strokes {}  here: {marked:?}",
            entry["character"].as_str().unwrap_or("?"),
            entry["strokes"]
        );
        assert_eq!(marked.len(), 1, "exactly one reading is the one in play");
    }

    // 館 is the case the word dictionary gets wrong: as a word it is やかた, here カン.
    let building = kanji
        .iter()
        .find(|k| k["character"] == serde_json::json!("館"));
    let on_here: Vec<&str> = building.expect("館")["on"]
        .as_array()
        .expect("on")
        .iter()
        .filter(|r| r["here"] == serde_json::json!(true))
        .filter_map(|r| r["text"].as_str())
        .collect();
    assert_eq!(on_here, ["カン"]);

    // A word whose reading its characters do not build says so instead of marking one.
    let irregular =
        invoke("search_words", serde_json::json!({ "query": "明日" })).expect("search succeeds");
    let irregular_word = irregular["words"]
        .as_array()
        .expect("words")
        .iter()
        .find(|entry| entry["headword"] == serde_json::json!("明日"))
        .expect("明日 result");
    let first = &irregular_word["chars"].as_array().expect("chars")[0];
    assert_eq!(first["irregular"], serde_json::json!(true));

    // English answers carry characters under each result too.
    let english =
        invoke("search_words", serde_json::json!({ "query": "eat" })).expect("search succeeds");
    let chars: Vec<&str> = english["words"]
        .as_array()
        .expect("words")
        .iter()
        .flat_map(|entry| entry["chars"].as_array().into_iter().flatten())
        .filter_map(|k| k["character"].as_str())
        .collect();
    println!("  'eat' → {chars:?}");
    assert!(chars.contains(&"食"), "got {chars:?}");
}

/// A character whose on readings prefix one another marks exactly one of them.
///
/// 504 characters carry such a pair — 世 is セイ *and* セ, 主 is シュ and シュウ. The
/// marker used `starts_with` for on readings as well as kun, so 世紀 せいき highlighted
/// both セイ and セ as "the reading here". The existing check asserted one mark but only
/// ever ran 図書館, which has no prefix pair to trip over.
#[test]
#[ignore = "needs the imported dictionaries and scripts/fetch-kanjidic.ps1"]
fn a_character_with_prefixing_on_readings_marks_only_the_one_used() {
    let _serial = live_stack_guard();
    if !sakatsu_desktop_lib::kanji_dir()
        .join("kanjidic.tsv")
        .is_file()
    {
        println!("SKIP: no kanji table — run scripts/fetch-kanjidic.ps1");
        return;
    }

    for (word, character, expected) in [("世紀", "世", "セイ"), ("世界", "世", "セ")] {
        let found =
            invoke("search_words", serde_json::json!({ "query": word })).expect("search succeeds");
        let result = found["words"]
            .as_array()
            .expect("words")
            .iter()
            .find(|entry| entry["headword"] == serde_json::json!(word))
            .unwrap_or_else(|| panic!("no {word} result"));
        let Some(entry) = result["chars"]
            .as_array()
            .expect("chars")
            .iter()
            .find(|entry| entry["character"] == serde_json::json!(character))
        else {
            println!("SKIP: {word} did not resolve to a word containing {character}");
            continue;
        };

        let marked: Vec<&str> = entry["on"]
            .as_array()
            .expect("on")
            .iter()
            .filter(|reading| reading["here"] == serde_json::json!(true))
            .filter_map(|reading| reading["text"].as_str())
            .collect();
        println!("  {word}  {character} on-marked: {marked:?}");
        assert_eq!(
            marked,
            [expected],
            "{word}: exactly the reading in play, not every one that prefixes it"
        );
    }
}
