//! The pooled corpus and FuguMT's translation of it, written out for `scripts/mt-metrics/`.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test translation_export -- --ignored --nocapture --test-threads=1
//! ```
//!
//! COMET and MetricX are Python models, so the scoring lives there and this side only produces
//! what they read: `target/evaluation/mt/pooled.tsv` (index, Japanese, human English) and
//! `target/evaluation/mt/candidates/fugumt.tsv` (index, translation), both ignored by git. FuguMT
//! runs in-process through the same `Translator` the app calls, from `SAKATSU_MT_DIR` or the
//! profile's installed copy (read, never written).
//!
//! It translates the corpus twice from a cold engine and counts identical outputs, because the
//! app's translation memo assumes the same input gives the same output — FuguMT decodes greedily,
//! and this checks rather than assumes it.

mod common;

use std::io::Write;
use std::time::Instant;

use sakatsu_core::translate::{FuguMt, Translator};

/// The size of the pooled tier `local documentation` reports on.
const POOLED: usize = 300;

fn model_dir() -> std::path::PathBuf {
    std::env::var_os("SAKATSU_MT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"))
                .join("com.sakatsu.app")
                .join("models")
                .join("fugumt-ja-en")
        })
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
#[ignore = "needs the FuguMT model and the Jitendex archive; use --release"]
fn the_pooled_corpus_and_fugumt_are_written_for_the_metric_scripts() {
    let corpus = common::pooled_corpus(POOLED);
    assert_eq!(corpus.len(), POOLED, "the pooled corpus came up short");

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/evaluation/mt");
    std::fs::create_dir_all(out.join("candidates")).expect("create the evaluation directory");
    let mut pooled = std::fs::File::create(out.join("pooled.tsv")).expect("write pooled.tsv");
    for (index, (japanese, english)) in corpus.iter().enumerate() {
        writeln!(
            pooled,
            "{index}\t{}\t{}",
            one_line(japanese),
            one_line(english)
        )
        .expect("write");
    }

    let dir = model_dir();
    assert!(
        FuguMt::is_present(&dir),
        "{}: FuguMT is not installed there",
        dir.display()
    );
    let mut passes: Vec<Vec<String>> = Vec::new();
    let mut seconds = Vec::new();
    for _ in 0..2 {
        // A fresh engine each pass: nothing carried over from the one before.
        let engine = FuguMt::load(&dir).expect("load FuguMT");
        let mut rows = Vec::new();
        for (japanese, _) in &corpus {
            let started = Instant::now();
            rows.push(one_line(&engine.translate(japanese).expect("translate")));
            seconds.push(started.elapsed().as_secs_f64());
        }
        passes.push(rows);
    }

    let mut file =
        std::fs::File::create(out.join("candidates/fugumt.tsv")).expect("write fugumt.tsv");
    for (index, text) in passes[0].iter().enumerate() {
        writeln!(file, "{index}\t{text}").expect("write");
    }
    let identical = passes[0]
        .iter()
        .zip(&passes[1])
        .filter(|(a, b)| a == b)
        .count();
    let empty = passes[0].iter().filter(|t| t.is_empty()).count();
    seconds.sort_by(f64::total_cmp);
    let mean = seconds.iter().sum::<f64>() / seconds.len() as f64;
    let median = seconds[seconds.len() / 2];
    let p95 = seconds[(seconds.len() - 1) * 95 / 100];
    let run = format!(
        "{{\"label\": \"fugumt\", \"sentences\": {POOLED}, \"passes\": 2, \
         \"identical_to_first_pass\": [{identical}], \"empty\": {empty}, \
         \"latency_seconds\": {{\"mean\": {mean:.4}, \"median\": {median:.4}, \"p95\": {p95:.4}}}}}\n"
    );
    std::fs::write(out.join("candidates/fugumt.run.json"), &run).expect("write the run record");
    println!("{run}");
    println!("written: {}", out.display());
    assert_eq!(empty, 0, "FuguMT returned nothing for {empty} sentences");
}
