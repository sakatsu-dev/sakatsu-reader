//! Before/after latency probe for intent-aware warmup.
//!
//! This is an ignored measurement job, not a timing assertion: CI machines and Windows file
//! caches vary too much for a millisecond threshold to be honest. Keep the workload fixed and run
//! it in release mode, one process at a time:
//!
//! ```text
//! cargo test --release -p sakatsu-desktop --test warmup_benchmark -- --ignored --nocapture --test-threads=1
//! ```

use std::time::Instant;

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer, Remembering};
use sakatsu_desktop_lib::commands::{AnalyzerState, FrequencyState, KanjiState, StoreState};
use serde::Serialize;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;

const PASSAGE: &str =
    "気持ちが伝わってきます。しかも珍しく人間に友好的で、学校へ静かに歩いていきました。";
const JAPANESE_QUERIES: &[&str] = &[
    "美味しかった",
    "読み続ける",
    "懐かしい",
    "図書館",
    "気をつけて",
    "間に合わない",
];
const ENGLISH_QUERIES: &[&str] = &["unusual", "library", "continue reading"];

#[derive(Debug, Serialize)]
struct Measurements {
    startup_ms: f64,
    study_first_ms: f64,
    study_repeat_median_ms: f64,
    analysis_selection_first_pass_ms: f64,
    analysis_selection_repeat_pass_ms: f64,
    analysis_prefetch_work_ms: f64,
    analysis_after_prefetch_ms: f64,
    reference_japanese_first_pass_ms: f64,
    reference_japanese_repeat_pass_ms: f64,
    reference_japanese_prefetch_work_ms: f64,
    reference_japanese_after_prefetch_ms: f64,
    reference_english_first_pass_ms: f64,
    reference_english_repeat_pass_ms: f64,
    reference_english_prefetch_work_ms: f64,
    reference_english_after_prefetch_ms: f64,
}

fn app() -> tauri::App<MockRuntime> {
    let dictionaries =
        sakatsu_desktop_lib::discover_dictionaries(&sakatsu_desktop_lib::dictionaries_dir());
    assert!(
        !dictionaries.0.now().dictionaries.is_empty(),
        "import Jitendex before running the live latency benchmark"
    );
    mock_builder()
        .manage(AnalyzerState::fixed(std::sync::Arc::new(
            LinderaAnalyzer::new().expect("the embedded dictionary loads"),
        )))
        .manage(dictionaries)
        .manage(StoreState {
            store: None,
            error: Some("no persistent store in the latency probe".into()),
        })
        .manage(KanjiState::load(
            sakatsu_desktop_lib::kanji_dir().join("kanjidic.tsv"),
        ))
        .manage(FrequencyState(std::sync::RwLock::new(None)))
        .manage(sakatsu_desktop_lib::warmup::WarmupState::default())
        .invoke_handler(tauri::generate_handler![
            sakatsu_desktop_lib::commands::study,
            sakatsu_desktop_lib::commands::lookup_in_context,
            sakatsu_desktop_lib::commands::search_words,
            sakatsu_desktop_lib::commands::prefetch_reference,
            sakatsu_desktop_lib::commands::prefetch_analysis,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app")
}

fn call(
    webview: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    tauri::test::get_ipc_response(
        webview,
        InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().expect("valid URL"),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .unwrap_or_else(|error| panic!("{command} failed: {error}"))
    .deserialize::<serde_json::Value>()
    .expect("response deserializes")
}

fn timed<T>(work: impl FnOnce() -> T) -> (T, f64) {
    let started = Instant::now();
    let value = work();
    (value, started.elapsed().as_secs_f64() * 1_000.0)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn search_pass(
    webview: &tauri::WebviewWindow<MockRuntime>,
    queries: &[&str],
) -> (Vec<serde_json::Value>, f64) {
    timed(|| {
        queries
            .iter()
            .map(|query| {
                call(
                    webview,
                    "search_words",
                    serde_json::json!({ "query": query }),
                )
            })
            .collect()
    })
}

fn prefetch_reference_pass(webview: &tauri::WebviewWindow<MockRuntime>, queries: &[&str]) -> f64 {
    let (_, elapsed) = timed(|| {
        for query in queries {
            assert_eq!(
                call(
                    webview,
                    "prefetch_reference",
                    serde_json::json!({ "query": query }),
                ),
                serde_json::Value::Bool(true),
                "prefetch should complete for {query}"
            );
        }
    });
    elapsed
}

fn selection_pass(
    webview: &tauri::WebviewWindow<MockRuntime>,
    tokens: &[serde_json::Value],
    indices: &[usize],
) -> (Vec<serde_json::Value>, f64) {
    timed(|| {
        indices
            .iter()
            .map(|index| {
                call(
                    webview,
                    "lookup_in_context",
                    serde_json::json!({ "tokens": tokens, "index": index }),
                )
            })
            .collect()
    })
}

#[test]
#[ignore = "measurement job: needs installed dictionaries and a release build"]
fn current_reference_and_analysis_latency() {
    let (owned, startup_ms) = timed(app);
    let webview =
        tauri::WebviewWindowBuilder::new(&owned, "warmup-benchmark", tauri::WebviewUrl::default())
            .build()
            .expect("build mock webview");

    let (study, study_first_ms) =
        timed(|| call(&webview, "study", serde_json::json!({ "text": PASSAGE })));
    let mut repeated = Vec::new();
    for _ in 0..5 {
        let (_, elapsed) =
            timed(|| call(&webview, "study", serde_json::json!({ "text": PASSAGE })));
        repeated.push(elapsed);
    }

    let tokens = study["analysis"]["tokens"]
        .as_array()
        .expect("study returns tokens");
    let model_tokens: Vec<sakatsu_core::model::Token> =
        serde_json::from_value(serde_json::Value::Array(tokens.clone()))
            .expect("study tokens match the public model");
    let indices: Vec<usize> = model_tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.is_word())
        .map(|(index, _)| index)
        .take(10)
        .collect();
    assert!(
        indices.len() >= 6,
        "benchmark passage produced too few words: {indices:?}"
    );

    let (first_selections, analysis_selection_first_pass_ms) =
        selection_pass(&webview, tokens, &indices);
    let (repeat_selections, analysis_selection_repeat_pass_ms) =
        selection_pass(&webview, tokens, &indices);
    assert_eq!(
        first_selections, repeat_selections,
        "selection answers changed"
    );

    let (first_japanese, reference_japanese_first_pass_ms) =
        search_pass(&webview, JAPANESE_QUERIES);
    let (repeat_japanese, reference_japanese_repeat_pass_ms) =
        search_pass(&webview, JAPANESE_QUERIES);
    assert_eq!(
        first_japanese, repeat_japanese,
        "Japanese search answers changed"
    );

    let (first_english, reference_english_first_pass_ms) = search_pass(&webview, ENGLISH_QUERIES);
    let (repeat_english, reference_english_repeat_pass_ms) = search_pass(&webview, ENGLISH_QUERIES);
    assert_eq!(
        first_english, repeat_english,
        "English search answers changed"
    );

    // A second application instance starts with empty session caches. Its prefetch calls model
    // work performed during the learner's pause, while the following foreground passes measure
    // the latency the learner actually feels. Both paths must return the exact baseline answers.
    let prefetched = app();
    let prefetched_webview = tauri::WebviewWindowBuilder::new(
        &prefetched,
        "warmup-prefetched-benchmark",
        tauri::WebviewUrl::default(),
    )
    .build()
    .expect("build prefetched mock webview");
    let prefetched_study = call(
        &prefetched_webview,
        "study",
        serde_json::json!({ "text": PASSAGE }),
    );
    let prefetched_tokens = prefetched_study["analysis"]["tokens"]
        .as_array()
        .expect("prefetched study returns tokens");

    let (_, analysis_prefetch_work_ms) = timed(|| {
        call(
            &prefetched_webview,
            "prefetch_analysis",
            serde_json::json!({ "tokens": prefetched_tokens, "indices": indices }),
        )
    });
    let (after_prefetch_selections, analysis_after_prefetch_ms) =
        selection_pass(&prefetched_webview, prefetched_tokens, &indices);
    assert_eq!(
        first_selections, after_prefetch_selections,
        "analysis prefetch changed the visible answers"
    );

    let reference_japanese_prefetch_work_ms =
        prefetch_reference_pass(&prefetched_webview, JAPANESE_QUERIES);
    let (after_prefetch_japanese, reference_japanese_after_prefetch_ms) =
        search_pass(&prefetched_webview, JAPANESE_QUERIES);
    assert_eq!(
        first_japanese, after_prefetch_japanese,
        "Japanese prefetch changed the visible answers"
    );

    let reference_english_prefetch_work_ms =
        prefetch_reference_pass(&prefetched_webview, ENGLISH_QUERIES);
    let (after_prefetch_english, reference_english_after_prefetch_ms) =
        search_pass(&prefetched_webview, ENGLISH_QUERIES);
    assert_eq!(
        first_english, after_prefetch_english,
        "English prefetch changed the visible answers"
    );

    let result = Measurements {
        startup_ms,
        study_first_ms,
        study_repeat_median_ms: median(repeated),
        analysis_selection_first_pass_ms,
        analysis_selection_repeat_pass_ms,
        analysis_prefetch_work_ms,
        analysis_after_prefetch_ms,
        reference_japanese_first_pass_ms,
        reference_japanese_repeat_pass_ms,
        reference_japanese_prefetch_work_ms,
        reference_japanese_after_prefetch_ms,
        reference_english_first_pass_ms,
        reference_english_repeat_pass_ms,
        reference_english_prefetch_work_ms,
        reference_english_after_prefetch_ms,
    };
    println!(
        "SAKATSU_WARMUP_BENCH {}",
        serde_json::to_string(&result).expect("serialize measurements")
    );
}

#[test]
#[ignore = "measurement job: needs a working local Ichiran sidecar"]
fn current_ichiran_exact_input_cache_latency() {
    // A passage rather than one word: Remembering intentionally persists word lookups but keeps
    // passages in memory only, so an earlier benchmark process cannot make the first call a hit.
    let analyzer = Remembering::new(IchiranAnalyzer::new());
    let passage = "昨日見つけた不思議な本を、今日は図書館でゆっくり読み続けています。";
    let (first, first_ms) = timed(|| analyzer.analyze(passage).expect("Ichiran answers"));
    let (second, second_ms) = timed(|| analyzer.analyze(passage).expect("cached answer"));
    assert_eq!(first, second, "the cache changed Ichiran's answer");
    println!(
        "SAKATSU_ICHIRAN_BENCH {}",
        serde_json::json!({
            "first_ms": first_ms,
            "repeat_ms": second_ms,
            "engine": analyzer.engine(),
        })
    );
}
