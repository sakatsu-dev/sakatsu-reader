//! **Where Ichiran scores two readings identically, which rule gets the answer right?**
//!
//! ```text
//! cargo test -p sakatsu-core --test tie_audit -- --ignored --nocapture --test-threads=1
//! ```
//!
//! # What this audits
//!
//! Ichiran declines to choose when its top candidates score the same, and the adapter breaks the
//! tie with a second opinion from the embedded Lindera. It used to ask about the **bare word, with
//! no sentence around it**; three KWDLC losses were attributed to that tie-break
//! in the baseline evaluation, all the same single-kanji noun, and the hypothesis was
//! that the question lacked its context. This measured every tie, not the three — and it is what
//! the change to ask about the word **in its sentence first** (`ichiran::to_analysis`) rests on.
//!
//! For every exact-score tie in Ichiran's **raw** output, it records the tied candidates and what
//! each rule would choose, then scores each rule against the gold reading where one exists:
//!
//! | rule | chooses |
//! |---|---|
//! | `first-listed` | Ichiran's own order among the tied — what no tie-break would give |
//! | `served` | the converted token's reading: the tie-break as shipped (re-derived as `in-context-then-bare`, and checked) |
//! | `bare` | Lindera over the word alone — the tie-break before the change |
//! | `in-context` | Lindera over the whole sentence, at exactly this span and spelling; `first-listed` where it has no such token |
//! | `in-context-then-bare` | `in-context`, else `bare`, else `first-listed` |
//!
//! # The instrument is checked against production, and fails on any unexplained difference
//!
//! The shipped rule is re-derived here ([`simulate`]) and every tie is reconciled with what the
//! adapter actually served ([`reconcile`]):
//!
//! - **agrees** — the simulation is the production answer;
//! - **ungrounded** — the raw word has no source span, so neither the sentence nor the gold can
//!   be consulted and no served token can be located. Counted and listed by identifier, never
//!   silently skipped, and excluded from scoring for a stated reason;
//! - **unserved** — the tie has a span but production has no token there. The audit's grounding
//!   and the adapter's then disagree, so the comparison would be about different words;
//! - **disagrees** — production served something else.
//!
//! The last two are **unexplained** and fail the run: an audit that simulates a rule nobody runs
//! measures nothing. The always-run tests at the bottom are the controls that prove each is
//! detected.
//!
//! A rule's answer counts only when it is **one of the tied candidates**: the tie-break promotes a
//! candidate, it never invents a reading. Where gold is not among the tied candidates, no tie-break
//! can be right, and that is reported apart rather than charged to any rule.
//!
//! # Gold
//!
//! - **KWDLC**, the same development-side selection `standalone_reference.rs` makes (stride over a
//!   2,000 sample, first 300, reserved documents excluded before loading). A gold morpheme counts
//!   only at exactly the tie's span.
//! - **The heteronym fixture** (`common/heteronyms.rs`): Tatoeba sentences bundled by Jitendex,
//!   each filed under one JMdict sequence, so the dictionary itself says which reading the spelling
//!   has there. The spelling's first occurrence gives the span.
//!
//! # Licence
//!
//! KWDLC is consult-only. Sentences, readings and gold go only to the ignored private artifact
//! under `target/evaluation/`; what is printed carries identifiers, spans and counts.

mod common;

#[path = "common/heteronyms.rs"]
mod heteronyms;

use std::collections::BTreeMap;

use common::knp;
use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::model::Analysis;
use serde_json::json;

const RULES: &[&str] = &[
    "first-listed",
    "served",
    "bare",
    "in-context",
    "in-context-then-bare",
];

fn hiragana(text: &str) -> String {
    sakatsu_core::kana::to_hiragana(text)
}

/// Lindera's reading of a token at exactly this span **and spelling**, if it has one.
fn lindera_at(analysis: &Analysis, span: (usize, usize), surface: &str) -> Option<String> {
    analysis
        .tokens
        .iter()
        .find(|token| (token.start, token.end) == (Some(span.0), Some(span.1)))
        .filter(|token| token.surface == surface && !token.reading.is_empty())
        .map(|token| hiragana(&token.reading))
}

/// The bare-word question `ichiran::second_opinion` asks, re-derived: one token, spelled the
/// same, with a reading.
fn bare(lindera: &LinderaAnalyzer, surface: &str) -> Option<String> {
    let analysis = lindera.analyze(surface).ok()?;
    let [token] = analysis.tokens.as_slice() else {
        return None;
    };
    (token.surface == surface && !token.reading.is_empty()).then(|| hiragana(&token.reading))
}

fn reading_at(analysis: &Analysis, span: (usize, usize)) -> Option<String> {
    analysis
        .tokens
        .iter()
        .find(|token| (token.start, token.end) == (Some(span.0), Some(span.1)))
        .map(|token| hiragana(&token.reading))
}

/// The shipped rule: the reading in the sentence, else the bare word's, else Ichiran's order —
/// each counting only if it names a tied candidate.
fn simulate(tied: &[String], in_context: Option<&str>, bare: Option<&str>) -> String {
    [in_context, bare]
        .into_iter()
        .flatten()
        .find(|answer| tied.iter().any(|candidate| candidate == answer))
        .map(str::to_string)
        .unwrap_or_else(|| tied[0].clone())
}

/// How one tie's simulated answer relates to what production served.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reconciled {
    Agrees,
    Ungrounded,
    Unserved,
    Disagrees { served: String, simulated: String },
}

impl Reconciled {
    fn is_unexplained(&self) -> bool {
        matches!(self, Reconciled::Unserved | Reconciled::Disagrees { .. })
    }
}

fn reconcile(span: Option<(usize, usize)>, served: Option<&str>, simulated: &str) -> Reconciled {
    match (span, served) {
        (None, _) => Reconciled::Ungrounded,
        (Some(_), None) => Reconciled::Unserved,
        (Some(_), Some(served)) if served == simulated => Reconciled::Agrees,
        (Some(_), Some(served)) => Reconciled::Disagrees {
            served: served.to_string(),
            simulated: simulated.to_string(),
        },
    }
}

/// Every tie reconciled, and the verdict on the whole run.
#[derive(Debug, Default)]
struct Accounting {
    agrees: usize,
    ungrounded: Vec<String>,
    unexplained: Vec<String>,
}

impl Accounting {
    fn record(&mut self, id: &str, reconciled: &Reconciled) {
        match reconciled {
            Reconciled::Agrees => self.agrees += 1,
            Reconciled::Ungrounded => self.ungrounded.push(id.to_string()),
            unexplained => self.unexplained.push(format!("{id}: {unexplained:?}")),
        }
    }

    fn enforce(&self) -> Result<(), String> {
        if self.unexplained.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{} ties where the simulated rule is not what production served:\n  {}",
                self.unexplained.len(),
                self.unexplained.join("\n  ")
            ))
        }
    }
}

/// One sentence to audit, with whatever gold it carries.
struct Case {
    source: &'static str,
    id: String,
    text: String,
    /// Gold readings by exact byte span.
    gold: BTreeMap<(usize, usize), String>,
}

fn kwdlc_cases(evaluation: &std::path::Path) -> (Vec<Case>, String) {
    let corpus = std::env::var_os("SAKATSU_KWDLC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            common::dictionaries_dir()
                .parent()
                .map(|parent| parent.join("corpora").join("kwdlc"))
                .unwrap_or_default()
        })
        .join("knp");
    let (reserved, audit) = common::reference::reserved_documents(evaluation);
    assert!(
        !reserved.is_empty(),
        "the reservation must be readable, or this would score the pool it is meant to hold back; \
         {audit}"
    );
    // **The same selection `standalone_reference.rs` makes**, so these ties are the ties in the
    // population the §4 occurrences came from.
    let (all_files, walk_faults) = knp::collect(&corpus);
    let development: Vec<std::path::PathBuf> = all_files
        .iter()
        .filter(|path| {
            !reserved.contains(
                &path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .cloned()
        .collect();
    let sentences = knp::load_files(&development, walk_faults).unwrap_or_else(|damaged| {
        panic!("{damaged}");
    });
    let limit = 2000usize;
    let stride = sentences.len().div_ceil(limit).max(1);
    let cases = sentences
        .iter()
        .step_by(stride)
        .take(300)
        .map(|sentence| Case {
            source: "kwdlc",
            id: sentence.id(),
            text: sentence.text.clone(),
            gold: sentence
                .morphemes
                .iter()
                .map(|morpheme| ((morpheme.start, morpheme.end), hiragana(&morpheme.reading)))
                .collect(),
        })
        .collect();
    (
        cases,
        format!(
            "{} development sentences, stride {stride}; {audit}",
            sentences.len()
        ),
    )
}

fn heteronym_cases() -> Vec<Case> {
    heteronyms::HETERONYMS
        .iter()
        .enumerate()
        .filter_map(|(index, case)| {
            let start = case.text.find(case.term)?;
            Some(Case {
                source: "heteronym",
                id: format!("heteronym-{index:03}"),
                text: case.text.to_string(),
                gold: BTreeMap::from([((start, start + case.term.len()), hiragana(case.reading))]),
            })
        })
        .collect()
}

#[derive(Default)]
struct Tally {
    ties: usize,
    /// Ties at a span with a gold reading.
    judged: usize,
    /// …of which gold is one of the tied candidates, so some tie-break could be right.
    reachable: usize,
    /// Per rule: right among the judged.
    right: BTreeMap<&'static str, usize>,
    /// Every tie, reconciled with production.
    accounting: Accounting,
}

#[test]
#[ignore = "needs Ichiran, KWDLC (scripts/fetch-kwdlc.ps1) and the reservation file"]
fn every_exact_score_tie_is_scored_under_each_rule() {
    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let ichiran = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    ichiran.warm();
    assert!(
        ichiran.analyze("これって").is_ok(),
        "Ichiran must answer; a tie audit without it measures nothing"
    );
    let route = sakatsu_core::analyzer::ichiran::server::last_route();

    let (mut cases, kwdlc_note) = kwdlc_cases(&evaluation);
    let kwdlc_count = cases.len();
    cases.extend(heteronym_cases());

    let mut private = String::new();
    let mut tallies: BTreeMap<&'static str, Tally> = BTreeMap::new();
    let mut failed: Vec<String> = Vec::new();
    let mut losses: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();

    for case in &cases {
        let traced = match ichiran.analyze_traced(&case.text) {
            Ok(traced) => traced,
            Err(error) => {
                failed.push(format!("{} {}", case.id, error));
                continue;
            }
        };
        let Some(raw) = traced.raw.as_deref() else {
            failed.push(format!("{} no raw output", case.id));
            continue;
        };
        let words = match common::reference::raw_words(raw, &case.text) {
            Ok(words) => words,
            Err(error) => {
                failed.push(format!("{} {}", case.id, error));
                continue;
            }
        };
        let in_sentence = lindera.analyze(&case.text).ok();
        let tally = tallies.entry(case.source).or_default();

        for word in &words {
            let tied: Vec<String> = word.top_readings().into_iter().map(hiragana).collect();
            if tied.len() < 2 {
                continue;
            }
            tally.ties += 1;
            let first_listed = tied[0].clone();
            let bare_answer = bare(&lindera, &word.text);
            let grounded = word.start.zip(word.end);
            let context = grounded.and_then(|span| {
                in_sentence
                    .as_ref()
                    .and_then(|analysis| lindera_at(analysis, span, &word.text))
            });
            let served = grounded.and_then(|span| reading_at(&traced.analysis, span));
            let shipped = simulate(&tied, context.as_deref(), bare_answer.as_deref());
            let reconciled = reconcile(grounded, served.as_deref(), &shipped);
            let label = match grounded {
                Some((start, end)) => format!("{} [{start},{end}) {}", case.id, word.text),
                None => format!("{} (no span) {}", case.id, word.text),
            };
            tally.accounting.record(&label, &reconciled);
            // An ungrounded tie has no gold and no sentence reading to score: accounted above,
            // and scored nowhere.
            let Some((start, end)) = grounded else {
                continue;
            };
            let span = (start, end);
            let among = |answer: &Option<String>| answer.clone().filter(|a| tied.contains(a));
            let bare_rule = among(&bare_answer).unwrap_or_else(|| first_listed.clone());

            let choices: BTreeMap<&'static str, Option<String>> = BTreeMap::from([
                ("first-listed", Some(first_listed.clone())),
                ("served", served.clone()),
                ("bare", Some(bare_rule.clone())),
                (
                    "in-context",
                    Some(among(&context).unwrap_or_else(|| first_listed.clone())),
                ),
                ("in-context-then-bare", Some(shipped.clone())),
            ]);

            let gold = case.gold.get(&span).cloned();
            let mut verdicts = BTreeMap::new();
            if let Some(gold) = &gold {
                tally.judged += 1;
                let reachable = tied.contains(gold);
                tally.reachable += usize::from(reachable);
                for rule in RULES {
                    let right = choices[rule].as_deref() == Some(gold.as_str());
                    *tally.right.entry(rule).or_default() += usize::from(right);
                    verdicts.insert(*rule, right);
                    if *rule == "served" && !right {
                        losses.entry(case.source).or_default().push(format!(
                            "{} [{start},{end}) gold {} tied",
                            case.id,
                            if reachable { "among" } else { "not among" }
                        ));
                    }
                }
            }
            private.push_str(
                &json!({
                    "source": case.source, "id": case.id, "text": case.text,
                    "span": [start, end], "surface": word.text,
                    "tied": tied, "scores": word.alternatives,
                    "bare": bare_answer, "in_context": context,
                    "choices": choices, "gold": gold, "verdicts": verdicts,
                    "reconciled": format!("{reconciled:?}"),
                })
                .to_string(),
            );
            private.push('\n');
        }
    }

    let artifact = common::reference::publish(&evaluation, "tie-audit", "jsonl", &private)
        .unwrap_or_else(|error| panic!("the audit could not be published: {error}"));

    println!("\n=== exact-score ties, scored per rule ===");
    println!("  route        {}", route.name());
    println!("  kwdlc        {kwdlc_count} sentences ({kwdlc_note})");
    println!("  heteronyms   {} sentences", cases.len() - kwdlc_count);
    println!("  failed       {}", failed.len());
    for failure in &failed {
        println!("    {failure}");
    }
    for (source, tally) in &tallies {
        println!(
            "\n  [{source}] ties {} · with gold at the span {} · gold among the tied {}",
            tally.ties, tally.judged, tally.reachable
        );
        let accounting = &tally.accounting;
        println!(
            "    reconciled with production: {} agree · {} ungrounded (not comparable) · {} unexplained",
            accounting.agrees,
            accounting.ungrounded.len(),
            accounting.unexplained.len()
        );
        for id in &accounting.ungrounded {
            println!("      ungrounded: {id}");
        }
        for rule in RULES {
            println!(
                "    {rule:<22} {}/{}",
                tally.right.get(rule).copied().unwrap_or(0),
                tally.judged
            );
        }
        for loss in losses.get(source).into_iter().flatten() {
            println!("    served wrong: {loss}");
        }
    }
    println!(
        "\n  artifact     {} (private: holds the sentences)",
        artifact.display()
    );
    assert!(
        failed.is_empty(),
        "{} sentences were not analysed; the audit is incomplete",
        failed.len()
    );
    for (source, tally) in &tallies {
        let accounted = tally.accounting.agrees
            + tally.accounting.ungrounded.len()
            + tally.accounting.unexplained.len();
        assert_eq!(
            accounted, tally.ties,
            "[{source}] every tie is reconciled once"
        );
        if let Err(unexplained) = tally.accounting.enforce() {
            panic!("[{source}] {unexplained}");
        }
    }
}

// ---------------------------------------------------------------------------
// Controls for the instrument: each unexplained case is detected. These run in the ordinary suite.

fn readings(list: &[&str]) -> Vec<String> {
    list.iter().map(|reading| reading.to_string()).collect()
}

#[test]
fn the_simulated_rule_prefers_the_sentence_then_the_bare_word_then_ichiran() {
    let tied = readings(&["ほう", "かた"]);
    assert_eq!(simulate(&tied, Some("かた"), Some("ほう")), "かた");
    assert_eq!(simulate(&tied, None, Some("かた")), "かた");
    // An answer naming nothing tied has no vote, and the next one is asked.
    assert_eq!(simulate(&tied, Some("がた"), Some("かた")), "かた");
    assert_eq!(simulate(&tied, Some("がた"), None), "ほう");
    assert_eq!(simulate(&tied, None, None), "ほう");
}

#[test]
fn a_served_reading_that_differs_from_the_simulation_fails_the_run() {
    let mut accounting = Accounting::default();
    accounting.record(
        "case [0,3) 方",
        &reconcile(Some((0, 3)), Some("ほう"), "かた"),
    );
    assert!(accounting.enforce().is_err());
}

#[test]
fn a_grounded_tie_production_did_not_serve_fails_the_run() {
    let mut accounting = Accounting::default();
    accounting.record("case [0,3) 方", &reconcile(Some((0, 3)), None, "かた"));
    assert!(accounting.enforce().is_err());
}

#[test]
fn an_ungrounded_tie_is_accounted_by_name_and_not_counted_as_agreement() {
    let mut accounting = Accounting::default();
    let reconciled = reconcile(None, None, "かた");
    assert_eq!(reconciled, Reconciled::Ungrounded);
    assert!(!reconciled.is_unexplained());
    accounting.record("case (no span) 方", &reconciled);
    assert_eq!(
        accounting.agrees, 0,
        "an ungrounded tie is not an agreement"
    );
    assert_eq!(accounting.ungrounded, ["case (no span) 方"]);
    assert!(accounting.enforce().is_ok());
}

#[test]
fn only_agreement_passes() {
    let mut accounting = Accounting::default();
    accounting.record(
        "case [0,3) 方",
        &reconcile(Some((0, 3)), Some("かた"), "かた"),
    );
    assert_eq!(accounting.agrees, 1);
    assert!(accounting.enforce().is_ok());
}
