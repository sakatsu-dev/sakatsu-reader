//! Does beam search actually degenerate on this model, or did it degenerate in Python?
//!
//! ```text
//! cargo test --release -p sakatsu-core --test beam_experiment -- --ignored --nocapture
//! ```
//!
//! `--release` is not optional here: a debug build spends longer on this than anyone will wait.
//!
//! # The claim that was tested
//!
//! `project notes` §7 forbids beam search, and records what it saw:
//!
//! > Beam search degenerates badly on this model **under transformers 5.x**
//! > (水を飲む → "DRINK AWN THIS WAN THROUGH THROUED THROUED…").
//!
//! Three things made that look worth re-testing rather than simply obeying: it happened in
//! **Python, through a library**, never in this Rust/ONNX decoder; the output did not look like
//! beam degeneration, which usually gets blander and repetitive rather than emitting phonetic
//! all-caps; and the model's own `generation_config.json` asks for **12 beams**, written by
//! transformers 4.31 against a failure seen under 5.x.
//!
//! # The answer, measured 2026-08-18: §7 was right and that reasoning was wrong
//!
//! Run in this decoder, beam search reproduces the recorded failure almost character for
//! character:
//!
//! ```text
//! recorded, transformers 5.x   水を飲む → "DRINK AWN THIS WAN THROUGH THROUED THROUED…"
//! measured, this Rust/ONNX     水を飲む → "DRINK AWN THIS WAN THROUGH THROUGH THROUED THROUED…"
//! ```
//!
//! Two independent implementations, in two languages, producing the same phonetic all-caps output
//! means this is a property of **the model**, not of a library version. Eleven of twelve outputs
//! were degenerate by the automated check and every one of the twelve differed from greedy.
//!
//! It is unusable on speed alone as well: on 水を飲む, greedy 0.03s, beam 4 **78s**, beam 12
//! **234s**. Some of that is this implementation running a hypothesis at a time with no cache, but
//! three orders of magnitude is not an optimisation problem.
//!
//! So this test **pins the finding** rather than proposing a change: it asserts that beam search
//! still degenerates. `FuguMt::translate` stays greedy, and §7 stands — now confirmed in a second
//! runtime rather than only recorded from the first.

use std::path::PathBuf;

use sakatsu_core::translate::{FuguMt, Translator};

fn model_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_MT_DIR") {
        return PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA").expect("APPDATA (or set SAKATSU_MT_DIR)");
    PathBuf::from(appdata)
        .join("com.sakatsu.app")
        .join("models")
        .join("fugumt-ja-en")
}

/// The same shape of check `fugumt_live.rs` applies: word salad repeats itself.
fn degenerate(english: &str) -> bool {
    let words: Vec<&str> = english.split_whitespace().collect();
    if words.len() <= 6 {
        return false;
    }
    let unique: std::collections::HashSet<&&str> = words.iter().collect();
    unique.len() * 3 <= words.len()
}

#[test]
#[ignore = "needs scripts/fetch-translation-model.ps1, and --release"]
fn beam_search_still_degenerates_on_this_model() {
    let engine = FuguMt::load(&model_dir()).expect("load the model");

    // The first is §7's own example, so the recorded failure has a direct comparison.
    let sources = [
        "水を飲む。",
        "気持ちが伝わってきます。",
        "先生ってほんと情けないですね。",
        "今日は寒いですね。",
        "友達と映画を見に行きました。",
        "心の中で否定しようとも汝は私による支配を求めている。",
    ];

    let mut salad = Vec::new();
    let mut clean = Vec::new();
    let mut changed = 0usize;

    for source in sources {
        let started = std::time::Instant::now();
        let greedy = engine.translate(source).expect("greedy");
        println!("\n  {source}");
        println!(
            "    greedy   {greedy:?}  ({:.2}s)",
            started.elapsed().as_secs_f64()
        );

        for beams in [4usize, 12] {
            let started = std::time::Instant::now();
            let beam = engine
                .translate_with_beams(source, beams)
                .expect("beam search");
            let elapsed = started.elapsed().as_secs_f64();
            // Truncated: a degenerate hypothesis runs to the token limit and printing it whole
            // buries every other line in the report.
            let shown: String = beam.chars().take(90).collect();
            println!("    beam {beams:<2}  {shown:?}…  ({elapsed:.1}s)");

            if beam != greedy {
                changed += 1;
            }
            if degenerate(&beam) {
                salad.push(format!("{source} at {beams}"));
            } else {
                clean.push(format!("{source} at {beams} → {beam:?}"));
            }
        }
    }

    println!(
        "\n  differed from greedy: {changed} of {}",
        sources.len() * 2
    );
    println!(
        "  degenerate:           {} of {}",
        salad.len(),
        sources.len() * 2
    );
    println!("  the readable ones, for the record:");
    for line in clean.iter().take(4) {
        println!("    {line}");
    }

    // **The finding, pinned — this asserts the failure, not a fix.**
    //
    // Beam search was measured to degenerate here, in a runtime sharing nothing with the Python
    // one where it was first seen, so the fact is about the model rather than about a library.
    // Should this ever stop being true, something real has changed — a new export, a corrected
    // tokenizer — and `project notes` §7 wants re-reading rather than this test deleting.
    assert!(
        !salad.is_empty(),
        "beam search no longer degenerates on this model. That contradicts project notes §7 and the \
         2026-08-18 measurement in local documentation, both taken when it did. Something \
         has changed — re-read them before assuming beam search is now safe."
    );
}
