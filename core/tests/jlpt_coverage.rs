//! Fifty native sentences, ten per JLPT level, through both analyzers.
//!
//! ```text
//! cargo test -p sakatsu-core --test jlpt_coverage                       # Lindera
//! cargo test -p sakatsu-core --test jlpt_coverage -- --ignored --nocapture   # + Ichiran
//! ```
//!
//! # What this adds that the other grammar tests do not
//!
//! `grammar_coverage.rs` runs Lindera. `ichiran_coverage.rs` runs Ichiran. They use
//! *different sentences*, so nothing has ever asserted the two **agree** — which is the
//! entire promise of `match_text`: one rule, both segmentations. A rule alive under one
//! analyzer and silently dead under the other is the failure `ichiran_coverage.rs` was
//! written after, and until now the only thing standing between that and a shipped build
//! was someone remembering.
//!
//! It is also the first level-balanced measurement. The existing suite asserts N1: 5,
//! N2: 14, N3: 4, N4: 6, N5: 4 — good on N2 and thin exactly where a beginner reads.
//!
//! # The sentences are not mine
//!
//! See `common/sentences.rs`. They are native examples from the corpus Jitendex bundles,
//! chosen by reading their translations; the search that found them (substring) is a
//! different mechanism from the one under test (token runs with element constraints).
//!
//! # This is a measurement, not a gate
//!
//! Rules authored against one set of sentences will not all generalise to another, and
//! the useful output is *which* ones. So the reports print a table and the assertions sit
//! at a floor, not at perfection — a floor that should be raised as rules are fixed, and
//! that is honest about where it stands today.

mod common;
#[path = "common/sentences.rs"]
mod sentences;

use std::collections::BTreeMap;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer};
use sakatsu_core::grammar::{matcher, pack};
use sakatsu_core::model::Analysis;

use sentences::{NEGATIVES, SENTENCES};

const LEVELS: [&str; 5] = ["N5", "N4", "N3", "N2", "N1"];

/// The pattern ids the engine reports for a sentence.
fn fired(analysis: &Analysis) -> Vec<String> {
    matcher::find_across(&analysis.tokens, &pack::all())
        .iter()
        .map(|found| found.pattern_id.clone())
        .collect()
}

/// Whether the engine found this sentence's pattern — or its documented twin.
fn found_it(sentence: &sentences::Sentence, fired: &[String]) -> bool {
    fired.iter().any(|id| {
        id == sentence.pattern_id
            || (!sentence.also_accept.is_empty() && id == sentence.also_accept)
    })
}

fn lindera(analyzer: &LinderaAnalyzer, text: &str) -> Analysis {
    analyzer.analyze(text).expect("lindera analyses")
}

/// Per level: how many of the ten were found.
fn report(title: &str, hit: &BTreeMap<&str, Vec<&str>>) {
    println!("\n=== {title} ===");
    let mut total = 0;
    for level in LEVELS {
        let found = hit.get(level).map(Vec::len).unwrap_or(0);
        total += found;
        println!("  {level}  {found:2}/10");
    }
    println!("  total {total}/50");
}

/// Every sentence, under the analyzer that ships without a sidecar.
#[test]
fn the_suite_is_found_under_lindera() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    let mut hit: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut missed: Vec<(&sentences::Sentence, Vec<String>)> = Vec::new();

    for sentence in SENTENCES {
        let analysis = lindera(&analyzer, sentence.text);
        let found = fired(&analysis);
        if found_it(sentence, &found) {
            hit.entry(sentence.level)
                .or_default()
                .push(sentence.pattern_id);
        } else {
            missed.push((sentence, found));
        }
    }

    report("Lindera", &hit);
    if !missed.is_empty() {
        println!("\n  not found ({}):", missed.len());
        for (sentence, found) in &missed {
            println!("    {} {}", sentence.level, sentence.pattern_id);
            println!("      {}", sentence.text);
            println!("      {}", sentence.english);
            // What fired *instead* is the whole triage. Nothing at all means the rule did
            // not generalise; a near neighbour means a more specific rule won and the
            // learner still sees an explanation, which is a different problem entirely.
            if found.is_empty() {
                println!("      nothing fired");
            } else {
                println!("      fired instead: {}", found.join(", "));
            }
        }
    }

    let found = SENTENCES.len() - missed.len();
    assert!(
        found >= FLOOR_LINDERA,
        "only {found}/50 found under Lindera, floor is {FLOOR_LINDERA}"
    );
}

/// Where the suite stands, so a regression fails and progress is visible.
///
/// A ratchet, not a target — raise it as rules are fixed, never lower it to make a run go
/// green.
///
/// # The two it does not reach, and why neither is a rule to fix
///
/// `hanabira-n1-verb-ru-ga-hayai-ka` on 「山の麓に日が昇るが早いかただ一人山を登り始めた」.
/// Lindera segments the tail as `早い | かた | だ | 一 | 人山` — it has read 早いかた as a
/// word and swallowed the か the pattern needs. No rule can match across a segmentation
/// that put the boundary in the wrong place. It was kept on the guess that Ichiran might
/// not make the same mistake, and **the agreement test confirmed it: Ichiran finds this
/// one.** It is the whole argument for running both.
///
/// `hanabira-n1-you-mo-nai` on 「どうしようもないほど…」. Lindera gives
/// `どう | しよう | も | ない`, so ようもない would have to begin *inside* しよう, which
/// `project notes` §5 forbids and this engine will not do. Not firing is correct behaviour;
/// the poor judgement was mine in choosing the sentence, since どうしようもない is a fixed
/// expression rather than the productive construction the pattern describes. Neither
/// analyzer finds it, and that is the right answer.
const FLOOR_LINDERA: usize = 48;

/// Measured, not assumed.
///
/// An earlier version of this file asserted nothing here and said why: "no sidecar was
/// installed on the machine this was written on". **That was false.** It was never
/// checked — it was inferred from the tests carrying `#[ignore]`, which says what a test
/// *needs*, not what a machine *has*. Ichiran was installed the whole time and answers in
/// under a second, so these could have run from the day they were written.
///
/// The first real run found nine rules that worked under Lindera and were dead under
/// Ichiran — the analyzer the app *prefers* when it is present. Anyone who followed the
/// setup guide was getting nine fewer grammar points than someone who ignored it.
const FLOOR_ICHIRAN: usize = 48;

/// The number that matters.
///
/// Lindera and Ichiran can each reach 48 while agreeing on far fewer, and a rule that
/// works under only one is exactly what the separate totals hide. This started at 39.
///
/// The three it does not reach are the two segmentation failures above — one from each
/// analyzer, in opposite directions — and the one that correctly fires under neither.
const FLOOR_BOTH: usize = 47;

/// The same fifty through the sidecar the app prefers when it is installed.
#[test]
#[ignore = "needs a working ichiran-cli"]
fn the_suite_is_found_under_ichiran() {
    let analyzer = IchiranAnalyzer::new();
    if analyzer.analyze("水").is_err() {
        println!("SKIP: ichiran-cli is not answering");
        return;
    }

    let mut hit: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut missed = Vec::new();
    for sentence in SENTENCES {
        let Ok(analysis) = analyzer.analyze(sentence.text) else {
            missed.push(sentence);
            continue;
        };
        if found_it(sentence, &fired(&analysis)) {
            hit.entry(sentence.level)
                .or_default()
                .push(sentence.pattern_id);
        } else {
            missed.push(sentence);
        }
    }

    report("Ichiran", &hit);
    // Named, as the Lindera half names its misses: a count alone says something regressed and
    // not which rule took the sentence instead.
    for sentence in &missed {
        let instead = analyzer
            .analyze(sentence.text)
            .map(|a| fired(&a))
            .unwrap_or_default();
        println!(
            "  missed {} {}\n    {}\n    fired instead: {instead:?}",
            sentence.level, sentence.pattern_id, sentence.text
        );
    }
    let found = SENTENCES.len() - missed.len();
    assert!(
        found >= FLOOR_ICHIRAN,
        "only {found}/50 found under Ichiran, floor is {FLOOR_ICHIRAN}"
    );
}

/// **The test that did not exist.**
///
/// A rule must behave the same whichever analyzer segmented the sentence. Where the two
/// disagree, the rule is written against one segmentation and is dead under the other —
/// and since the app prefers Ichiran but falls back to Lindera, *which* users are affected
/// depends on whether they installed a sidecar.
#[test]
#[ignore = "needs a working ichiran-cli"]
fn the_two_analyzers_agree_on_every_sentence() {
    let ichiran = IchiranAnalyzer::new();
    if ichiran.analyze("水").is_err() {
        println!("SKIP: ichiran-cli is not answering");
        return;
    }
    let lindera_analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    let mut both = 0;
    let mut only_lindera = Vec::new();
    let mut only_ichiran = Vec::new();
    let mut neither = 0;

    for sentence in SENTENCES {
        let under_lindera = found_it(sentence, &fired(&lindera(&lindera_analyzer, sentence.text)));
        let under_ichiran = ichiran
            .analyze(sentence.text)
            .map(|analysis| found_it(sentence, &fired(&analysis)))
            .unwrap_or(false);

        match (under_lindera, under_ichiran) {
            (true, true) => both += 1,
            (true, false) => only_lindera.push(sentence),
            (false, true) => only_ichiran.push(sentence),
            (false, false) => neither += 1,
        }
    }

    println!("\n=== agreement ===");
    println!("  both analyzers   {both}");
    println!("  Lindera only     {}", only_lindera.len());
    println!("  Ichiran only     {}", only_ichiran.len());
    println!("  neither          {neither}");

    for (label, group) in [
        ("Lindera only", &only_lindera),
        ("Ichiran only", &only_ichiran),
    ] {
        if group.is_empty() {
            continue;
        }
        println!("\n  {label} — these rules depend on the segmentation:");
        for sentence in group {
            println!("    {} {}", sentence.level, sentence.pattern_id);
            println!("      {}", sentence.text);
        }
    }

    assert!(
        both >= FLOOR_BOTH,
        "only {both}/50 found under both analyzers, floor is {FLOOR_BOTH}"
    );
}

/// The false friends: a pattern's surface present, its grammar absent.
///
/// Every one of these was found by the same substring search that found the positives,
/// which is what makes them fair — they are the mistakes the naive method makes, not
/// examples invented to be easy.
#[test]
fn a_surface_without_its_grammar_stays_silent() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    let mut wrong = Vec::new();
    for negative in NEGATIVES {
        let analysis = lindera(&analyzer, negative.text);
        if fired(&analysis).iter().any(|id| id == negative.pattern_id) {
            wrong.push(negative);
        }
    }

    if !wrong.is_empty() {
        println!("\n=== rules firing on a surface that is not their grammar ===");
        for negative in &wrong {
            println!("  {}", negative.pattern_id);
            println!("    {}", negative.text);
            println!("    {}", negative.english);
            println!("    why it must not fire: {}", negative.why);
        }
    }

    assert!(
        wrong.is_empty(),
        "{} rule(s) matched a surface without their grammar — see above",
        wrong.len()
    );
}

/// The suite must stay held out.
///
/// It is today — the overlap between the corpus and the packs' own examples was measured
/// at 1 sentence in 3,320, and that one is not used. This makes a future addition copied
/// from a pack entry fail rather than quietly turn the suite circular, which is the kind
/// of rot nobody notices because everything goes green.
#[test]
fn no_sentence_is_one_the_rules_were_written_from() {
    let mut pack_examples = Vec::new();
    for bundled in pack::all() {
        for entry in &bundled.patterns {
            for example in &entry.examples {
                pack_examples.push(example.japanese.trim().to_string());
            }
        }
    }

    for sentence in SENTENCES {
        assert!(
            !pack_examples.iter().any(|example| example == sentence.text),
            "{} is one of the pack's own examples — the suite must be held out",
            sentence.pattern_id
        );
    }
}

/// Ten per level, and ten *different* patterns, or the suite measures one rule ten ways.
#[test]
fn the_suite_is_balanced_and_covers_distinct_patterns() {
    for level in LEVELS {
        let at_level: Vec<&sentences::Sentence> =
            SENTENCES.iter().filter(|s| s.level == level).collect();
        assert_eq!(at_level.len(), 10, "{level} should have ten sentences");

        let mut ids: Vec<&str> = at_level.iter().map(|s| s.pattern_id).collect();
        ids.sort_unstable();
        let distinct = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), distinct, "{level} repeats a pattern id");
    }
}
