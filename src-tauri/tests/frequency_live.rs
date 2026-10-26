//! Does switching the frequency list actually change what the reader sees?
//!
//! Ignored by default: needs imported dictionaries with a gloss index and at least one
//! frequency list on disk. Run it after touching the ranking:
//!
//! ```text
//! cargo test -p sakatsu-desktop --test frequency_live -- --ignored --nocapture
//! ```
//!
//! **Written to answer a complaint that switching lists changed nothing visible.** It drives the
//! real `search_words` command through the real state, with `reload_frequency` in between, so it
//! covers the whole path the interface uses — including the warmup cache, which would happily
//! serve the previous ordering if the reload did not invalidate it.

use sakatsu_core::analyzer::StubAnalyzer;
use sakatsu_core::dict::frequency::Direction;
use sakatsu_desktop_lib::commands::{
    AnalyzerState, AudioState, FrequencyState, KanjiState, OcrState, StoreState, TranslatorState,
};
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;

type Mock = tauri::test::MockRuntime;

fn app() -> (tauri::App<Mock>, tauri::WebviewWindow<Mock>) {
    let dictionaries =
        sakatsu_desktop_lib::discover_dictionaries(&sakatsu_desktop_lib::dictionaries_dir());
    let app = mock_builder()
        .manage(AnalyzerState::fixed(std::sync::Arc::new(StubAnalyzer)))
        .manage(StoreState {
            store: None,
            error: Some("no store in this test".into()),
        })
        .manage(OcrState::new(
            std::path::PathBuf::new(),
            std::path::PathBuf::new(),
            &sakatsu_core::store::Settings::default(),
        ))
        .manage(dictionaries)
        .manage(sakatsu_desktop_lib::commands::ParticleState::load(
            sakatsu_desktop_lib::wordnet_dir().join("categories.tsv"),
        ))
        // Starts with none, exactly as a profile that has never chosen one does. Each case
        // below installs the list it wants through `reload_frequency`, which is the command the
        // settings screen calls.
        .manage(FrequencyState(std::sync::RwLock::new(None)))
        .manage(sakatsu_desktop_lib::warmup::WarmupState::default())
        .manage(KanjiState::load(
            sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv"),
        ))
        .manage(AudioState::default())
        .manage(TranslatorState::with_engine(std::sync::Arc::new(
            sakatsu_core::translate::StubTranslator,
        )))
        .invoke_handler(tauri::generate_handler![
            sakatsu_desktop_lib::commands::search_words,
            sakatsu_desktop_lib::commands::reload_frequency,
            sakatsu_desktop_lib::commands::gloss_index_ready,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
        .build()
        .expect("build mock webview");
    (app, webview)
}

fn call(
    webview: &tauri::WebviewWindow<Mock>,
    cmd: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    tauri::test::get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
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

/// The headwords a search returned, in the order the reader would see them.
fn order(webview: &tauri::WebviewWindow<Mock>, query: &str) -> (Vec<String>, Option<String>) {
    let found = call(
        webview,
        "search_words",
        serde_json::json!({ "query": query }),
    )
    .expect("search_words");
    // `WordHit` flattens its `word`, so the headword is at the top level. Reading it from
    // `hit["word"]["headword"]` yielded `null` for every entry, and a vector of identical
    // placeholders compares equal however the results were ordered - which made this test
    // report "switching changed nothing" no matter what the ranking did.
    let words: Vec<String> = found["words"]
        .as_array()
        .expect("words")
        .iter()
        .map(|hit| {
            hit["headword"]
                .as_str()
                .unwrap_or_else(|| panic!("a hit with no headword: {hit}"))
                .to_string()
        })
        .collect();
    (words, found["ranked_by"].as_str().map(str::to_string))
}

fn use_list(webview: &tauri::WebviewWindow<Mock>, source: &str, direction: &str) -> Option<String> {
    call(
        webview,
        "reload_frequency",
        serde_json::json!({ "source": source, "direction": direction }),
    )
    .expect("reload_frequency")
    .as_str()
    .map(str::to_string)
}

fn installed(name: &str) -> bool {
    sakatsu_core::dict::frequency::is_present(
        &sakatsu_desktop_lib::dictionaries_dir()
            .join("frequency")
            .join(name),
    )
}

#[test]
#[ignore = "needs imported dictionaries with a gloss index and a frequency list on disk"]
fn switching_the_list_reorders_an_english_search() {
    let (_app, webview) = app();

    if !call(&webview, "gloss_index_ready", serde_json::json!({}))
        .expect("gloss_index_ready")
        .as_bool()
        .unwrap_or(false)
    {
        println!("SKIP: no gloss index — English search cannot answer");
        return;
    }
    if !installed("jpdb") || !installed("bccwj") {
        println!("SKIP: both frequency lists are needed to compare them");
        return;
    }

    // English, because that is the only branch the ranking reaches: a Japanese query returns
    // one entry per token in reading order and leaves `ranked_by` empty on purpose.
    let query = "feeling";

    let (unranked, ranked_by) = order(&webview, query);
    println!("no list      ranked_by={ranked_by:?}  n={}", unranked.len());
    assert!(
        ranked_by.is_none(),
        "with no list installed nothing should claim to have ordered the results"
    );
    assert!(
        unranked.len() > 3,
        "this query has to return enough words for an order to be visible, got {}",
        unranked.len()
    );

    let jpdb_name = use_list(&webview, "jpdb", "rank");
    let (jpdb, jpdb_ranked_by) = order(&webview, query);
    println!("jpdb  rank   ranked_by={jpdb_ranked_by:?}");
    println!("  {:?}", &jpdb[..jpdb.len().min(8)]);
    assert!(jpdb_name.is_some(), "jpdb must load");
    assert_eq!(
        jpdb_ranked_by, jpdb_name,
        "the results must name the list that ordered them"
    );
    assert_ne!(
        jpdb, unranked,
        "installing a list has to change the order, or the ranking is not reaching the results"
    );

    let bccwj_name = use_list(&webview, "bccwj", "rank");
    let (bccwj, bccwj_ranked_by) = order(&webview, query);
    println!("bccwj rank   ranked_by={bccwj_ranked_by:?}");
    println!("  {:?}", &bccwj[..bccwj.len().min(8)]);
    assert!(bccwj_name.is_some(), "bccwj must load");
    assert_eq!(bccwj_ranked_by, bccwj_name);

    // **The complaint, as an assertion.** Switching between the two lists must change something,
    // or the setting is offering a choice that does not exist. If this fails, the cache is
    // serving a stale order or the two corpora simply agree about this query.
    assert_ne!(
        jpdb, bccwj,
        "switching the list changed nothing: jpdb and bccwj returned the identical order"
    );

    // **A wrong direction setting must not reach a list the app fetched itself.** Both offered
    // lists are ranks; read as counts, every value is negated and the search leads with the
    // rarest word in the dictionary. This profile really was set to `count`, and the results
    // opened with the likes of `感取` where `気持ち` belongs. `shipped_direction` now answers for
    // these two, so the setting is left to do its job for a list the reader installed instead.
    let _ = use_list(&webview, "jpdb", "count");
    let (with_count, _) = order(&webview, query);
    println!("jpdb  count  {:?}", &with_count[..with_count.len().min(8)]);
    assert_eq!(
        with_count, jpdb,
        "a shipped list must read the same whatever the direction setting says"
    );
}

/// A Japanese query deliberately does not rank, and says so by leaving `ranked_by` empty.
///
/// Recorded as a test because it is the likeliest explanation for "switching the list changes
/// nothing": in a Japanese reading tool, a Japanese query is the one most likely to be typed.
#[test]
#[ignore = "needs imported dictionaries and a frequency list on disk"]
fn a_japanese_query_is_not_ranked_at_all() {
    let (_app, webview) = app();
    if !installed("jpdb") {
        println!("SKIP: no jpdb frequency list");
        return;
    }
    use_list(&webview, "jpdb", "rank");

    let (words, ranked_by) = order(&webview, "気持ち");
    println!("japanese query: n={} ranked_by={ranked_by:?}", words.len());
    assert!(
        ranked_by.is_none(),
        "a Japanese query returns tokens in reading order, and must not claim a corpus ordered them"
    );
}

/// What the two lists say about the same words, printed rather than asserted.
///
/// Diagnostic: the lists agreeing about common words is not a fault, but it is the reason
/// switching between them can look like it did nothing.
#[test]
#[ignore = "needs both frequency lists on disk"]
fn how_much_the_two_lists_actually_disagree() {
    if !installed("jpdb") || !installed("bccwj") {
        println!("SKIP: both frequency lists are needed to compare them");
        return;
    }
    let root = sakatsu_desktop_lib::dictionaries_dir().join("frequency");
    let jpdb =
        sakatsu_core::dict::frequency::Frequencies::load(&root.join("jpdb"), Direction::Rank);
    let bccwj =
        sakatsu_core::dict::frequency::Frequencies::load(&root.join("bccwj"), Direction::Rank);
    println!("jpdb {} keys, bccwj {} keys", jpdb.len(), bccwj.len());

    for word in ["気持ち", "感じ", "気分", "感情", "心地", "情"] {
        println!(
            "  {word:<6} jpdb={:?} bccwj={:?}",
            jpdb.rank(word, ""),
            bccwj.rank(word, "")
        );
    }
}
