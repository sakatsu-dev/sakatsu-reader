//! FuguMT against the real model.
//!
//! Ignored by default: needs `scripts/fetch-translation-model.ps1` (~300 MB). Run:
//!
//! ```text
//! cargo test -p sakatsu-core --test fugumt_live -- --ignored --nocapture
//! ```
//!
//! The assertions are deliberately loose. A 61M-parameter model does not produce one
//! canonical English sentence, and pinning exact output would make this a test of the
//! weights rather than of the pipeline. What must hold is that recognisable English
//! comes back, that it is not the input echoed, and that the pad token never leaks —
//! the failure modes §7 warns about.

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

#[test]
#[ignore = "needs scripts/fetch-translation-model.ps1"]
fn the_model_translates_japanese_into_english() {
    let dir = model_dir();
    assert!(
        FuguMt::is_present(&dir),
        "no model at {} — run scripts/fetch-translation-model.ps1",
        dir.display()
    );

    let started = std::time::Instant::now();
    let engine = FuguMt::load(&dir).expect("load the model");
    println!("loaded in {:.1}s", started.elapsed().as_secs_f64());
    assert_eq!(engine.engine(), "fugumt");

    for source in [
        "水を飲む。",
        "気持ちが伝わってきます。",
        "先生ってほんと情けないですね。",
    ] {
        let started = std::time::Instant::now();
        let english = engine.translate(source).expect("translate");
        println!(
            "{source}\n  → {english:?}  ({:.2}s)",
            started.elapsed().as_secs_f64()
        );

        assert!(!english.trim().is_empty(), "empty translation for {source}");
        // Latin letters, i.e. it produced English rather than echoing the Japanese.
        assert!(
            english.chars().any(|c| c.is_ascii_alphabetic()),
            "no English in {english:?}"
        );
        assert!(
            !english.contains('<'),
            "a special token leaked into {english:?}"
        );
        // The word-salad failure §7 warns about shows up as the same token forever.
        let words: Vec<&str> = english.split_whitespace().collect();
        if words.len() > 6 {
            let unique: std::collections::HashSet<&&str> = words.iter().collect();
            assert!(
                unique.len() * 3 > words.len(),
                "output looks degenerate: {english:?}"
            );
        }
    }
}

/// **What decoding actually costs, as the output gets longer.**
///
/// The decoder ignores the `present.*` cache tensors it emits, so every generated token re-runs
/// it over the whole prefix — quadratic in output length where a KV cache would be linear. Whether
/// that matters is not a matter of opinion: it depends on how long the outputs actually are for
/// the sentences this app sees, and that is what this prints.
///
/// It exists to answer one question — can a reader afford to translate a sentence *many times*,
/// once per variant, while someone is clicking — and to be the before/after if the cache is ever
/// wired up. Time per produced word is the number to watch: flat means the quadratic term is not
/// biting at these lengths, and rising means it is.
#[test]
#[ignore = "needs scripts/fetch-translation-model.ps1"]
fn decoding_cost_grows_with_the_length_of_what_it_writes() {
    let engine = FuguMt::load(&model_dir()).expect("load the model");

    // Real sentences, shortest first. The last is the kind of line this app is usually pointed at.
    let sources = [
        "水を飲む。",
        "今日は寒いですね。",
        "友達と映画を見に行きました。",
        "心の中で否定しようとも汝は私による支配を求めている。",
        "彼は毎朝六時に起きて、犬を散歩に連れて行ってから、会社まで電車で通っている。",
        "この問題については、来週の会議で詳しく議論することになると思いますが、         その前に資料を準備しておいたほうがいいでしょう。",
    ];

    println!(
        "
  chars   words out   seconds   ms/word"
    );
    for source in sources {
        // Warm: the first call through a session pays one-off costs that are not decoding.
        let _ = engine.translate(source);

        let started = std::time::Instant::now();
        let english = engine.translate(source).expect("translate");
        let elapsed = started.elapsed().as_secs_f64();
        let words = english.split_whitespace().count().max(1);
        println!(
            "  {:>5}   {:>9}   {:>7.3}   {:>7.1}",
            source.chars().count(),
            words,
            elapsed,
            elapsed * 1000.0 / words as f64
        );
        println!("        {english}");
    }
}

/// **Does a passage translated whole say less than its sentences translated one at a time?**
///
/// The two paths in the app disagree about what to send. The Translation disclosure hands over the
/// entire passage (`App.svelte` → `translate`, `text: passage`), while the comparison strip works a
/// sentence at a time (`translate::variants`, bounded by `model::sentence_bounds`). If a 61M-param
/// model compresses when given more to carry, the strip would read as the more detailed of the two
/// on exactly the same Japanese — which is a difference in *how it is asked*, not in the model.
///
/// # The answer, measured 2026-08-18: no, and the interesting result is the other way round
///
/// Length is a non-issue. Whole and per-sentence come back within a word or two of each other and
/// say the same things, so the strip is not getting better translations out of shorter inputs.
///
/// **What isolation costs is the subject.** Japanese drops it and the neighbouring sentences
/// supply it, so a sentence translated alone has to guess:
///
/// ```text
/// whole         "He gets up at six every morning. He takes his dog for a walk.
///                Then he goes to work by train."
/// per-sentence  "He gets up at six every morning."
///               "I take my dog for a walk."      ← 彼 was two sentences ago
///               "Then I go to work by train."    ← and the model reached for "I"
/// ```
///
/// That is inherent to the comparison strip rather than a bug in it: the ablation has to be
/// sentence-scoped or the diff is swamped by sentences that did not change. It is the price of
/// the feature, and this test is where it is written down.
///
/// Counting English words is the crude proxy for detail; the text underneath it is the evidence.
#[test]
#[ignore = "needs scripts/fetch-translation-model.ps1"]
fn a_passage_translated_whole_against_its_sentences_one_at_a_time() {
    let engine = FuguMt::load(&model_dir()).expect("load the model");

    // Multi-sentence passages, which is what a capture of a page actually looks like.
    let passages = [
        "家で食べる。友達と映画を見に行きました。今日は寒いですね。",
        "彼は毎朝六時に起きる。犬を散歩に連れて行く。それから会社まで電車で通っている。",
        "心の中で否定しようとも汝は私による支配を求めている。もう遅い。",
    ];

    for passage in passages {
        let whole = engine.translate(passage).expect("whole");

        // The same split the strip works from: Japanese terminators, keeping the terminator with
        // the sentence it closes.
        let mut sentences: Vec<String> = Vec::new();
        let mut current = String::new();
        for character in passage.chars() {
            current.push(character);
            if matches!(character, '。' | '！' | '？') {
                sentences.push(std::mem::take(&mut current));
            }
        }
        if !current.trim().is_empty() {
            sentences.push(current);
        }

        let parts: Vec<String> = sentences
            .iter()
            .map(|sentence| engine.translate(sentence).expect("sentence"))
            .collect();
        let joined = parts.join(" ");

        let count = |text: &str| text.split_whitespace().count();
        println!(
            "
  {passage}"
        );
        println!("    whole      ({:>2} words)  {whole:?}", count(&whole));
        println!("    per-sentence ({:>2} words)", count(&joined));
        for (sentence, english) in sentences.iter().zip(&parts) {
            println!("        {sentence}  →  {english:?}");
        }
    }
}

#[test]
#[ignore = "needs scripts/fetch-translation-model.ps1"]
fn empty_input_is_refused_before_any_inference() {
    let engine = FuguMt::load(&model_dir()).expect("load the model");
    assert!(engine.translate("   ").is_err());
}
