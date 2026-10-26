//! Hand-segmented captures, and the rule they are held to.
//!
//! ```text
//! cargo test -p sakatsu-core --test segmentation_gold -- --ignored --nocapture
//! ```
//!
//! # Why annotate by hand at all
//!
//! Every other gold in this repository is somebody else's register. UD_Japanese-GSD is news and
//! blogs; KWDLC is web documents. This app reads manga panels and game dialogue, and the measured
//! difference is not small — boundary conflict with Ichiran runs 2.1% on Tatoeba sentences and
//! 6.3% on the captures. Nothing borrowed measures that, so a little of it is annotated here.
//!
//! It is deliberately small and honestly labelled. `.tok.mine` is a draft nobody has reviewed,
//! the same standing `.txt.mine` has beside it, and `corpus/` is gitignored because the images
//! are copyrighted (`project notes` §8).
//!
//! # The rule, and why it is checkable
//!
//! `project notes` §4a says a token is the longest span the dictionary can answer for. That makes
//! hand annotation verifiable rather than a matter of taste: every token must reach an entry, by
//! its surface or by its dictionary form. This file is that check, and it caught four tokens on
//! the first pass — ところ, ある, ひどく, はず — where the kana spelling is not a Jitendex *term*
//! because the headword is 所, 有る, 酷い and 筈. They resolve by reading, which is how the
//! vocabulary panel finds them too, and that is the sense of "can answer" this asserts.
//!
//! **What this cannot do** is check that the segmentation is *good* — only that no token is a
//! span the dictionary is silent about. A rule this test would pass and a reader would not is
//! still possible, which is why the files say `.mine`.

mod common;
#[path = "common/runner.rs"]
mod runner;

use std::collections::{BTreeMap, BTreeSet};

use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::contains_japanese;

use runner::{Outcome, Profile, Requirement, Run, Stage};

fn corpus_dir() -> std::path::PathBuf {
    common::corpus_dir()
}

#[test]
#[ignore = "needs the corpus and the dictionaries"]
fn every_hand_segmented_token_is_a_word_the_dictionary_knows() {
    let dictionaries: Vec<Dictionary> =
        Dictionary::open(&common::dictionaries_dir().join("jitendex"))
            .into_iter()
            .collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no Jitendex — run scripts/fetch-dictionaries.ps1");
        return;
    }
    let Ok(entries) = std::fs::read_dir(corpus_dir()) else {
        eprintln!("SKIP: no corpus at {}", corpus_dir().display());
        return;
    };
    let analyzer = common::built_in().expect("the dictionary is compiled in");

    // Any hit, not an exact term match. The vocabulary panel is not empty when the dictionary
    // answers by reading, and "the dictionary can answer for this span" is what §4a asks.
    let resolves = |query: &str| {
        dictionaries.iter().any(|dictionary| {
            dictionary
                .lookup(query)
                .map(|h| !h.is_empty())
                .unwrap_or(false)
        })
    };

    let mut files = 0usize;
    let mut checked = 0usize;
    let mut unresolved: Vec<String> = Vec::new();

    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if !path.to_string_lossy().ends_with(".tok.mine") {
            continue;
        }
        let Ok(line) = std::fs::read_to_string(&path) else {
            continue;
        };
        files += 1;
        for token in line.trim().split('|') {
            if !contains_japanese(token) {
                continue;
            }
            checked += 1;
            if resolves(token) {
                continue;
            }
            // Inflected: the app asks about the dictionary form, so this does too.
            let lemma = analyzer
                .analyze(token)
                .ok()
                .and_then(|analysis| analysis.tokens.first().map(|t| t.dict_form.clone()))
                .unwrap_or_default();
            if lemma.is_empty() || !resolves(&lemma) {
                unresolved.push(format!("{token} (dictionary form {lemma:?})"));
            }
        }
    }

    if files == 0 {
        eprintln!(
            "SKIP: no .tok.mine segmentations in {}",
            corpus_dir().display()
        );
        return;
    }
    println!("{checked} Japanese tokens over {files} hand-segmented captures");
    assert!(
        unresolved.is_empty(),
        "these tokens are spans no dictionary can answer for, which §4a says is not a token: {unresolved:#?}"
    );
}

/// The analyzer against the hand segmentation, in **this app's own register**.
///
/// ```text
/// cargo test --release -p sakatsu-core --test segmentation_gold -- --ignored --nocapture against_the_hand
/// ```
///
/// # Why this is the measurement everything else has been standing in for
///
/// Every accuracy figure this project steers by is borrowed register. UD_Japanese-GSD is
/// newspapers; KWDLC is web documents. The disagreement profiles are visibly different — である
/// and ので dominate what the analyzer gets wrong on GSD, while じゃない and んだ dominate the
/// casual corpus — so a rule validated there is validated as a *mechanism*, not as fit for manga.
///
/// The test above checks the annotation is *valid*: every hand-written token reaches a dictionary
/// entry, which is what `project notes` §4a makes checkable. It never asks whether the analyzer
/// **agrees** with it. This does, and that is the only correctness number here measured on the
/// text this app exists to read.
///
/// # It is small, and the number must be read as small
///
/// Seventeen captures is not a corpus. A boundary F1 from it has an interval wide enough that
/// only large differences mean anything, and no rule should be adopted or rejected on this alone.
/// What it *can* do is contradict a borrowed number — if GSD says a change helps and this says it
/// hurts, the borrowed one is the one describing somebody else's Japanese.
///
/// Growing it is cheap and is the highest-value work available: one `.tok.mine` file per capture,
/// tokens separated by `|`, and the test above will tell you if a token you wrote is not a word.
///
/// # The pipeline, not the analyzer
///
/// Scored through `merge_compounds` and `merge_names` because that is what a reader sees. Measured
/// on GSD, skipping them understated long-unit F1 by 2.7 points.
#[test]
#[ignore = "needs the corpus and the dictionaries"]
fn the_analyzers_against_the_hand_segmented_captures() {
    // Shared with the application, and with every other harness: `Dictionary::discover` decides
    // what counts and in what order. Opening jitendex by name scored a different set than the app
    // uses and could not see a generation-published install at all.
    let dictionaries: Vec<Dictionary> = common::installed_dictionaries();
    // **This is a gate, so absent data fails rather than passes.** A skip here prints nothing
    // alarming and returns green, which reads exactly like "the floor held" to anyone scanning a
    // log — and the floor cannot hold over zero captures.
    let entries = std::fs::read_dir(corpus_dir())
        .unwrap_or_else(|error| panic!("no corpus at {}: {error}", corpus_dir().display()));

    // (text, gold spans) per capture. The text is the tokens run together, so the spans are exact
    // by construction — the same reason `common/gold.rs` rebuilds offsets rather than trusting a
    // file to carry them.
    let mut captures: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if !path.to_string_lossy().ends_with(".tok.mine") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut text = String::new();
        let mut spans = Vec::new();
        for token in body.trim().split('|') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let start = text.len();
            text.push_str(token);
            spans.push((start, text.len()));
        }
        if !spans.is_empty() {
            captures.push((text, spans));
        }
    }

    assert!(
        !captures.is_empty(),
        "no .tok.mine segmentations in {}",
        corpus_dir().display()
    );
    println!(
        "{} hand-segmented captures, {} gold tokens — small, and the interval is wide",
        captures.len(),
        captures.iter().map(|(_, spans)| spans.len()).sum::<usize>(),
    );

    let built_in = score_against_hand(
        "built-in (lindera)",
        &common::built_in().expect("compiled in"),
        &captures,
        &dictionaries,
    );

    // **The floor, on counts, and before Ichiran is consulted.** The built-in analyzer's gate does
    // not depend on a sidecar being installed: an absent Ichiran leaves the *comparison* incomplete
    // and says so, but it must never leave the gate unrun and the test green.
    //
    // 118 of 120 is where this stood when the three colloquial compositions landed, and none of
    // them moved it. Unlike the news floor there is no spent exception here — any decrease is a
    // decrease. These annotations are draft rather than reviewed gold (see the header), so this
    // detects change rather than certifying accuracy, which is the most it can honestly do.
    const CAPTURE_FLOOR: usize = 118;
    let (matched, gold_total) = built_in;
    assert_eq!(
        gold_total, 120,
        "the hand-segmented set changed size; this floor describes a different denominator"
    );
    assert!(
        matched >= CAPTURE_FLOOR,
        "boundary matches on the captures fell to {matched} from a floor of {CAPTURE_FLOOR}"
    );

    // The run's identity and its case accounting, reconciled against a manifest declared before
    // anything was evaluated. Without this the gate reports a rate and nothing about what it ran
    // against or whether it reached every capture.
    let ichiran = sakatsu_core::IchiranAnalyzer::new();
    let ichiran_answers = ichiran.analyze("テスト").is_ok();

    let manifest: Vec<String> = (0..captures.len())
        .map(|index| format!("capture-{index:03}"))
        .collect();
    let mut run = Run::new(Profile::InstalledResources, manifest.clone())
        .engine("lindera")
        .needs(Requirement::required(
            "dictionaries",
            !dictionaries.is_empty(),
        ))
        .needs(Requirement::required(
            "hand-segmented captures",
            !captures.is_empty(),
        ))
        .needs(Requirement::optional("ichiran", ichiran_answers).because(
            "the WSL sidecar did not answer, so the cross-analyzer comparison was not made; \
             the built-in gate above did run and held",
        ));
    for dictionary in &dictionaries {
        run = run.dictionary(format!(
            "{} ({} entries; sha256 {})",
            dictionary.meta().title,
            dictionary.meta().entry_count,
            dictionary
                .content_fingerprint()
                .expect("resource content identity is required")
        ));
    }

    let built_in_analyzer = common::built_in().expect("compiled in");
    for (index, (text, gold)) in captures.iter().enumerate() {
        let id = format!("capture-{index:03}");
        // A case that panics must fail the run rather than leave the denominator, so each one is
        // caught: surviving a bad case is not the same as forgiving it.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut analysis = built_in_analyzer.analyze(text)?;
            analysis.tokens = common::through_pipeline(&analysis.tokens, &dictionaries);
            let produced: BTreeSet<(usize, usize)> = analysis
                .tokens
                .iter()
                .filter_map(|token| Some((token.start?, token.end?)))
                .collect();
            let missed: Vec<&str> = gold
                .iter()
                .filter(|span| !produced.contains(span))
                .map(|span| &text[span.0..span.1])
                .collect();
            Ok::<_, sakatsu_core::AnalyzeError>(missed.join(", "))
        }));
        run.record(
            &id,
            match outcome {
                Err(_) => Outcome::incomplete("the case panicked"),
                Ok(Err(error)) => Outcome::incomplete(format!("the analyzer failed: {error}")),
                Ok(Ok(missed)) if missed.is_empty() => Outcome::Passed,
                // Attribution is not established per span here, and saying "analyzer" would be a
                // guess: the joins run between the analyzer and this comparison.
                Ok(Ok(missed)) => {
                    Outcome::failed(Stage::Unknown, format!("gold spans not produced: {missed}"))
                }
            },
        );
    }
    // **Accounting, not the verdict.** The floor above is the gate — these annotations are draft,
    // so an aggregate is the most that can be claimed honestly, and pinning which two tokens may
    // be missed would over-claim. What the floor cannot see is whether every capture was reached
    // at all: a rate over a corpus that quietly shrank looks exactly like a rate that held.
    run.accounted_for().expect_success();

    if !ichiran_answers {
        return;
    }
    score_against_hand("ichiran", &ichiran, &captures, &dictionaries);
    head_to_head(&ichiran, &captures, &dictionaries);
}

/// The gold tokens one analyzer finds and the other does not, both ways round.
///
/// **A pair of F1 scores cannot answer "what does the sidecar do better".** Two analyzers can post
/// the same number by being right about entirely different things, and the work that follows from
/// knowing which is not the work that follows from knowing how far apart they are. This names the
/// spans.
///
/// Read the two columns differently. Ichiran's wins are a **list of things to fix**, because the
/// built-in analyzer is the default and every one of them is a word a reader currently sees cut in
/// half. The built-in analyzer's wins are a reminder that the sidecar is not a ceiling — it splits
/// 加藤 into 加 and 藤 — and they are why retiring it as a *reference* would be a mistake even
/// after it stops being the default.
fn head_to_head(
    ichiran: &dyn Analyzer,
    captures: &[(String, Vec<(usize, usize)>)],
    dictionaries: &[Dictionary],
) {
    let lindera = common::built_in().expect("compiled in");
    let mut ichiran_only: BTreeMap<String, usize> = BTreeMap::new();
    let mut lindera_only: BTreeMap<String, usize> = BTreeMap::new();

    for (text, gold) in captures {
        let spans = |analyzer: &dyn Analyzer| -> BTreeSet<(usize, usize)> {
            let Ok(mut analysis) = analyzer.analyze(text) else {
                return BTreeSet::new();
            };
            analysis.tokens = common::through_pipeline(&analysis.tokens, dictionaries);
            analysis
                .tokens
                .iter()
                .filter_map(|token| Some((token.start?, token.end?)))
                .collect()
        };
        let ours = spans(&lindera);
        let theirs = spans(ichiran);

        for span in gold {
            let word = &text[span.0..span.1];
            match (ours.contains(span), theirs.contains(span)) {
                (false, true) => *ichiran_only.entry(word.to_string()).or_default() += 1,
                (true, false) => *lindera_only.entry(word.to_string()).or_default() += 1,
                _ => {}
            }
        }
    }

    let report = |title: &str, rows: &BTreeMap<String, usize>| {
        let mut sorted: Vec<_> = rows.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        println!(
            "
=== {title} — {} distinct, {} occurrences ===",
            sorted.len(),
            sorted.iter().map(|(_, count)| **count).sum::<usize>()
        );
        for (word, count) in sorted.iter().take(30) {
            println!("  {count:>3}x  {word}");
        }
    };
    report(
        "gold words ICHIRAN finds and the built-in one splits",
        &ichiran_only,
    );
    report(
        "gold words the BUILT-IN one finds and Ichiran splits",
        &lindera_only,
    );
}

fn score_against_hand(
    name: &str,
    analyzer: &dyn Analyzer,
    captures: &[(String, Vec<(usize, usize)>)],
    dictionaries: &[Dictionary],
) -> (usize, usize) {
    let (mut gold_total, mut produced_total, mut matched) = (0usize, 0usize, 0usize);
    let mut differences: Vec<String> = Vec::new();

    for (text, gold) in captures {
        let Ok(mut analysis) = analyzer.analyze(text) else {
            continue;
        };
        analysis.tokens = common::through_pipeline(&analysis.tokens, dictionaries);
        let produced: Vec<(usize, usize)> = analysis
            .tokens
            .iter()
            .filter_map(|token| Some((token.start?, token.end?)))
            .collect();

        gold_total += gold.len();
        produced_total += produced.len();
        matched += produced.iter().filter(|span| gold.contains(span)).count();

        if produced.len() != gold.len() || !produced.iter().all(|span| gold.contains(span)) {
            let ours: Vec<&str> = analysis
                .tokens
                .iter()
                .map(|token| token.surface.as_str())
                .collect();
            let theirs: Vec<&str> = gold
                .iter()
                .map(|(start, end)| &text[*start..*end])
                .collect();
            differences.push(format!(
                "  hand: {}\n  ours: {}",
                theirs.join(" | "),
                ours.join(" | ")
            ));
        }
    }

    let precision = matched as f64 / produced_total.max(1) as f64 * 100.0;
    let recall = matched as f64 / gold_total.max(1) as f64 * 100.0;
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    println!("\n=== {name} against the hand segmentation ===");
    println!("  gold {gold_total}   produced {produced_total}   matched {matched}");
    println!("  P {precision:.1}%   R {recall:.1}%   F1 {f1:.1}");

    // **Printed in full, because seventeen captures is few enough to read.** A rate this small is
    // only worth having if the disagreements behind it are looked at, and at this size they can be.
    println!("  captures that differ: {}", differences.len());
    for difference in differences.iter().take(12) {
        println!("{difference}");
    }
    (matched, gold_total)
}
