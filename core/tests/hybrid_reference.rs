//! Lindera, Ichiran and Hybrid against hand-annotated Japanese, through one scoring path.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test hybrid_reference -- --ignored --nocapture
//! ```
//!
//! # One pipeline boundary, because the existing harnesses disagree about it
//!
//! `ud_gold.rs` scores tokens **after** `common::through_pipeline`; `kwdlc_gold.rs` scores
//! `analyzer.analyze()` output **directly**. Both are defensible on their own terms and they are
//! not comparable with each other — one includes compound and name joining, the other does not.
//! Reusing both unchanged would have put a three-way comparison on two different stages.
//!
//! So every arm here scores the **finished production analysis, with the pipeline applied exactly
//! once**. That is also the only stage at which Hybrid exists at all: it selects between two
//! already-displayed analyses. The existing harnesses keep their own paths and their published
//! numbers remain valid **as historical baselines on their own boundary** — they are reported
//! beside these rows, never merged into them.
//!
//! # What each reference can and cannot settle
//!
//! | Reference | Boundaries | Readings |
//! |---|---|---|
//! | UD_Japanese-GSD | SUW and LUW, separately | **no** — its readings are UniDic's own answer, and UniDic is the built-in analyzer's dictionary |
//! | KWDLC | morpheme, 基本句, 文節, separately | **yes** — JUMAN lineage, related to neither engine's dictionary |
//!
//! SUW and LUW are different annotation granularities, not competing definitions of one correct
//! word. Their scores on the same sentences are correlated observations of the same analysis, not
//! independent evidence.
//!
//! # Licence
//!
//! KWDLC declares none; `project notes` §8 treats it as **consult, never import**. Nothing derived from
//! it is persisted here — the artifact carries counts and document identifiers, never a surface, a
//! reading or a boundary.

mod common;

#[path = "common/runner.rs"]
mod runner;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

#[allow(unused_imports)]
use common::knp::Morpheme;
use common::knp::{self, level};
use common::resume::{Identity, Resume, BUDGET};
use runner::{Outcome, Profile, Requirement, Run};
use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::{hybrid, Analysis};
use serde_json::json;
use sha2::{Digest, Sha256};

/// The boundary these rows are scored on. Part of the resume key: a changed scorer must not
/// silently reuse cases measured by the previous one.
const SCORING_METHOD: &str = "finished-production-analysis-v1";

/// Which analysis an arm contributes.
///
/// **Not `&dyn Analyzer`.** Hybrid is not an analyzer; it is a choice between two finished ones, so
/// the scorer takes the finished [`Analysis`] and each metric derives what it needs from it. A span
/// set would have been enough for boundaries and would have thrown away the readings KWDLC is the
/// only reference able to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Arm {
    Lindera,
    Ichiran,
    Hybrid,
}

impl Arm {
    fn name(self) -> &'static str {
        match self {
            Arm::Lindera => "lindera",
            Arm::Ichiran => "ichiran",
            Arm::Hybrid => "hybrid",
        }
    }

    fn all() -> [Arm; 3] {
        [Arm::Lindera, Arm::Ichiran, Arm::Hybrid]
    }
}

/// Precision, recall and F1 at one granularity.
#[derive(Default, Clone, Copy)]
struct Level {
    gold: usize,
    produced: usize,
    matched: usize,
}

impl Level {
    fn add(&mut self, gold: usize, produced: usize, matched: usize) {
        self.gold += gold;
        self.produced += produced;
        self.matched += matched;
    }

    fn precision(&self) -> f64 {
        if self.produced == 0 {
            0.0
        } else {
            self.matched as f64 / self.produced as f64
        }
    }

    fn recall(&self) -> f64 {
        if self.gold == 0 {
            0.0
        } else {
            self.matched as f64 / self.gold as f64
        }
    }

    fn f1(&self) -> f64 {
        let (p, r) = (self.precision(), self.recall());
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }

    fn describe(&self) -> serde_json::Value {
        json!({
            "gold": self.gold,
            "produced": self.produced,
            "matched": self.matched,
            "precision": self.precision(),
            "recall": self.recall(),
            "f1": self.f1(),
        })
    }
}

/// Readings, checked only where a span matched exactly — otherwise it is two accounts of different
/// words. The denominators are reported so "accuracy" cannot hide how little it covered.
#[derive(Default, Clone, Copy)]
struct Readings {
    aligned: usize,
    agreed: usize,
}

impl Readings {
    fn describe(&self, gold_total: usize) -> serde_json::Value {
        json!({
            "aligned_spans": self.aligned,
            "agreed": self.agreed,
            "accuracy_over_aligned": if self.aligned == 0 { 0.0 } else { self.agreed as f64 / self.aligned as f64 },
            "gold_morphemes": gold_total,
            "coverage_of_gold": if gold_total == 0 { 0.0 } else { self.aligned as f64 / gold_total as f64 },
            "note": "accuracy is over aligned spans only; coverage says how much of gold that was",
        })
    }
}

/// Everything one arm accumulated over one reference.
#[derive(Default)]
struct Scored {
    levels: BTreeMap<&'static str, Level>,
    readings: Readings,
    gold_morphemes: usize,
    /// Sentences where this arm produced nothing usable.
    failures: usize,
}

impl Scored {
    fn level(&mut self, name: &'static str) -> &mut Level {
        self.levels.entry(name).or_default()
    }
}

/// The schema of a persisted case record.
///
/// **Bumped whenever the fields a total is built from change.** A record without it — or with an
/// older one — is not reusable for metrics. It may faithfully say what a case's *outcome* was and
/// still be useless for a *total*, and a total assembled partly from records that never carried
/// counts is not the total it claims to be. Rejecting them costs a re-run; accepting them costs
/// the number.
const RECORD_SCHEMA: u64 = 2;

/// What one case contributed, in full, so a resumed invocation reports what an uninterrupted one
/// would.
///
/// The first version of this harness persisted `{"outcome": "passed"}` and nothing else, then
/// `continue`d past a resumed case without adding its spans to any arm. A fully resumed run would
/// have published zeros under a green verdict.
#[derive(Default, Clone, PartialEq, Eq, Debug)]
struct CaseMetrics {
    /// `(arm, level)` → gold, produced, matched.
    levels: BTreeMap<(String, String), (usize, usize, usize)>,
    /// arm → aligned spans, agreed readings.
    readings: BTreeMap<String, (usize, usize)>,
    /// arm → gold morphemes this case offered it.
    gold_morphemes: BTreeMap<String, usize>,
    /// Arms that produced no analysis at all.
    failures: Vec<String>,
    /// Why the case contributed to no arm, when it did not.
    excluded: Option<String>,
    /// Regions where the selector chose Ichiran's sequence over Lindera's.
    changed_regions: usize,
    /// Which named rule decided each of them.
    rule_changes: BTreeMap<String, usize>,
    /// No arm matched a single reference unit here.
    neither_matched: bool,
    /// arm → readings that went right→wrong and wrong→right against the Lindera baseline, over
    /// spans **all three arms aligned**. A net figure hides that a change can do both.
    reading_transitions: BTreeMap<String, (usize, usize)>,
    /// Reference units the selector's regions cut across, which no arm can be credited or blamed
    /// for on a per-region basis.
    reference_spans_crossing_regions: usize,
    /// Gold readings that left the comparison, and why.
    readings_unaligned: BTreeMap<String, usize>,
    /// `(rule, direction, level)` → reference units recovered, lost, and crossing the region.
    ///
    /// **A rule's benefit, not its frequency.** Recovered units the chosen sequence matches and
    /// the rejected one does not; lost is the reverse. Units straddling the region boundary are
    /// counted apart, because neither side can be credited or blamed for them here.
    rule_outcomes: BTreeMap<(String, String, String), (usize, usize, usize)>,
    /// arm → readings that differ from Lindera's **as strings**, split by whether the
    /// right/wrong classification moved.
    ///
    /// Transitions alone hide this: two different wrong readings are both wrong, so a
    /// right→wrong / wrong→right table calls them unchanged when the reader is shown something
    /// else entirely.
    reading_strings: BTreeMap<String, (usize, usize)>,
}

impl CaseMetrics {
    fn to_json(&self) -> serde_json::Value {
        json!({
            "schema": RECORD_SCHEMA,
            "levels": self.levels.iter().map(|((arm, level), (gold, produced, matched))| {
                json!({"arm": arm, "level": level, "gold": gold, "produced": produced, "matched": matched})
            }).collect::<Vec<_>>(),
            "readings": self.readings.iter().map(|(arm, (aligned, agreed))| {
                json!({"arm": arm, "aligned": aligned, "agreed": agreed})
            }).collect::<Vec<_>>(),
            "gold_morphemes": self.gold_morphemes,
            "failures": self.failures,
            "excluded": self.excluded,
            "changed_regions": self.changed_regions,
            "rule_changes": self.rule_changes,
            "neither_matched": self.neither_matched,
            "reading_transitions": self.reading_transitions.iter().map(|(arm, (worse, better))| {
                json!({"arm": arm, "became_wrong": worse, "became_right": better})
            }).collect::<Vec<_>>(),
            "reference_spans_crossing_regions": self.reference_spans_crossing_regions,
            "readings_unaligned": self.readings_unaligned,
            "rule_outcomes": self.rule_outcomes.iter().map(|((rule, direction, level), (recovered, lost, crossing))| {
                json!({"rule": rule, "direction": direction, "level": level,
                       "recovered": recovered, "lost": lost, "crossing": crossing})
            }).collect::<Vec<_>>(),
            "reading_strings": self.reading_strings.iter().map(|(arm, (differing_same_class, differing_total))| {
                json!({"arm": arm, "differing_same_classification": differing_same_class,
                       "differing_total": differing_total})
            }).collect::<Vec<_>>(),
        })
    }

    /// Read a persisted record back, **refusing anything that cannot supply a total**.
    fn from_json(record: &serde_json::Value) -> Option<CaseMetrics> {
        if record["schema"].as_u64() != Some(RECORD_SCHEMA) {
            return None;
        }
        let mut metrics = CaseMetrics::default();
        for row in record["levels"].as_array()? {
            metrics.levels.insert(
                (
                    row["arm"].as_str()?.to_string(),
                    row["level"].as_str()?.to_string(),
                ),
                (
                    row["gold"].as_u64()? as usize,
                    row["produced"].as_u64()? as usize,
                    row["matched"].as_u64()? as usize,
                ),
            );
        }
        for row in record["readings"].as_array()? {
            metrics.readings.insert(
                row["arm"].as_str()?.to_string(),
                (
                    row["aligned"].as_u64()? as usize,
                    row["agreed"].as_u64()? as usize,
                ),
            );
        }
        for row in record["reading_transitions"].as_array()? {
            metrics.reading_transitions.insert(
                row["arm"].as_str()?.to_string(),
                (
                    row["became_wrong"].as_u64()? as usize,
                    row["became_right"].as_u64()? as usize,
                ),
            );
        }
        let counts = |value: &serde_json::Value| -> BTreeMap<String, usize> {
            value
                .as_object()
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(|(key, count)| Some((key.clone(), count.as_u64()? as usize)))
                        .collect()
                })
                .unwrap_or_default()
        };
        metrics.gold_morphemes = counts(&record["gold_morphemes"]);
        metrics.rule_changes = counts(&record["rule_changes"]);
        metrics.readings_unaligned = counts(&record["readings_unaligned"]);
        metrics.failures = record["failures"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        metrics.excluded = record["excluded"].as_str().map(str::to_string);
        metrics.changed_regions = record["changed_regions"].as_u64().unwrap_or(0) as usize;
        metrics.neither_matched = record["neither_matched"].as_bool().unwrap_or(false);
        metrics.reference_spans_crossing_regions = record["reference_spans_crossing_regions"]
            .as_u64()
            .unwrap_or(0) as usize;
        Some(metrics)
    }
}

/// Everything a report is built from, foldable one case at a time.
///
/// **Totals come from folding every completed case**, whether it was measured in this invocation or
/// restored from a checkpoint. That is the only arrangement under which an interrupted run and an
/// uninterrupted one publish the same numbers.
#[derive(Default)]
struct Totals {
    arms: BTreeMap<Arm, Scored>,
    changed_regions: usize,
    neither_matched: usize,
    unpaired: usize,
    refusals: BTreeMap<String, usize>,
    rule_changes: BTreeMap<String, usize>,
    reading_transitions: BTreeMap<String, (usize, usize)>,
    reference_spans_crossing_regions: usize,
    readings_unaligned: BTreeMap<String, usize>,
    rule_outcomes: BTreeMap<(String, String, String), (usize, usize, usize)>,
    reading_strings: BTreeMap<String, (usize, usize)>,
}

fn arm_named(name: &str) -> Option<Arm> {
    Arm::all().into_iter().find(|arm| arm.name() == name)
}

impl Totals {
    fn fold(&mut self, metrics: &CaseMetrics) {
        for ((arm, level), (gold, produced, matched)) in &metrics.levels {
            let Some(arm) = arm_named(arm) else { continue };
            // `level` is owned here but `Scored` keys on `&'static str`; resolve it back to the
            // static name so a folded total and a measured one share a key.
            let Some(level) = LEVELS.iter().find(|known| *known == level) else {
                continue;
            };
            self.arms
                .entry(arm)
                .or_default()
                .level(level)
                .add(*gold, *produced, *matched);
        }
        for (arm, (aligned, agreed)) in &metrics.readings {
            let Some(arm) = arm_named(arm) else { continue };
            let entry = self.arms.entry(arm).or_default();
            entry.readings.aligned += aligned;
            entry.readings.agreed += agreed;
        }
        for (arm, gold) in &metrics.gold_morphemes {
            let Some(arm) = arm_named(arm) else { continue };
            self.arms.entry(arm).or_default().gold_morphemes += gold;
        }
        for arm in &metrics.failures {
            if let Some(arm) = arm_named(arm) {
                self.arms.entry(arm).or_default().failures += 1;
            }
        }
        if let Some(reason) = &metrics.excluded {
            self.unpaired += 1;
            *self.refusals.entry(reason.clone()).or_default() += 1;
        }
        self.changed_regions += metrics.changed_regions;
        for (rule, count) in &metrics.rule_changes {
            *self.rule_changes.entry(rule.clone()).or_default() += count;
        }
        if metrics.neither_matched {
            self.neither_matched += 1;
        }
        for (arm, (worse, better)) in &metrics.reading_transitions {
            let entry = self.reading_transitions.entry(arm.clone()).or_default();
            entry.0 += worse;
            entry.1 += better;
        }
        self.reference_spans_crossing_regions += metrics.reference_spans_crossing_regions;
        for (reason, count) in &metrics.readings_unaligned {
            *self.readings_unaligned.entry(reason.clone()).or_default() += count;
        }
        for (key, (recovered, lost, crossing)) in &metrics.rule_outcomes {
            let entry = self.rule_outcomes.entry(key.clone()).or_default();
            entry.0 += recovered;
            entry.1 += lost;
            entry.2 += crossing;
        }
        for (arm, (same_class, total)) in &metrics.reading_strings {
            let entry = self.reading_strings.entry(arm.clone()).or_default();
            entry.0 += same_class;
            entry.1 += total;
        }
    }
}

/// Every level name either reference uses, so a folded record can be keyed to a `&'static str`.
const LEVELS: [&str; 5] = [
    "short unit (SUW)",
    "long unit (LUW)",
    "morpheme",
    "基本句 (basic phrase)",
    "文節 (bunsetsu)",
];

/// What each rule's decisions did to the reference units inside their own regions.
///
/// **This is the benefit a substitution count cannot supply.** Knowing a rule fired 206 times says
/// nothing about whether those 206 regions came out better; only comparing what the chosen and the
/// rejected sequence each match can. Units straddling a region's edge are counted apart, since
/// neither side can be credited or blamed for a unit the region does not contain.
fn score_rules(
    metrics: &mut CaseMetrics,
    decisions: &[RegionDecision],
    level: &str,
    reference: &BTreeSet<(usize, usize)>,
) {
    for decision in decisions {
        let (mut recovered, mut lost, mut crossing) = (0usize, 0usize, 0usize);
        for unit in reference {
            let (start, end) = *unit;
            if start >= decision.end || end <= decision.start {
                continue;
            }
            if start < decision.start || end > decision.end {
                crossing += 1;
                continue;
            }
            match (
                decision.chosen_spans.contains(unit),
                decision.rejected_spans.contains(unit),
            ) {
                (true, false) => recovered += 1,
                (false, true) => lost += 1,
                _ => {}
            }
        }
        let direction = match decision.chose {
            Arm::Ichiran => "prefer-ichiran",
            _ => "prefer-lindera",
        };
        let entry = metrics
            .rule_outcomes
            .entry((
                decision.rule.clone(),
                direction.to_string(),
                level.to_string(),
            ))
            .or_default();
        entry.0 += recovered;
        entry.1 += lost;
        entry.2 += crossing;
    }
}

/// The span set an analysis produces, as byte offsets into the scored text.
fn spans(analysis: &Analysis) -> BTreeSet<(usize, usize)> {
    analysis
        .tokens
        .iter()
        .filter_map(|token| Some((token.start?, token.end?)))
        .collect()
}

/// Katakana folded to hiragana, so コーヒー and こーひー are not a disagreement.
fn to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            other => other,
        })
        .collect()
}

/// What one arm said about each gold morpheme it aligned to: the normalised reading, and whether
/// it matched.
type ReadingsBySpan = BTreeMap<(usize, usize), (String, bool)>;

/// One region a named rule decided, with what each side proposed for it.
struct RegionDecision {
    start: usize,
    end: usize,
    rule: String,
    /// Which engine's sequence the rule preferred.
    chose: Arm,
    /// Spans the preferred sequence contributes inside this region.
    chosen_spans: BTreeSet<(usize, usize)>,
    /// Spans the rejected sequence would have contributed.
    rejected_spans: BTreeSet<(usize, usize)>,
}

/// Run all three arms over one sentence, from one Ichiran call.
///
/// **Ichiran is consulted once**, and Hybrid reuses that analysis rather than asking again — which
/// is also what a hybrid would do in production. Asking twice would triple the cost of the run and
/// measure nothing extra.
struct Analysed {
    lindera: Option<Analysis>,
    ichiran: Option<Analysis>,
    hybrid: Option<Analysis>,
    /// Regions where the selector actually changed which analysis is shown.
    changed_regions: usize,
    /// Which named rule decided each of those, so a gain or loss can be attributed to a rule
    /// rather than to "the hybrid".
    rule_changes: BTreeMap<String, usize>,
    /// **Every region a named rule decided, in either direction**, with the spans each side
    /// offered — so a rule's *benefit* can be measured and not merely its frequency. Counting
    /// substitutions says how busy a rule is; it says nothing about whether the substitution was
    /// an improvement, and it omits the regions where a rule explicitly kept Lindera.
    decisions: Vec<RegionDecision>,
    /// Why the two analyses could not be combined, when they could not.
    ///
    /// **Kept, because "hybrid produced nothing" is not a diagnosis.** `combine` refuses when an
    /// analysis fails its span contract, and which engine failed which check is the finding.
    not_comparable: Option<String>,
}

fn analyse(
    text: &str,
    lindera: &LinderaAnalyzer,
    ichiran: &IchiranAnalyzer,
    dictionaries: &[Dictionary],
) -> Analysed {
    let pipeline = Pipeline::new(dictionaries);
    let left = lindera.analyze(text).ok().map(|mut analysis| {
        pipeline.display(&mut analysis);
        analysis
    });
    let right = ichiran.analyze(text).ok().map(|mut analysis| {
        pipeline.display(&mut analysis);
        analysis
    });
    let mut not_comparable = None;
    let mut rules: BTreeMap<String, usize> = BTreeMap::new();
    let mut regions: Vec<RegionDecision> = Vec::new();
    let (hybrid_analysis, changed) = match (&left, &right) {
        (Some(left), Some(right)) => match hybrid::combine(left, right, dictionaries) {
            Ok((decisions, assembled)) => {
                let mut changed = 0usize;
                for decision in &decisions {
                    let (rule, chose) = match decision.choice {
                        hybrid::Choice::PreferIchiran { rule } => {
                            changed += 1;
                            (rule, Arm::Ichiran)
                        }
                        // **Included, and it was not before.** A rule that explicitly keeps
                        // Lindera has decided something; leaving it out measured half the
                        // selector.
                        hybrid::Choice::PreferLindera { rule } => (rule, Arm::Lindera),
                        _ => continue,
                    };
                    *rules.entry(rule.to_string()).or_default() += 1;
                    let spans_of = |candidate: &hybrid::Candidate| -> BTreeSet<(usize, usize)> {
                        candidate
                            .tokens
                            .iter()
                            .filter_map(|token| Some((token.start?, token.end?)))
                            .collect()
                    };
                    let (chosen_spans, rejected_spans) = match chose {
                        Arm::Ichiran => (spans_of(&decision.ichiran), spans_of(&decision.lindera)),
                        _ => (spans_of(&decision.lindera), spans_of(&decision.ichiran)),
                    };
                    regions.push(RegionDecision {
                        start: decision.region.start,
                        end: decision.region.end,
                        rule: rule.to_string(),
                        chose,
                        chosen_spans,
                        rejected_spans,
                    });
                }
                (Some(assembled), changed)
            }
            Err(reason) => {
                not_comparable = Some(describe_refusal(&reason));
                (None, 0)
            }
        },
        _ => {
            if left.is_none() {
                not_comparable = Some("lindera produced no analysis".into());
            } else if right.is_none() {
                not_comparable = Some("ichiran produced no analysis".into());
            }
            (None, 0)
        }
    };
    Analysed {
        lindera: left,
        ichiran: right,
        hybrid: hybrid_analysis,
        changed_regions: changed,
        rule_changes: rules,
        decisions: regions,
        not_comparable,
    }
}

/// Name a refusal by engine and fault kind, without carrying corpus text.
fn describe_refusal(reason: &hybrid::NotComparable) -> String {
    match reason {
        hybrid::NotComparable::DifferentSource => "the two analyses describe different text".into(),
        hybrid::NotComparable::Invalid { engine, faults } => {
            let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
            for fault in faults {
                *kinds
                    .entry(match fault {
                        hybrid::SpanFault::Ungrounded { .. } => "ungrounded token",
                        hybrid::SpanFault::OutOfBounds { .. } => "span out of bounds",
                        hybrid::SpanFault::NotACharBoundary { .. } => "span off a char boundary",
                        hybrid::SpanFault::SurfaceDisagrees { .. } => "surface disagrees with span",
                        hybrid::SpanFault::OutOfOrder { .. } => {
                            "tokens out of order or overlapping"
                        }
                        hybrid::SpanFault::Uncovered { .. } => "source covered by no token",
                    })
                    .or_default() += 1;
            }
            let summary: Vec<String> = kinds
                .into_iter()
                .map(|(kind, count)| format!("{count} {kind}"))
                .collect();
            format!("{}: {}", engine.name(), summary.join(", "))
        }
    }
}

fn report(reference: &str, totals: &Totals, levels: &[&str]) {
    println!("\n=== {reference} ===");
    for level in levels {
        println!("\n  {level}");
        println!("    arm         gold  produced   matched       P       R      F1");
        for arm in Arm::all() {
            let Some(result) = totals.arms.get(&arm) else {
                continue;
            };
            let Some(row) = result.levels.get(level) else {
                continue;
            };
            println!(
                "    {:<10} {:>6} {:>9} {:>9}  {:>5.1}%  {:>5.1}%  {:>6.1}",
                arm.name(),
                row.gold,
                row.produced,
                row.matched,
                row.precision() * 100.0,
                row.recall() * 100.0,
                row.f1() * 100.0,
            );
        }
    }
}

/// The identity every arm's cases are keyed on, and the artifact records.
fn identity(dictionaries: &[Dictionary], workload: &str, route: &str) -> Identity {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| {
            format!(
                "{}:{}",
                sakatsu_core::hashing::hex(Sha256::digest(&bytes)),
                bytes.len()
            )
        })
        .unwrap_or_else(|| "unknown".into());
    let fingerprints: Vec<String> = dictionaries
        .iter()
        .map(|dictionary| {
            format!(
                "{}@{}",
                dictionary.meta().title,
                dictionary
                    .content_fingerprint()
                    .expect("resource content identity is required")
            )
        })
        .collect();
    Identity::new()
        // **Full digest and byte length**, not the runner's abbreviated human-readable summary.
        .with("executable", executable)
        .with("source_tree", runner::source_digest())
        // **Content**, not title-and-count: two dictionaries can agree on those and differ inside.
        .with("dictionaries", fingerprints.join(" | "))
        .with("workload", workload)
        .with("scoring_method", SCORING_METHOD)
        .with("ruleset", hybrid::RULESET)
        .with("ichiran_route", route)
}

/// Hash what was actually measured: the source text, the ids in order, and how they were chosen.
///
/// **Ids alone are not the workload.** Two runs can select the same identifiers from a corpus whose
/// bytes have changed underneath them, and a resume keyed on identifiers would happily continue
/// across that. So the source of every selected sentence goes into the hash, together with the
/// sampling that chose them, the normalisation applied, and the scoring version — each of which
/// changes what the numbers mean without changing which sentences they are about.
fn workload_id(ids: &[String], sources: &[&str], sampling: &str, normalisation: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"workload-v2");
    hasher.update(SCORING_METHOD.as_bytes());
    hasher.update(sampling.as_bytes());
    hasher.update(normalisation.as_bytes());
    hasher.update(ids.len().to_le_bytes());
    for (id, source) in ids.iter().zip(sources) {
        hasher.update(id.as_bytes());
        hasher.update([0]);
        // The bytes themselves, so a corpus edited under a stable identifier refuses reuse. Only
        // the digest survives into any artifact — KWDLC's text never leaves this function.
        hasher.update(source.as_bytes());
        hasher.update([0]);
    }
    sakatsu_core::hashing::hex(hasher.finalize())
}

/// Open Ichiran on the **session route**, which is required rather than optional.
///
/// A comparison missing one of its engines is a single-engine measurement wearing a comparison's
/// name, and the one-shot route is a different measurement again — ten times the latency and a
/// different code path — so mixing them would average two things.
fn ichiran_on_the_session_route() -> (IchiranAnalyzer, bool, String) {
    let engine = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    engine.warm();
    let answered = engine.analyze("これって").is_ok();
    let route = sakatsu_core::analyzer::ichiran::server::last_route();
    (
        engine,
        answered && route.is_session(),
        route.name().to_string(),
    )
}

/// **UD_Japanese-GSD: short and long units, reported separately.**
///
/// Readings are not scored here at all. GSD's come from UniDic, which is the built-in analyzer's
/// own dictionary, so scoring Lindera's readings against them would ask whether UniDic agrees with
/// itself — see `ud_gold.rs`'s header, which excludes them for the same reason.
#[test]
#[ignore = "needs scripts/fetch-ud-japanese.ps1, the dictionaries and Ichiran"]
fn three_arms_against_ud_japanese_gsd() {
    let started = Instant::now();
    let dictionaries = common::installed_dictionaries();
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let (ichiran, session, route) = ichiran_on_the_session_route();

    let sentences = common::gold::test_and_dev();
    let cap: usize = std::env::var("SAKATSU_UD_ICHIRAN")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(300);
    let stride = sentences.len().div_ceil(cap.max(1)).max(1);
    let sampled: Vec<&common::gold::GoldSentence> = sentences.iter().step_by(stride).collect();
    let ids: Vec<String> = sampled.iter().map(|sentence| sentence.id.clone()).collect();

    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");
    let sources: Vec<&str> = sampled
        .iter()
        .map(|sentence| sentence.text.as_str())
        .collect();
    let sampling = format!("stride over test+dev, SAKATSU_UD_ICHIRAN={cap}, stride={stride}");
    let key = identity(
        &dictionaries,
        &workload_id(
            &ids,
            &sources,
            &sampling,
            "none — gold text is used as read",
        ),
        &route,
    );
    let resume = Resume::open(
        evaluation.join("hybrid-reference-ud-resume.json"),
        key.clone(),
    );
    if let Some(reason) = resume.refused() {
        println!("RESUME REFUSED: {reason}");
        if let Some(kept) = resume.superseded() {
            println!("  the previous checkpoint is kept at {}", kept.display());
        }
    }

    let mut run = Run::new(Profile::InstalledResources, ids.iter().cloned())
        .engine("lindera, ichiran, hybrid")
        .needs(
            Requirement::required("the treebank", !sentences.is_empty())
                .because(format!("{} sentences", sentences.len())),
        )
        .needs(
            Requirement::required("dictionaries", !dictionaries.is_empty())
                .because(format!("{} opened", dictionaries.len())),
        )
        .needs(
            Requirement::required("ichiran via the session route", session)
                .because(format!("route {route}")),
        );
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

    let mut totals = Totals::default();
    let mut completed: BTreeMap<String, serde_json::Value> = resume.completed().clone();
    let mut reused = 0usize;
    let mut refused_records = 0usize;
    let mut budget_spent = false;

    for sentence in &sampled {
        // **A resumed case contributes its numbers, not just its verdict.** Anything that cannot
        // supply them is re-measured rather than folded as zero.
        if let Some(record) = completed.get(&sentence.id) {
            match CaseMetrics::from_json(record) {
                Some(metrics) => {
                    let outcome = if metrics.excluded.is_some() {
                        Outcome::failed(runner::Stage::Analyzer, "an arm produced no analysis")
                    } else {
                        Outcome::Passed
                    };
                    totals.fold(&metrics);
                    run.record(sentence.id.clone(), outcome);
                    reused += 1;
                    continue;
                }
                None => {
                    refused_records += 1;
                    completed.remove(&sentence.id);
                }
            }
        }
        if started.elapsed() > BUDGET {
            run.record(
                sentence.id.clone(),
                Outcome::incomplete(
                    "the invocation's time budget was exhausted before this case was reached; \
                     run again to continue",
                ),
            );
            budget_spent = true;
            continue;
        }

        let analysed = analyse(&sentence.text, &lindera, &ichiran, &dictionaries);
        let mut metrics = CaseMetrics {
            changed_regions: analysed.changed_regions,
            rule_changes: analysed.rule_changes.clone(),
            ..CaseMetrics::default()
        };

        let short: BTreeSet<(usize, usize)> = sentence
            .tokens
            .iter()
            .map(|token| (token.start, token.end))
            .collect();
        let long: BTreeSet<(usize, usize)> = common::gold::long_units(&sentence.tokens)
            .into_iter()
            .collect();

        // **Paired, or it is not a comparison.** An arm that produced nothing must not simply drop
        // the sentence: its gold would go uncounted for that arm and counted for the others, and
        // the arm with the most failures would post the best recall on the smallest denominator.
        let complete =
            analysed.lindera.is_some() && analysed.ichiran.is_some() && analysed.hybrid.is_some();
        if !complete {
            for (arm, analysis) in [
                (Arm::Lindera, &analysed.lindera),
                (Arm::Ichiran, &analysed.ichiran),
                (Arm::Hybrid, &analysed.hybrid),
            ] {
                if analysis.is_none() {
                    metrics.failures.push(arm.name().to_string());
                }
            }
            metrics.excluded = Some(
                analysed
                    .not_comparable
                    .clone()
                    .unwrap_or_else(|| "unstated".into()),
            );
            // A case that scored nothing still changed no region that anyone can be credited for.
            metrics.changed_regions = 0;
            metrics.rule_changes.clear();
            totals.fold(&metrics);
            completed.insert(sentence.id.clone(), metrics.to_json());
            run.record(
                sentence.id.clone(),
                Outcome::failed(runner::Stage::Analyzer, "an arm produced no analysis"),
            );
            continue;
        }

        // Reference units the selector's regions cut across: neither arm can be credited or blamed
        // for them per region, so they are reported rather than silently attributed.
        if let (Some(left), Some(right)) = (&analysed.lindera, &analysed.ichiran) {
            if let Ok(regions) = hybrid::regions(left, right) {
                let bounds: BTreeSet<usize> = regions
                    .iter()
                    .flat_map(|region| [region.start, region.end])
                    .collect();
                metrics.reference_spans_crossing_regions = long
                    .iter()
                    .filter(|(start, end)| bounds.iter().any(|at| *at > *start && *at < *end))
                    .count();
            }
        }

        let mut long_hit = false;
        for (arm, analysis) in [
            (Arm::Lindera, &analysed.lindera),
            (Arm::Ichiran, &analysed.ichiran),
            (Arm::Hybrid, &analysed.hybrid),
        ] {
            let Some(analysis) = analysis else { continue };
            let produced = spans(analysis);
            let matched_short = produced.intersection(&short).count();
            let matched_long = produced.intersection(&long).count();
            metrics.levels.insert(
                (arm.name().to_string(), "short unit (SUW)".to_string()),
                (short.len(), produced.len(), matched_short),
            );
            metrics.levels.insert(
                (arm.name().to_string(), "long unit (LUW)".to_string()),
                (long.len(), produced.len(), matched_long),
            );
            long_hit |= matched_long > 0;
        }
        metrics.neither_matched = !long_hit && !long.is_empty();
        score_rules(
            &mut metrics,
            &analysed.decisions,
            "short unit (SUW)",
            &short,
        );
        score_rules(&mut metrics, &analysed.decisions, "long unit (LUW)", &long);

        totals.fold(&metrics);
        completed.insert(sentence.id.clone(), metrics.to_json());
        run.record(sentence.id.clone(), Outcome::Passed);
    }

    if refused_records > 0 {
        println!(
            "  {refused_records} checkpoint records carried no metrics and were re-measured \
             rather than folded as zero"
        );
    }
    if reused > 0 {
        println!("  {reused} cases restored from the checkpoint, with their counts");
    }

    if let Err(error) = resume.save(&completed) {
        println!("  WARNING: the checkpoint was not saved: {error}");
    }

    report(
        "UD_Japanese-GSD (readings not scored: UniDic lineage)",
        &totals,
        &["short unit (SUW)", "long unit (LUW)"],
    );
    summarise(&totals, budget_spent);
    publish(
        &evaluation,
        "hybrid-reference-ud",
        &key,
        &totals,
        sampled.len(),
    );
    run.accounted_for().expect_success();
}

/// The lines every reference prints after its score table.
fn summarise(totals: &Totals, budget_spent: bool) {
    println!(
        "\n  regions where the selector changed the analysis: {}",
        totals.changed_regions
    );
    for (rule, count) in &totals.rule_changes {
        println!("    {count}x by {rule}");
    }
    if !totals.rule_outcomes.is_empty() {
        // **A substitution count is not a benefit.** These rows say what each rule's decisions did
        // to the reference units lying wholly inside the regions it decided — recovered where the
        // chosen sequence matches a unit the rejected one missed, lost the other way round.
        // Crossing units are reported and never attributed, because neither side contains them.
        println!(
            "
  what each rule's decisions did inside their own regions"
        );
        // The level label goes last on purpose: 基本句 and 文節 are double-width, so any column
        // after them lands one place right of its header on a terminal that pads by character.
        println!("    recovered    lost     net  crossing   rule / direction / level");
        for ((rule, direction, level), (recovered, lost, crossing)) in &totals.rule_outcomes {
            let net = *recovered as i64 - *lost as i64;
            println!(
                "    {recovered:>9} {lost:>7} {net:>7} {crossing:>9}   {rule} / {direction} / {level}"
            );
        }
    }
    println!(
        "  reference units the regions cut across: {} — no arm is credited or blamed for these \
         per region",
        totals.reference_spans_crossing_regions
    );
    println!(
        "  sentences where no arm matched a single reference unit: {}",
        totals.neither_matched
    );
    println!(
        "  sentences excluded from every arm because one produced nothing: {} — the rows above \
         are a PAIRED comparison over the rest",
        totals.unpaired
    );
    for (reason, count) in &totals.refusals {
        println!("    {count}x {reason}");
    }
    for arm in Arm::all() {
        if let Some(result) = totals.arms.get(&arm) {
            if result.failures > 0 {
                println!(
                    "  {} produced nothing for {} sentences",
                    arm.name(),
                    result.failures
                );
            }
        }
    }
    if !totals.reading_transitions.is_empty() {
        println!("\n  readings against the lindera baseline, over spans all three arms aligned");
        println!("    arm         became wrong   became right");
        for (arm, (worse, better)) in &totals.reading_transitions {
            println!("    {arm:<10} {worse:>12} {better:>14}");
        }
    }
    if !totals.reading_strings.is_empty() {
        // **Right-to-wrong counts of zero do not establish identical readings.** Two different
        // wrong readings are both wrong, and a transition table calls that pair unchanged; a
        // reader looking at furigana sees two different words. This compares the strings.
        println!(
            "
  normalised reading strings differing from lindera, over the same spans"
        );
        println!("    arm          differing   of which classified the same");
        for (arm, (same_class, differing)) in &totals.reading_strings {
            println!("    {arm:<10} {differing:>11} {same_class:>28}");
        }
    }
    if !totals.readings_unaligned.is_empty() {
        println!("\n  gold readings that left the comparison");
        for (reason, count) in &totals.readings_unaligned {
            println!("    {count:>6}  {reason}");
        }
    }
    if budget_spent {
        println!("  BUDGET EXHAUSTED — some cases were not reached; the run fails and resumes");
    }
}

fn publish(
    evaluation: &std::path::Path,
    stem: &str,
    key: &Identity,
    totals: &Totals,
    cases: usize,
) {
    let arms: serde_json::Map<String, serde_json::Value> = Arm::all()
        .into_iter()
        .filter_map(|arm| {
            totals.arms.get(&arm).map(|result| {
                (
                    arm.name().to_string(),
                    json!({
                        "levels": result.levels.iter()
                            .map(|(name, level)| (name.to_string(), level.describe()))
                            .collect::<serde_json::Map<_, _>>(),
                        "readings": result.readings.describe(result.gold_morphemes),
                        "sentences_with_no_analysis": result.failures,
                    }),
                )
            })
        })
        .collect();
    let document = json!({
        "what_this_is": "Three arms against one reference, all scored on finished production \
                         analyses with the pipeline applied exactly once.",
        "identity": key.describe(),
        "cases": cases,
        "population": "PAIRED — every arm is scored over exactly the sentences all three analysed. \
                       Excluded sentences are counted separately and belong to no arm's totals.",
        "arms": arms,
        "regions_where_the_selector_changed_the_analysis": totals.changed_regions,
        "changed_regions_by_rule": totals.rule_changes,
        "reference_units_crossing_region_boundaries": totals.reference_spans_crossing_regions,
        "sentences_where_no_arm_matched": totals.neither_matched,
        "sentences_excluded_because_an_arm_produced_nothing": totals.unpaired,
        "why_they_were_excluded": totals.refusals,
        "reading_transitions_against_lindera": totals.reading_transitions.iter()
            .map(|(arm, (worse, better))| (arm.clone(), json!({
                "became_wrong": worse,
                "became_right": better,
                "population": "spans all three arms aligned to the same gold morpheme",
            })))
            .collect::<serde_json::Map<_, _>>(),
        "gold_readings_that_left_the_comparison": totals.readings_unaligned,
        "reading_strings_differing_from_lindera": totals.reading_strings.iter()
            .map(|(arm, (same_class, differing))| (arm.clone(), json!({
                "differing_normalised_strings": differing,
                "of_which_classification_unchanged": same_class,
                "note": "a transition table calls these unchanged; the reader does not",
            })))
            .collect::<serde_json::Map<_, _>>(),
        "rule_outcomes": totals.rule_outcomes.iter()
            .map(|((rule, direction, level), (recovered, lost, crossing))| (
                format!("{rule} / {direction} / {level}"),
                json!({
                    "reference_units_recovered": recovered,
                    "reference_units_lost": lost,
                    "reference_units_crossing_the_region": crossing,
                    "against": "the sequence this rule rejected in the same region",
                }),
            ))
            .collect::<serde_json::Map<_, _>>(),
        "limits": "Boundary agreement with one annotation standard is not correctness; SUW and LUW \
                   are correlated observations of the same analysis, not independent evidence.",
    });
    let rendered = serde_json::to_string_pretty(&document).expect("serializes");
    let digest = sakatsu_core::hashing::hex(Sha256::digest(rendered.as_bytes()));
    let path = evaluation.join(format!("{stem}-{}.json", &digest[..16]));
    let staging = evaluation.join(format!(".{stem}-{}.partial", &digest[..16]));
    std::fs::create_dir_all(evaluation).expect("create");
    std::fs::write(&staging, &rendered).expect("write");
    std::fs::rename(&staging, &path).expect("publish");
    println!("  wrote {}", path.display());
}

/// **KWDLC: three granularities and the readings no other reference here can judge.**
///
/// Readings *are* scored, unlike the GSD arm: KWDLC is annotated in the JUMAN lineage, related to
/// neither UniDic nor JMdict, so it can hold the coats. They are checked only where a span matched
/// exactly — anywhere else it would be two accounts of different words — and both denominators are
/// reported so an accuracy figure cannot hide how little of gold it covered.
#[test]
#[ignore = "needs scripts/fetch-kwdlc.ps1, the dictionaries and Ichiran"]
fn three_arms_against_kwdlc() {
    let started = Instant::now();
    let dictionaries = common::installed_dictionaries();
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let (ichiran, session, route) = ichiran_on_the_session_route();

    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");
    let corpus = std::env::var_os("SAKATSU_KWDLC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            common::dictionaries_dir()
                .parent()
                .map(|parent| parent.join("corpora").join("kwdlc"))
                .unwrap_or_default()
        })
        .join("knp");

    let (reserved, audit) = common::reference::reserved_documents(&evaluation);
    assert!(
        !reserved.is_empty(),
        "the reservation must be readable, or this run would score the pool it is meant to hold \
         back; {audit}"
    );

    // **Excluded before their annotations are loaded.** Filtering results afterwards would mean
    // the reserved documents had been read, and "reserved" would describe only the scoring.
    let (all_files, walk_faults) = knp::collect(&corpus);
    let development: Vec<std::path::PathBuf> = all_files
        .iter()
        .filter(|path| {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            !reserved.contains(&stem)
        })
        .cloned()
        .collect();
    let withheld = all_files.len() - development.len();
    println!(
        "  {} files, {withheld} reserved and not loaded — {audit}",
        all_files.len()
    );

    let sentences = match knp::load_files(&development, walk_faults) {
        Ok(sentences) => sentences,
        Err(damaged) => panic!("{damaged}"),
    };

    // The same stride the standing harness uses, over the development side only.
    let limit: usize = std::env::var("SAKATSU_KWDLC_SAMPLE")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(2000);
    let stride = sentences.len().div_ceil(limit.max(1)).max(1);
    let strided: Vec<&knp::Sentence> = sentences.iter().step_by(stride).collect();
    let cap: usize = std::env::var("SAKATSU_KWDLC_ICHIRAN")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(300)
        .min(strided.len());
    // A prefix, exactly as `kwdlc_gold` takes it, so the arms score the same sentences.
    let sampled = &strided[..cap];
    // **Document identity, not position.** `kwdlc-0041` names whichever sentence happens to land
    // there; `w201106-0000060050:2` names one sentence whatever the sampling does.
    let ids: Vec<String> = sampled.iter().map(|sentence| sentence.id()).collect();

    let sources: Vec<&str> = sampled
        .iter()
        .map(|sentence| sentence.text.as_str())
        .collect();
    let sampling = format!(
        "development side only, SAKATSU_KWDLC_SAMPLE={limit}, stride={stride}, prefix of {cap}"
    );
    let key = identity(
        &dictionaries,
        &workload_id(
            &ids,
            &sources,
            &sampling,
            "none — gold text is used as read",
        ),
        &route,
    )
    .with("reservation", audit.clone());
    let resume = Resume::open(
        evaluation.join("hybrid-reference-kwdlc-resume.json"),
        key.clone(),
    );
    if let Some(reason) = resume.refused() {
        println!("RESUME REFUSED: {reason}");
        if let Some(kept) = resume.superseded() {
            println!("  the previous checkpoint is kept at {}", kept.display());
        }
    }
    if let Some(reason) = resume.preservation_failed() {
        panic!("the previous checkpoint could not be preserved: {reason}");
    }

    let mut run = Run::new(Profile::InstalledResources, ids.iter().cloned())
        .engine("lindera, ichiran, hybrid")
        .needs(
            Requirement::required("the corpus", !sentences.is_empty())
                .because(format!("{} development sentences", sentences.len())),
        )
        .needs(
            Requirement::required("the reservation", !reserved.is_empty())
                .because(format!("{} documents withheld", reserved.len())),
        )
        .needs(
            Requirement::required("dictionaries", !dictionaries.is_empty())
                .because(format!("{} opened", dictionaries.len())),
        )
        .needs(
            Requirement::required("ichiran via the session route", session)
                .because(format!("route {route}")),
        );
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

    let mut totals = Totals::default();
    let mut completed = resume.completed().clone();
    let mut reused = 0usize;
    let mut refused_records = 0usize;
    let mut budget_spent = false;

    for (id, sentence) in ids.iter().zip(sampled) {
        if let Some(record) = completed.get(id) {
            match CaseMetrics::from_json(record) {
                Some(metrics) => {
                    let outcome = if metrics.excluded.is_some() {
                        Outcome::failed(runner::Stage::Analyzer, "an arm produced no analysis")
                    } else {
                        Outcome::Passed
                    };
                    totals.fold(&metrics);
                    run.record(id.clone(), outcome);
                    reused += 1;
                    continue;
                }
                None => {
                    refused_records += 1;
                    completed.remove(id);
                }
            }
        }
        if started.elapsed() > BUDGET {
            run.record(
                id.clone(),
                Outcome::incomplete(
                    "the invocation's time budget was exhausted before this case was reached; \
                     run again to continue",
                ),
            );
            budget_spent = true;
            continue;
        }

        let analysed = analyse(&sentence.text, &lindera, &ichiran, &dictionaries);
        let mut metrics = CaseMetrics {
            changed_regions: analysed.changed_regions,
            rule_changes: analysed.rule_changes.clone(),
            ..CaseMetrics::default()
        };

        let complete =
            analysed.lindera.is_some() && analysed.ichiran.is_some() && analysed.hybrid.is_some();
        if !complete {
            for (arm, analysis) in [
                (Arm::Lindera, &analysed.lindera),
                (Arm::Ichiran, &analysed.ichiran),
                (Arm::Hybrid, &analysed.hybrid),
            ] {
                if analysis.is_none() {
                    metrics.failures.push(arm.name().to_string());
                }
            }
            metrics.excluded = Some(
                analysed
                    .not_comparable
                    .clone()
                    .unwrap_or_else(|| "unstated".into()),
            );
            metrics.changed_regions = 0;
            metrics.rule_changes.clear();
            totals.fold(&metrics);
            completed.insert(id.clone(), metrics.to_json());
            run.record(
                id.clone(),
                Outcome::failed(runner::Stage::Analyzer, "an arm produced no analysis"),
            );
            continue;
        }

        let gold: [(&'static str, BTreeSet<(usize, usize)>); 3] = [
            (
                "morpheme",
                knp::spans_of(&sentence.morphemes, level::morpheme)
                    .into_iter()
                    .collect(),
            ),
            (
                "基本句 (basic phrase)",
                knp::spans_of(&sentence.morphemes, level::kihonku)
                    .into_iter()
                    .collect(),
            ),
            (
                "文節 (bunsetsu)",
                knp::spans_of(&sentence.morphemes, level::bunsetsu)
                    .into_iter()
                    .collect(),
            ),
        ];
        let by_span: BTreeMap<(usize, usize), &knp::Morpheme> = sentence
            .morphemes
            .iter()
            .map(|morpheme| ((morpheme.start, morpheme.end), morpheme))
            .collect();

        let mut any_matched = false;
        // arm → the reading it gave each gold morpheme it aligned to.
        // The reading itself, not only whether it was right: two different wrong readings are
        // still two different things to show a reader.
        let mut reading_by_arm: BTreeMap<Arm, ReadingsBySpan> = BTreeMap::new();
        for (arm, analysis) in [
            (Arm::Lindera, &analysed.lindera),
            (Arm::Ichiran, &analysed.ichiran),
            (Arm::Hybrid, &analysed.hybrid),
        ] {
            let Some(analysis) = analysis else { continue };
            metrics
                .gold_morphemes
                .insert(arm.name().to_string(), sentence.morphemes.len());
            let produced = spans(analysis);
            for (name, reference) in &gold {
                let matched = produced.intersection(reference).count();
                metrics.levels.insert(
                    (arm.name().to_string(), name.to_string()),
                    (reference.len(), produced.len(), matched),
                );
                if *name == "morpheme" && matched > 0 {
                    any_matched = true;
                }
            }
            let mut aligned = 0usize;
            let mut agreed = 0usize;
            let mut judged: ReadingsBySpan = BTreeMap::new();
            for token in &analysis.tokens {
                let (Some(start), Some(end)) = (token.start, token.end) else {
                    continue;
                };
                let Some(morpheme) = by_span.get(&(start, end)) else {
                    continue;
                };
                aligned += 1;
                let ours = to_hiragana(&token.reading);
                let right = ours == to_hiragana(&morpheme.reading);
                if right {
                    agreed += 1;
                }
                judged.insert((start, end), (ours, right));
            }
            metrics
                .readings
                .insert(arm.name().to_string(), (aligned, agreed));
            reading_by_arm.insert(arm, judged);
        }
        metrics.neither_matched = !any_matched;
        for (name, reference) in &gold {
            score_rules(&mut metrics, &analysed.decisions, name, reference);
        }

        // **Transitions over spans all three arms aligned**, which is the only population where a
        // change of reading is a change rather than a change of subject. A net figure would hide
        // that one arm can be right where another is wrong and wrong where it is right.
        if let Some(baseline) = reading_by_arm.get(&Arm::Lindera).cloned() {
            let common: BTreeSet<(usize, usize)> = baseline
                .keys()
                .filter(|span| {
                    Arm::all().into_iter().all(|arm| {
                        reading_by_arm
                            .get(&arm)
                            .is_some_and(|by| by.contains_key(*span))
                    })
                })
                .copied()
                .collect();
            for arm in [Arm::Ichiran, Arm::Hybrid] {
                let Some(theirs) = reading_by_arm.get(&arm) else {
                    continue;
                };
                let mut became_wrong = 0usize;
                let mut became_right = 0usize;
                let mut differing_total = 0usize;
                let mut differing_same_class = 0usize;
                for span in &common {
                    let (Some((ours, we_are_right)), Some((theirs, they_are_right))) =
                        (baseline.get(span), theirs.get(span))
                    else {
                        continue;
                    };
                    // **The strings, compared directly.** A classification table would call two
                    // different wrong readings unchanged.
                    if ours != theirs {
                        differing_total += 1;
                        if we_are_right == they_are_right {
                            differing_same_class += 1;
                        }
                    }
                    match (we_are_right, they_are_right) {
                        (true, false) => became_wrong += 1,
                        (false, true) => became_right += 1,
                        _ => {}
                    }
                }
                metrics
                    .reading_transitions
                    .insert(arm.name().to_string(), (became_wrong, became_right));
                metrics.reading_strings.insert(
                    arm.name().to_string(),
                    (differing_same_class, differing_total),
                );
            }
            // Gold readings that left the comparison, by cause — a reading is not "wrong" when it
            // was never compared, and lumping the two together would flatter or damn an arm for
            // grouping rather than for reading.
            let unaligned = sentence.morphemes.len().saturating_sub(common.len());
            if unaligned > 0 {
                *metrics
                    .readings_unaligned
                    .entry("no span all three arms aligned to this gold morpheme".to_string())
                    .or_default() += unaligned;
            }
            let punctuation = sentence
                .morphemes
                .iter()
                .filter(|morpheme| morpheme.reading.trim().is_empty())
                .count();
            if punctuation > 0 {
                *metrics
                    .readings_unaligned
                    .entry("gold morpheme carries no reading (punctuation and marks)".to_string())
                    .or_default() += punctuation;
            }
        }

        totals.fold(&metrics);
        completed.insert(id.clone(), metrics.to_json());
        run.record(id.clone(), Outcome::Passed);
    }

    if refused_records > 0 {
        println!(
            "  {refused_records} checkpoint records carried no metrics and were re-measured \
             rather than folded as zero"
        );
    }
    if reused > 0 {
        println!("  {reused} cases restored from the checkpoint, with their counts");
    }

    if let Err(error) = resume.save(&completed) {
        println!("  WARNING: the checkpoint was not saved: {error}");
    }

    report(
        "KWDLC (JUMAN lineage; readings scorable)",
        &totals,
        &["morpheme", "基本句 (basic phrase)", "文節 (bunsetsu)"],
    );
    println!("\n  readings, where a span matched a gold morpheme exactly");
    println!("    arm         aligned   agreed   accuracy   coverage of gold");
    for arm in Arm::all() {
        if let Some(result) = totals.arms.get(&arm) {
            let accuracy = if result.readings.aligned == 0 {
                0.0
            } else {
                result.readings.agreed as f64 / result.readings.aligned as f64
            };
            let coverage = if result.gold_morphemes == 0 {
                0.0
            } else {
                result.readings.aligned as f64 / result.gold_morphemes as f64
            };
            println!(
                "    {:<10} {:>7} {:>8}    {:>6.1}%             {:>5.1}%",
                arm.name(),
                result.readings.aligned,
                result.readings.agreed,
                accuracy * 100.0,
                coverage * 100.0
            );
        }
    }
    summarise(&totals, budget_spent);
    publish(
        &evaluation,
        "hybrid-reference-kwdlc",
        &key,
        &totals,
        sampled.len(),
    );
    run.accounted_for().expect_success();
}

#[cfg(test)]
mod resumed_totals {
    use super::*;

    /// A case that contributed to every arm, with deliberately unequal numbers so a fold that
    /// dropped or double-counted one would show.
    fn measured(seed: usize) -> CaseMetrics {
        let mut metrics = CaseMetrics::default();
        for (index, arm) in Arm::all().into_iter().enumerate() {
            for (level_index, level) in ["short unit (SUW)", "long unit (LUW)"].iter().enumerate() {
                metrics.levels.insert(
                    (arm.name().to_string(), level.to_string()),
                    (
                        10 + seed + level_index,
                        8 + index + seed,
                        4 + index + level_index,
                    ),
                );
            }
            metrics
                .readings
                .insert(arm.name().to_string(), (7 + seed, 3 + index));
            metrics
                .gold_morphemes
                .insert(arm.name().to_string(), 12 + seed);
            metrics
                .reading_transitions
                .insert(arm.name().to_string(), (index, seed % 3));
        }
        metrics.changed_regions = seed % 5;
        metrics
            .rule_changes
            .insert("exact-expression-for-the-region".into(), seed % 3);
        metrics.reference_spans_crossing_regions = seed % 2;
        metrics.neither_matched = seed.is_multiple_of(7);
        metrics
    }

    /// A case no arm could be scored on, which must still be counted as excluded.
    fn excluded() -> CaseMetrics {
        CaseMetrics {
            excluded: Some("ichiran: 1 source covered by no token".into()),
            failures: vec!["hybrid".into()],
            ..CaseMetrics::default()
        }
    }

    fn describe(totals: &Totals) -> String {
        let arms: Vec<String> = totals
            .arms
            .iter()
            .map(|(arm, scored)| {
                let levels: Vec<String> = scored
                    .levels
                    .iter()
                    .map(|(level, row)| {
                        format!("{level}:{}/{}/{}", row.gold, row.produced, row.matched)
                    })
                    .collect();
                format!(
                    "{}[{} r{}/{} g{} f{}]",
                    arm.name(),
                    levels.join(","),
                    scored.readings.aligned,
                    scored.readings.agreed,
                    scored.gold_morphemes,
                    scored.failures
                )
            })
            .collect();
        format!(
            "{} | changed {} | neither {} | unpaired {} | refusals {:?} | rules {:?} | \
             transitions {:?} | crossing {} | unaligned {:?}",
            arms.join(" "),
            totals.changed_regions,
            totals.neither_matched,
            totals.unpaired,
            totals.refusals,
            totals.rule_changes,
            totals.reading_transitions,
            totals.reference_spans_crossing_regions,
            totals.readings_unaligned,
        )
    }

    /// The set of cases a run would measure: successes, a failure, and an exclusion.
    fn workload() -> Vec<CaseMetrics> {
        let mut cases: Vec<CaseMetrics> = (0..8).map(measured).collect();
        cases.insert(3, excluded());
        let mut failed = measured(99);
        failed.failures.push("ichiran".into());
        cases.push(failed);
        cases
    }

    /// Fold `cases`, sending the first `resumed` of them through JSON on the way — which is what a
    /// checkpoint does to them.
    fn totals_with_resume(cases: &[CaseMetrics], resumed: usize) -> Totals {
        let mut totals = Totals::default();
        for (index, case) in cases.iter().enumerate() {
            if index < resumed {
                let record = case.to_json();
                let restored = CaseMetrics::from_json(&record)
                    .expect("a record this harness wrote must be readable");
                assert_eq!(&restored, case, "the round trip lost something");
                totals.fold(&restored);
            } else {
                totals.fold(case);
            }
        }
        totals
    }

    /// **The property the whole mechanism exists for.**
    ///
    /// An invocation that finished in one go, one that was interrupted halfway, and one that did
    /// nothing but restore a complete checkpoint must publish identical totals. The earlier
    /// harness failed this completely: it persisted outcomes only and skipped resumed cases when
    /// accumulating, so a fully resumed run would have reported zeros under a green verdict.
    #[test]
    fn resumed_totals_equal_uninterrupted_totals() {
        let cases = workload();
        let uninterrupted = describe(&totals_with_resume(&cases, 0));
        let half = describe(&totals_with_resume(&cases, cases.len() / 2));
        let fully = describe(&totals_with_resume(&cases, cases.len()));

        assert_eq!(
            uninterrupted, half,
            "a partially resumed run reported differently"
        );
        assert_eq!(
            uninterrupted, fully,
            "a fully resumed run reported differently"
        );

        // And the totals are not trivially empty, or the equality above would prove nothing.
        assert!(
            uninterrupted.contains("changed") && !uninterrupted.contains("unpaired 0"),
            "the fixture must exercise successes, failures and exclusions: {uninterrupted}"
        );
    }

    /// **An outcome-only record cannot supply a total**, and must be refused rather than folded as
    /// zero — which would quietly shrink every number it touched.
    #[test]
    fn an_outcome_only_record_is_refused_for_metrics() {
        let legacy = json!({ "outcome": "passed" });
        assert!(
            CaseMetrics::from_json(&legacy).is_none(),
            "a record from the previous schema must not be reusable for metrics"
        );

        let older = json!({ "schema": RECORD_SCHEMA - 1, "levels": [], "readings": [] });
        assert!(
            CaseMetrics::from_json(&older).is_none(),
            "an older schema must not be reusable either"
        );

        let current = measured(1).to_json();
        assert!(
            CaseMetrics::from_json(&current).is_some(),
            "a current record must still be reusable, or nothing ever resumes"
        );
    }

    /// **The workload identity must describe the input, not merely count it.**
    ///
    /// Each of these changes what the numbers mean while leaving something a weaker key would have
    /// matched on: the same ids over edited bytes, different ids at the same count, a different
    /// sampling that happens to select the same number of sentences.
    #[test]
    fn a_changed_workload_cannot_be_mistaken_for_the_same_one() {
        let ids: Vec<String> = vec!["doc-a:0".into(), "doc-a:1".into(), "doc-b:0".into()];
        let sources = ["本を読んでいる", "気持ちが伝わる", "今日はいい天気"];
        let base = workload_id(&ids, &sources, "stride=9", "none");

        assert_eq!(
            base,
            workload_id(&ids, &sources, "stride=9", "none"),
            "the same workload must hash the same, or nothing ever resumes"
        );

        // Same identifiers, edited corpus.
        let edited = ["本を読んでいる", "気持ちが伝わった", "今日はいい天気"];
        assert_ne!(
            base,
            workload_id(&ids, &edited, "stride=9", "none"),
            "edited source under stable identifiers must refuse reuse"
        );

        // Different sentences, same count.
        let others: Vec<String> = vec!["doc-c:0".into(), "doc-c:1".into(), "doc-d:0".into()];
        assert_ne!(
            base,
            workload_id(&others, &sources, "stride=9", "none"),
            "a different selection at the same count must refuse reuse"
        );

        // Same sentences, different sampling — the rows would describe a different population.
        assert_ne!(
            base,
            workload_id(&ids, &sources, "stride=54", "none"),
            "a different sampling must refuse reuse"
        );

        // Same everything, different normalisation.
        assert_ne!(
            base,
            workload_id(&ids, &sources, "stride=9", "line breaks folded"),
            "a different normalisation must refuse reuse"
        );

        // Order matters: the same sentences in a different order are a different workload, because
        // a prefix-capped arm would score different ones.
        let reordered: Vec<String> = vec!["doc-b:0".into(), "doc-a:0".into(), "doc-a:1".into()];
        assert_ne!(base, workload_id(&reordered, &sources, "stride=9", "none"));
    }

    /// The identity carries no corpus text, only its digest — KWDLC is consult-never-import.
    #[test]
    fn the_workload_identity_exports_no_corpus_text() {
        let ids: Vec<String> = vec!["doc-a:0".into()];
        let sources = ["本を読んでいる"];
        let hashed = workload_id(&ids, &sources, "stride=9", "none");
        assert!(
            !hashed.contains("本") && hashed.chars().all(|c| c.is_ascii_hexdigit()),
            "the workload identity must be a digest and nothing else: {hashed}"
        );
    }

    /// Folding is order-independent, so a checkpoint restored in a different order is still the
    /// same total.
    #[test]
    fn folding_does_not_depend_on_order() {
        let cases = workload();
        let forward = describe(&totals_with_resume(&cases, 0));
        let mut backward_cases = cases.clone();
        backward_cases.reverse();
        let backward = describe(&totals_with_resume(&backward_cases, 0));
        assert_eq!(forward, backward);
    }
}
