//! Lindera and Ichiran against hand-annotated Japanese — **standalone, through the reader's path.**
//!
//! ```text
//! cargo test --release -p sakatsu-core --test standalone_reference -- --ignored --nocapture
//! ```
//!
//! # Why this is not `hybrid_reference.rs`
//!
//! That harness always runs three arms and calls `hybrid::combine` for every sentence. Hybrid is
//! frozen and unwired, and the questions now asked are about **today's Ichiran on the path a reader
//! is served**, with Lindera beside it as a reference arm. So this runs two arms only, each through
//! `study::run` with the production options — analyzer, display pipeline, span contract, grouping —
//! and never touches the Hybrid selector. Its figures are its own; they are not continuous with the
//! three-arm artifacts, whose population was "spans all **three** arms aligned".
//!
//! # The occurrence audit, and why resume records carry it
//!
//! The historical three-arm artifact kept **counts only**: Ichiran 11 right→wrong, 3 wrong→right,
//! 16 differing strings. The occurrences behind those numbers were never recorded, so they cannot
//! be audited — and a current run is a new measurement whose differing set may not be those 16.
//!
//! Here every reading that differs between the arms on a span **both** aligned to one gold morpheme
//! is recorded per case, with each arm's reading, lemma and correctness, where Ichiran's answer
//! first went wrong in its own pipeline, and what an N-best diagnostic observed. **The checkpoint
//! record carries the occurrences**, and a record without them is refused and re-measured — so a
//! fully resumed run cannot reproduce the totals without producing the audit.
//!
//! # N-best: presence, never absence
//!
//! `ichiran-cli -f -l 5` returns up to five segmentations. Finding the gold reading at the span in
//! one of them establishes that Ichiran **had** it and ranked another first. Not finding it
//! establishes only that it was **not observed within five** — not that Ichiran cannot produce it.
//! A failed diagnostic is a third outcome and is never folded into either.
//!
//! # Licence
//!
//! KWDLC declares none; `project notes` §8 treats it as consult-never-import. Readings, lemmas and gold
//! for the audited occurrences go **only** to the ignored private file under `target/evaluation/`;
//! the published summary carries counts, document identifiers, spans and categories.

mod common;

#[path = "common/runner.rs"]
mod runner;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use common::knp::{self, level};
use common::resume::{Identity, Resume, BUDGET};
use runner::{Outcome, Profile, Requirement, Run};
use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Analysis;
use sakatsu_core::study::{self, AnalysisOptions};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The boundary these rows are scored on, and part of every resume key.
const SCORING_METHOD: &str = "standalone-study-run-production-v1";

/// Bumped whenever a record's fields change. A record from another schema is re-measured.
/// 2: occurrences carry Ichiran's raw choice, and the case its raw response.
/// 3: component faults on selectable parts, and parts withheld from selection, per arm.
/// 4: the offered projection is validated on its own terms (`validate_selectable`).
const RECORD_SCHEMA: u64 = 4;

/// How many segmentations the N-best diagnostic asks for.
const NBEST_LIMIT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Arm {
    Lindera,
    Ichiran,
}

impl Arm {
    fn name(self) -> &'static str {
        match self {
            Arm::Lindera => "lindera",
            Arm::Ichiran => "ichiran",
        }
    }

    fn all() -> [Arm; 2] {
        [Arm::Lindera, Arm::Ichiran]
    }
}

/// What the N-best diagnostic observed for one occurrence. Three outcomes, never collapsed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NBest {
    /// Ichiran's own answer was right; there was nothing to look for.
    NotAsked,
    /// The gold reading appeared at the span in the returned segmentation of this rank (1-based).
    Present { rank: usize },
    /// Not in any returned segmentation — which says nothing about Ichiran's full inventory.
    NotObserved { limit: usize, returned: usize },
    /// The diagnostic itself failed; no conclusion either way.
    Unavailable { reason: String },
}

impl NBest {
    fn to_json(&self) -> Value {
        match self {
            NBest::NotAsked => json!({"outcome": "not-asked"}),
            NBest::Present { rank } => json!({"outcome": "present-in-returned", "rank": rank}),
            NBest::NotObserved { limit, returned } => json!({
                "outcome": "not-observed-within-limit", "limit": limit, "returned": returned,
            }),
            NBest::Unavailable { reason } => {
                json!({"outcome": "diagnostic-unavailable", "reason": reason})
            }
        }
    }

    fn from_json(value: &Value) -> Option<NBest> {
        Some(match value["outcome"].as_str()? {
            "not-asked" => NBest::NotAsked,
            "present-in-returned" => NBest::Present {
                rank: value["rank"].as_u64()? as usize,
            },
            "not-observed-within-limit" => NBest::NotObserved {
                limit: value["limit"].as_u64()? as usize,
                returned: value["returned"].as_u64()? as usize,
            },
            "diagnostic-unavailable" => NBest::Unavailable {
                reason: value["reason"].as_str()?.to_string(),
            },
            _ => return None,
        })
    }

    fn label(&self) -> &'static str {
        match self {
            NBest::NotAsked => "not-asked",
            NBest::Present { .. } => "present-in-returned",
            NBest::NotObserved { .. } => "not-observed-within-limit",
            NBest::Unavailable { .. } => "diagnostic-unavailable",
        }
    }
}

/// One arm's answer at an audited span.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Side {
    reading: String,
    right: bool,
    lemma: String,
    lemma_reading: String,
}

impl Side {
    fn to_json(&self) -> Value {
        json!({"reading": self.reading, "right": self.right,
               "lemma": self.lemma, "lemma_reading": self.lemma_reading})
    }

    fn from_json(value: &Value) -> Option<Side> {
        Some(Side {
            reading: value["reading"].as_str()?.to_string(),
            right: value["right"].as_bool()?,
            lemma: value["lemma"].as_str()?.to_string(),
            lemma_reading: value["lemma_reading"].as_str()?.to_string(),
        })
    }
}

/// A reading the two arms gave differently, on a span both aligned to one gold morpheme.
#[derive(Debug, Clone, PartialEq)]
struct Occurrence {
    start: usize,
    end: usize,
    gold: String,
    lindera: Side,
    ichiran: Side,
    /// Where Ichiran's answer first failed to hold, when it did, adjudicated against its **raw**
    /// output: `raw-engine-choice` (the engine chose this reading), `conversion-tie-break` (the
    /// engine tied, the gold reading among the tied, and the adapter's tie-break chose),
    /// `conversion` (the engine chose gold and the converted token does not say so),
    /// `raw-unadjudicable` (no raw word at the span), `display`, or `grouping`.
    ichiran_stage: Option<String>,
    /// The readings Ichiran scored highest for its raw word at the span — two or more on a tie.
    raw_top: Vec<String>,
    /// Every competing reading Ichiran scored for that word, with its score.
    raw_alternatives: Vec<(String, f64)>,
    nbest: NBest,
}

impl Occurrence {
    /// Which way the two arms differ, by correctness.
    fn category(&self) -> &'static str {
        match (self.lindera.right, self.ichiran.right) {
            (true, false) => "ichiran-wrong-lindera-right",
            (false, true) => "ichiran-right-lindera-wrong",
            (false, false) => "both-wrong-differently",
            (true, true) => "both-right-differently",
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "start": self.start, "end": self.end, "gold": self.gold,
            "lindera": self.lindera.to_json(), "ichiran": self.ichiran.to_json(),
            "ichiran_stage": self.ichiran_stage, "nbest": self.nbest.to_json(),
            "raw_top": self.raw_top, "raw_alternatives": self.raw_alternatives,
        })
    }

    fn from_json(value: &Value) -> Option<Occurrence> {
        Some(Occurrence {
            start: value["start"].as_u64()? as usize,
            end: value["end"].as_u64()? as usize,
            gold: value["gold"].as_str()?.to_string(),
            lindera: Side::from_json(&value["lindera"])?,
            ichiran: Side::from_json(&value["ichiran"])?,
            ichiran_stage: value["ichiran_stage"].as_str().map(str::to_string),
            raw_top: value["raw_top"]
                .as_array()?
                .iter()
                .map(|kana| kana.as_str().map(str::to_string))
                .collect::<Option<_>>()?,
            raw_alternatives: value["raw_alternatives"]
                .as_array()?
                .iter()
                .map(|pair| Some((pair[0].as_str()?.to_string(), pair[1].as_f64()?)))
                .collect::<Option<_>>()?,
            nbest: NBest::from_json(&value["nbest"])?,
        })
    }
}

/// Everything one case contributed, in full, so a resumed run reports what an uninterrupted one
/// would — the occurrences included.
#[derive(Debug, Default, Clone, PartialEq)]
struct CaseMetrics {
    /// `(arm, level)` → gold, produced, matched.
    levels: BTreeMap<(String, String), (usize, usize, usize)>,
    /// arm → aligned spans, agreed readings.
    readings: BTreeMap<String, (usize, usize)>,
    gold_morphemes: usize,
    /// arm → why it produced no analysis.
    failures: BTreeMap<String, String>,
    /// Spans both arms aligned to one gold morpheme.
    common_spans: usize,
    /// Over those: Ichiran wrong where Lindera was right, and the reverse.
    became_wrong: usize,
    became_right: usize,
    /// Over those: readings differing as strings, and how many kept the same correctness.
    differing: usize,
    differing_same_class: usize,
    /// arm → component-contract faults in its finished analysis (report-only).
    component_faults: BTreeMap<String, usize>,
    /// arm → those faults on a part the reader may select. Zero is the requirement.
    selectable_faults: BTreeMap<String, usize>,
    /// arm → parts, at any depth, the reader is not offered.
    unselectable_parts: BTreeMap<String, usize>,
    occurrences: Vec<Occurrence>,
    /// Ichiran's raw response, kept **when this case has occurrences to attribute** so the raw
    /// adjudication can be re-derived; its digest travels with it.
    ichiran_raw: Option<String>,
}

impl CaseMetrics {
    fn to_json(&self) -> Value {
        json!({
            "schema": RECORD_SCHEMA,
            "levels": self.levels.iter().map(|((arm, level), (gold, produced, matched))| json!({
                "arm": arm, "level": level, "gold": gold, "produced": produced, "matched": matched,
            })).collect::<Vec<_>>(),
            "readings": self.readings.iter().map(|(arm, (aligned, agreed))| json!({
                "arm": arm, "aligned": aligned, "agreed": agreed,
            })).collect::<Vec<_>>(),
            "gold_morphemes": self.gold_morphemes,
            "failures": self.failures,
            "common_spans": self.common_spans,
            "became_wrong": self.became_wrong,
            "became_right": self.became_right,
            "differing": self.differing,
            "differing_same_class": self.differing_same_class,
            "component_faults": self.component_faults,
            "selectable_faults": self.selectable_faults,
            "unselectable_parts": self.unselectable_parts,
            "occurrences": self.occurrences.iter().map(Occurrence::to_json).collect::<Vec<_>>(),
            "ichiran_raw": self.ichiran_raw,
            "ichiran_raw_sha256": self.ichiran_raw.as_deref()
                .map(|raw| sakatsu_core::hashing::hex(Sha256::digest(raw.as_bytes()))),
        })
    }

    /// **Refuses anything that cannot supply both the totals and the audit.**
    fn from_json(record: &Value) -> Option<CaseMetrics> {
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
        metrics.gold_morphemes = record["gold_morphemes"].as_u64()? as usize;
        for (arm, reason) in record["failures"].as_object()? {
            metrics
                .failures
                .insert(arm.clone(), reason.as_str()?.to_string());
        }
        metrics.common_spans = record["common_spans"].as_u64()? as usize;
        metrics.became_wrong = record["became_wrong"].as_u64()? as usize;
        metrics.became_right = record["became_right"].as_u64()? as usize;
        metrics.differing = record["differing"].as_u64()? as usize;
        metrics.differing_same_class = record["differing_same_class"].as_u64()? as usize;
        for (arm, count) in record["component_faults"].as_object()? {
            metrics
                .component_faults
                .insert(arm.clone(), count.as_u64()? as usize);
        }
        for (arm, count) in record["selectable_faults"].as_object()? {
            metrics
                .selectable_faults
                .insert(arm.clone(), count.as_u64()? as usize);
        }
        for (arm, count) in record["unselectable_parts"].as_object()? {
            metrics
                .unselectable_parts
                .insert(arm.clone(), count.as_u64()? as usize);
        }
        // **The audit is required, not optional.** A record without it could reproduce every total
        // above and still leave the occurrence audit empty.
        for occurrence in record["occurrences"].as_array()? {
            metrics.occurrences.push(Occurrence::from_json(occurrence)?);
        }
        metrics.ichiran_raw = record["ichiran_raw"].as_str().map(str::to_string);
        // A case with occurrences must carry the raw response they were attributed against.
        if !metrics.occurrences.is_empty() && metrics.ichiran_raw.is_none() {
            return None;
        }
        Some(metrics)
    }
}

#[derive(Default)]
struct Totals {
    levels: BTreeMap<(String, String), (usize, usize, usize)>,
    readings: BTreeMap<String, (usize, usize)>,
    gold_morphemes: usize,
    cases: usize,
    failed_cases: BTreeMap<String, usize>,
    common_spans: usize,
    became_wrong: usize,
    became_right: usize,
    differing: usize,
    differing_same_class: usize,
    component_faults: BTreeMap<String, usize>,
    selectable_faults: BTreeMap<String, usize>,
    unselectable_parts: BTreeMap<String, usize>,
    /// `(case id, occurrence)`, in case order.
    occurrences: Vec<(String, Occurrence)>,
}

impl Totals {
    fn fold(&mut self, id: &str, metrics: &CaseMetrics) {
        self.cases += 1;
        for (key, (gold, produced, matched)) in &metrics.levels {
            let entry = self.levels.entry(key.clone()).or_default();
            entry.0 += gold;
            entry.1 += produced;
            entry.2 += matched;
        }
        for (arm, (aligned, agreed)) in &metrics.readings {
            let entry = self.readings.entry(arm.clone()).or_default();
            entry.0 += aligned;
            entry.1 += agreed;
        }
        self.gold_morphemes += metrics.gold_morphemes;
        for arm in metrics.failures.keys() {
            *self.failed_cases.entry(arm.clone()).or_default() += 1;
        }
        self.common_spans += metrics.common_spans;
        self.became_wrong += metrics.became_wrong;
        self.became_right += metrics.became_right;
        self.differing += metrics.differing;
        self.differing_same_class += metrics.differing_same_class;
        for (arm, count) in &metrics.component_faults {
            *self.component_faults.entry(arm.clone()).or_default() += count;
        }
        for (arm, count) in &metrics.selectable_faults {
            *self.selectable_faults.entry(arm.clone()).or_default() += count;
        }
        for (arm, count) in &metrics.unselectable_parts {
            *self.unselectable_parts.entry(arm.clone()).or_default() += count;
        }
        for occurrence in &metrics.occurrences {
            self.occurrences.push((id.to_string(), occurrence.clone()));
        }
    }
}

/// Each aligned span's reading, and whether it matched gold.
type Judged = BTreeMap<(usize, usize), (String, bool)>;

/// The production path, traced — tracing keeps the stages and changes nothing computed
/// (`analysis_path_identity.rs` pins that).
fn production(
    text: &str,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
) -> Result<study::Run, String> {
    study::run(
        text,
        analyzer,
        dictionaries,
        AnalysisOptions::PRODUCTION.traced(),
    )
    .map_err(|failure| failure.error.to_string())
}

fn spans(analysis: &Analysis) -> BTreeSet<(usize, usize)> {
    analysis
        .tokens
        .iter()
        .filter_map(|token| Some((token.start?, token.end?)))
        .collect()
}

fn to_hiragana(text: &str) -> String {
    sakatsu_core::kana::to_hiragana(text)
}

/// The reading an analysis gives at exactly this span, if a token has it.
fn reading_at(analysis: &Analysis, span: (usize, usize)) -> Option<String> {
    analysis
        .tokens
        .iter()
        .find(|token| (token.start, token.end) == (Some(span.0), Some(span.1)))
        .map(|token| to_hiragana(&token.reading))
}

/// Where Ichiran's answer at `span` first stopped being `gold`, **adjudicated against its raw
/// output** — a wrong converted token is where the defect is first present, not proof the
/// conversion caused it.
///
/// Returns the stage and the raw word's top readings and alternatives, for the record.
fn attribute(
    run: &study::Run,
    span: (usize, usize),
    gold: &str,
) -> (Option<String>, Vec<String>, Vec<(String, f64)>) {
    let Some(stages) = run.stages.as_ref() else {
        return (None, Vec::new(), Vec::new());
    };
    let right = |analysis: &Analysis| reading_at(analysis, span).as_deref() == Some(gold);
    let raw = stages.raw.as_deref().and_then(|json| {
        common::reference::raw_words(json, &stages.converted.text)
            .ok()?
            .into_iter()
            .find(|word| (word.start, word.end) == (Some(span.0), Some(span.1)))
    });
    let (top, alternatives) = raw
        .as_ref()
        .map(|word| {
            (
                word.top_readings()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                word.alternatives.clone(),
            )
        })
        .unwrap_or_default();
    let stage = if right(&stages.converted) {
        if right(&stages.displayed) {
            "grouping"
        } else {
            "display"
        }
    } else {
        match &raw {
            None => "raw-unadjudicable",
            Some(_) if top.len() > 1 && top.iter().any(|kana| kana == gold) => {
                "conversion-tie-break"
            }
            Some(_) if top.len() == 1 && top[0] == gold => "conversion",
            Some(_) => "raw-engine-choice",
        }
    };
    (Some(stage.to_string()), top, alternatives)
}

/// **Presence, never absence.** Whether the gold reading sits at `span` in any segmentation the
/// sidecar returned, each converted by the adapter's own conversion.
///
/// Segmentation `k` takes the `k`-th candidate of every analysed segment (or its last, where it has
/// fewer). A span lies inside one segment, so every candidate that segment returned is examined.
fn nbest(ichiran: &IchiranAnalyzer, text: &str, span: (usize, usize), gold: &str) -> NBest {
    match common::reference::segmentations(ichiran, text, NBEST_LIMIT) {
        Err(reason) => NBest::Unavailable { reason },
        Ok(returned) => returned
            .iter()
            .position(|analysis| reading_at(analysis, span).as_deref() == Some(gold))
            .map(|at| NBest::Present { rank: at + 1 })
            .unwrap_or(NBest::NotObserved {
                limit: NBEST_LIMIT,
                returned: returned.len(),
            }),
    }
}

fn side(analysis: &Analysis, span: (usize, usize), gold: &str) -> Side {
    let token = analysis
        .tokens
        .iter()
        .find(|token| (token.start, token.end) == (Some(span.0), Some(span.1)));
    let reading = token
        .map(|token| to_hiragana(&token.reading))
        .unwrap_or_default();
    Side {
        right: reading == gold,
        reading,
        lemma: token
            .map(|token| token.dict_form.clone())
            .unwrap_or_default(),
        lemma_reading: token
            .map(|token| token.lemma_reading.clone())
            .unwrap_or_default(),
    }
}

/// The identity cases are keyed on and artifacts record.
fn identity(dictionaries: &[Dictionary], workload: &str, route: &str) -> Identity {
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
        .with("executable", common::reference::executable_identity())
        .with("source_tree", runner::source_digest())
        .with("dictionaries", fingerprints.join(" | "))
        .with("workload", workload)
        .with("scoring_method", SCORING_METHOD)
        .with("record_schema", RECORD_SCHEMA.to_string())
        .with("ichiran_route", route)
}

/// The source of every selected sentence, the ids in order, and how they were chosen.
fn workload_id(ids: &[String], sources: &[&str], sampling: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"standalone-workload-v1");
    hasher.update(SCORING_METHOD.as_bytes());
    hasher.update(sampling.as_bytes());
    hasher.update(ids.len().to_le_bytes());
    for (id, source) in ids.iter().zip(sources) {
        hasher.update(id.as_bytes());
        hasher.update([0]);
        hasher.update(source.as_bytes());
        hasher.update([0]);
    }
    sakatsu_core::hashing::hex(hasher.finalize())
}

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

fn evaluation_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation")
}

/// Publish under a name derived from the content, atomically and never over differing bytes.
fn publish(stem: &str, extension: &str, contents: &str) -> std::path::PathBuf {
    common::reference::publish(&evaluation_dir(), stem, extension, contents)
        .unwrap_or_else(|error| panic!("{stem} could not be published: {error}"))
}

fn level_table(totals: &Totals, levels: &[&str]) {
    for level in levels {
        println!("\n  {level}");
        println!("    arm         gold  produced   matched       P       R      F1");
        for arm in Arm::all() {
            let Some((gold, produced, matched)) = totals
                .levels
                .get(&(arm.name().to_string(), level.to_string()))
            else {
                continue;
            };
            let p = if *produced == 0 {
                0.0
            } else {
                *matched as f64 / *produced as f64
            };
            let r = if *gold == 0 {
                0.0
            } else {
                *matched as f64 / *gold as f64
            };
            let f = if p + r == 0.0 {
                0.0
            } else {
                2.0 * p * r / (p + r)
            };
            println!(
                "    {:<10} {gold:>6} {produced:>9} {matched:>9}  {:>5.1}%  {:>5.1}%  {:>6.1}",
                arm.name(),
                p * 100.0,
                r * 100.0,
                f * 100.0
            );
        }
    }
}

/// **KWDLC, both arms on the reader's path, with every differing common-span reading audited.**
#[test]
#[ignore = "needs scripts/fetch-kwdlc.ps1, the dictionaries and Ichiran"]
fn standalone_against_kwdlc() {
    let started = Instant::now();
    let dictionaries = common::installed_dictionaries();
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let (ichiran, session, route) = ichiran_on_the_session_route();

    let evaluation = evaluation_dir();
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
    // Excluded before their annotations are loaded, as the three-arm harness does.
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
    let sentences = match knp::load_files(&development, walk_faults) {
        Ok(sentences) => sentences,
        Err(damaged) => panic!("{damaged}"),
    };

    // **The same selection the three-arm harness makes**, so the sentence population is the
    // historical one even though the scoring population is not.
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
    let sampled = &strided[..cap];
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
        &workload_id(&ids, &sources, &sampling),
        &route,
    )
    .with("reservation", audit.clone());
    let resume = Resume::open(
        evaluation.join("standalone-reference-kwdlc-resume.json"),
        key.clone(),
    );
    if let Some(reason) = resume.refused() {
        println!("RESUME REFUSED: {reason}");
    }
    if let Some(reason) = resume.preservation_failed() {
        panic!("the previous checkpoint could not be preserved: {reason}");
    }

    let mut run = Run::new(Profile::InstalledResources, ids.iter().cloned())
        .engine("lindera, ichiran (standalone)")
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
    let (mut reused, mut refused_records) = (0usize, 0usize);

    for (id, sentence) in ids.iter().zip(sampled) {
        if let Some(record) = completed.get(id) {
            match CaseMetrics::from_json(record) {
                Some(metrics) => {
                    totals.fold(id, &metrics);
                    run.record(
                        id.clone(),
                        if metrics.failures.is_empty() {
                            Outcome::Passed
                        } else {
                            Outcome::failed(runner::Stage::Analyzer, "an arm produced no analysis")
                        },
                    );
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
                Outcome::incomplete("the time budget was exhausted; run again to continue"),
            );
            continue;
        }

        let lindera_run = production(&sentence.text, &lindera, &dictionaries);
        let ichiran_run = production(&sentence.text, &ichiran, &dictionaries);
        let mut metrics = CaseMetrics {
            gold_morphemes: sentence.morphemes.len(),
            ..CaseMetrics::default()
        };
        for (arm, outcome) in [(Arm::Lindera, &lindera_run), (Arm::Ichiran, &ichiran_run)] {
            if let Err(error) = outcome {
                metrics
                    .failures
                    .insert(arm.name().to_string(), error.clone());
            }
        }
        let (Ok(lindera_run), Ok(ichiran_run)) = (lindera_run, ichiran_run) else {
            // **Paired or not at all**: an arm that failed must not drop its gold from its own
            // denominator while the other keeps it.
            totals.fold(id, &metrics);
            completed.insert(id.clone(), metrics.to_json());
            run.record(
                id.clone(),
                Outcome::failed(runner::Stage::Analyzer, "an arm produced no analysis"),
            );
            continue;
        };

        let gold_levels: [(&str, BTreeSet<(usize, usize)>); 3] = [
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

        let mut judged: BTreeMap<Arm, Judged> = BTreeMap::new();
        for (arm, finished) in [(Arm::Lindera, &lindera_run), (Arm::Ichiran, &ichiran_run)] {
            let analysis = &finished.analysis;
            let produced = spans(analysis);
            for (name, reference) in &gold_levels {
                metrics.levels.insert(
                    (arm.name().to_string(), name.to_string()),
                    (
                        reference.len(),
                        produced.len(),
                        produced.intersection(reference).count(),
                    ),
                );
            }
            let mut by = BTreeMap::new();
            let (mut aligned, mut agreed) = (0usize, 0usize);
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
                agreed += usize::from(right);
                by.insert((start, end), (ours, right));
            }
            metrics
                .readings
                .insert(arm.name().to_string(), (aligned, agreed));
            let faults = sakatsu_core::analyzer::validate_components(analysis);
            metrics
                .component_faults
                .insert(arm.name().to_string(), faults.len());
            metrics.selectable_faults.insert(
                arm.name().to_string(),
                sakatsu_core::analyzer::validate_selectable(analysis).len(),
            );
            metrics.unselectable_parts.insert(
                arm.name().to_string(),
                common::reference::unselectable_parts(analysis),
            );
            judged.insert(arm, by);
        }

        // **Spans both arms aligned** — the only population where a differing reading is about the
        // same word.
        let lindera_judged = &judged[&Arm::Lindera];
        let ichiran_judged = &judged[&Arm::Ichiran];
        for (span, (ours, we_are_right)) in lindera_judged {
            let Some((theirs, they_are_right)) = ichiran_judged.get(span) else {
                continue;
            };
            metrics.common_spans += 1;
            match (we_are_right, they_are_right) {
                (true, false) => metrics.became_wrong += 1,
                (false, true) => metrics.became_right += 1,
                _ => {}
            }
            if ours == theirs {
                continue;
            }
            metrics.differing += 1;
            if we_are_right == they_are_right {
                metrics.differing_same_class += 1;
            }
            let gold = to_hiragana(&by_span[span].reading);
            let ichiran_side = side(&ichiran_run.analysis, *span, &gold);
            let (stage, raw_top, raw_alternatives) = if ichiran_side.right {
                (None, Vec::new(), Vec::new())
            } else {
                attribute(&ichiran_run, *span, &gold)
            };
            let nbest = if ichiran_side.right {
                NBest::NotAsked
            } else {
                nbest(&ichiran, &sentence.text, *span, &gold)
            };
            metrics.occurrences.push(Occurrence {
                start: span.0,
                end: span.1,
                lindera: side(&lindera_run.analysis, *span, &gold),
                ichiran: ichiran_side,
                ichiran_stage: stage,
                raw_top,
                raw_alternatives,
                nbest,
                gold,
            });
        }
        if !metrics.occurrences.is_empty() {
            metrics.ichiran_raw = ichiran_run
                .stages
                .as_ref()
                .and_then(|stages| stages.raw.clone());
        }

        totals.fold(id, &metrics);
        completed.insert(id.clone(), metrics.to_json());
        run.record(id.clone(), Outcome::Passed);
    }

    if refused_records > 0 {
        println!(
            "  {refused_records} checkpoint records could not supply the totals and the audit \
             and were re-measured"
        );
    }
    if reused > 0 {
        println!(
            "  {reused} cases restored from the checkpoint, with their counts and occurrences"
        );
    }
    if let Err(error) = resume.save(&completed) {
        println!("  WARNING: the checkpoint was not saved: {error}");
    }

    println!("\n=== KWDLC, standalone (study::run, production options) ===");
    level_table(
        &totals,
        &["morpheme", "基本句 (basic phrase)", "文節 (bunsetsu)"],
    );
    println!("\n  readings, where a span matched a gold morpheme exactly");
    for arm in Arm::all() {
        let (aligned, agreed) = totals.readings.get(arm.name()).copied().unwrap_or_default();
        println!(
            "    {:<10} aligned {aligned:>6}  agreed {agreed:>6}  accuracy {:>5.1}%  coverage {:>5.1}%",
            arm.name(),
            if aligned == 0 { 0.0 } else { agreed as f64 * 100.0 / aligned as f64 },
            if totals.gold_morphemes == 0 { 0.0 } else { aligned as f64 * 100.0 / totals.gold_morphemes as f64 },
        );
    }
    let mut categories: BTreeMap<&str, usize> = BTreeMap::new();
    let mut nbest_outcomes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut stages: BTreeMap<String, usize> = BTreeMap::new();
    for (_, occurrence) in &totals.occurrences {
        *categories.entry(occurrence.category()).or_default() += 1;
        *nbest_outcomes.entry(occurrence.nbest.label()).or_default() += 1;
        if let Some(stage) = &occurrence.ichiran_stage {
            *stages.entry(stage.clone()).or_default() += 1;
        }
    }
    println!(
        "\n  common spans {}  |  ichiran wrong where lindera right {}  |  right where lindera wrong {}",
        totals.common_spans, totals.became_wrong, totals.became_right
    );
    println!(
        "  differing strings {} (same correctness {})  — the historical three-arm count was 16 over a \
         different population; these are a current set, not those 16",
        totals.differing, totals.differing_same_class
    );
    println!("  by category {categories:?}");
    println!("  ichiran's first wrong stage {stages:?}");
    println!("  n-best (limit {NBEST_LIMIT}) {nbest_outcomes:?}");
    println!(
        "  component-contract faults (report-only) {:?}",
        totals.component_faults
    );
    println!(
        "    of which on selectable parts {:?}",
        totals.selectable_faults
    );
    println!(
        "  parts withheld from selection {:?}",
        totals.unselectable_parts
    );
    println!("  cases with a failed arm {:?}", totals.failed_cases);

    // Published: counts, identifiers, spans, categories. No reading, lemma or gold.
    let summary = json!({
        "what_this_is": "Lindera and Ichiran against KWDLC, each through study::run with the \
                         production options. Standalone: no Hybrid arm, no hybrid::combine.",
        "identity": key.describe(),
        "cases": totals.cases,
        "levels": totals.levels.iter().map(|((arm, level), (gold, produced, matched))| json!({
            "arm": arm, "level": level, "gold": gold, "produced": produced, "matched": matched,
        })).collect::<Vec<_>>(),
        "readings": totals.readings.iter().map(|(arm, (aligned, agreed))| json!({
            "arm": arm, "aligned": aligned, "agreed": agreed,
        })).collect::<Vec<_>>(),
        "gold_morphemes": totals.gold_morphemes,
        "cases_with_a_failed_arm": totals.failed_cases,
        "common_spans": totals.common_spans,
        "ichiran_wrong_where_lindera_right": totals.became_wrong,
        "ichiran_right_where_lindera_wrong": totals.became_right,
        "differing_strings": totals.differing,
        "differing_same_correctness": totals.differing_same_class,
        "by_category": categories,
        "ichiran_first_wrong_stage": stages,
        "nbest": nbest_outcomes,
        "component_contract_faults_report_only": totals.component_faults,
        "component_faults_on_selectable_parts": totals.selectable_faults,
        "parts_withheld_from_selection": totals.unselectable_parts,
        "occurrences": totals.occurrences.iter().map(|(id, occurrence)| json!({
            "case": id, "span": [occurrence.start, occurrence.end],
            "category": occurrence.category(),
            "ichiran_stage": occurrence.ichiran_stage,
            "nbest": occurrence.nbest.label(),
        })).collect::<Vec<_>>(),
        "historical": "hybrid-reference-kwdlc-f6383a2e1d9fbf19.json recorded 16 differing strings \
                       over spans all THREE arms aligned; that set was never recorded and is not \
                       this one",
    });
    let summary_path = publish(
        "standalone-reference-kwdlc",
        "json",
        &serde_json::to_string_pretty(&summary).expect("serializes"),
    );
    // Private: the readings, lemmas and gold for each audited occurrence.
    let mut private = String::new();
    for (id, occurrence) in &totals.occurrences {
        let mut record = occurrence.to_json();
        record["case"] = json!(id);
        record["category"] = json!(occurrence.category());
        private.push_str(&record.to_string());
        private.push('\n');
    }
    let private_path = publish("standalone-reference-kwdlc-occurrences", "jsonl", &private);
    println!("  wrote {}", summary_path.display());
    println!(
        "  wrote {} (private: readings and gold)",
        private_path.display()
    );
    run.accounted_for().expect_success();
}

/// **UD_Japanese-GSD, both arms on the reader's path: boundaries only.** GSD's readings are UniDic's,
/// the built-in analyzer's own dictionary, so they are not scored here either.
#[test]
#[ignore = "needs scripts/fetch-ud-japanese.ps1, the dictionaries and Ichiran"]
fn standalone_against_ud_japanese_gsd() {
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
    let sources: Vec<&str> = sampled
        .iter()
        .map(|sentence| sentence.text.as_str())
        .collect();
    let sampling = format!("stride over test+dev, SAKATSU_UD_ICHIRAN={cap}, stride={stride}");
    let key = identity(
        &dictionaries,
        &workload_id(&ids, &sources, &sampling),
        &route,
    );
    let evaluation = evaluation_dir();
    let resume = Resume::open(
        evaluation.join("standalone-reference-ud-resume.json"),
        key.clone(),
    );
    if let Some(reason) = resume.refused() {
        println!("RESUME REFUSED: {reason}");
    }
    if let Some(reason) = resume.preservation_failed() {
        panic!("the previous checkpoint could not be preserved: {reason}");
    }

    let mut run = Run::new(Profile::InstalledResources, ids.iter().cloned())
        .engine("lindera, ichiran (standalone)")
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
    let mut completed = resume.completed().clone();
    for sentence in &sampled {
        if let Some(record) = completed.get(&sentence.id) {
            if let Some(metrics) = CaseMetrics::from_json(record) {
                totals.fold(&sentence.id, &metrics);
                run.record(sentence.id.clone(), Outcome::Passed);
                continue;
            }
            completed.remove(&sentence.id);
        }
        if started.elapsed() > BUDGET {
            run.record(
                sentence.id.clone(),
                Outcome::incomplete("the time budget was exhausted; run again to continue"),
            );
            continue;
        }
        let short: BTreeSet<(usize, usize)> = sentence
            .tokens
            .iter()
            .map(|token| (token.start, token.end))
            .collect();
        let long: BTreeSet<(usize, usize)> = common::gold::long_units(&sentence.tokens)
            .into_iter()
            .collect();
        let mut metrics = CaseMetrics::default();
        let runs = [
            (
                Arm::Lindera,
                production(&sentence.text, &lindera, &dictionaries),
            ),
            (
                Arm::Ichiran,
                production(&sentence.text, &ichiran, &dictionaries),
            ),
        ];
        let paired = runs.iter().all(|(_, outcome)| outcome.is_ok());
        for (arm, outcome) in &runs {
            match outcome {
                Err(error) => {
                    metrics
                        .failures
                        .insert(arm.name().to_string(), error.clone());
                }
                Ok(finished) if paired => {
                    let produced = spans(&finished.analysis);
                    for (name, reference) in
                        [("short unit (SUW)", &short), ("long unit (LUW)", &long)]
                    {
                        metrics.levels.insert(
                            (arm.name().to_string(), name.to_string()),
                            (
                                reference.len(),
                                produced.len(),
                                produced.intersection(reference).count(),
                            ),
                        );
                    }
                    let faults = sakatsu_core::analyzer::validate_components(&finished.analysis);
                    metrics
                        .component_faults
                        .insert(arm.name().to_string(), faults.len());
                    metrics.selectable_faults.insert(
                        arm.name().to_string(),
                        sakatsu_core::analyzer::validate_selectable(&finished.analysis).len(),
                    );
                    metrics.unselectable_parts.insert(
                        arm.name().to_string(),
                        common::reference::unselectable_parts(&finished.analysis),
                    );
                }
                Ok(_) => {}
            }
        }
        totals.fold(&sentence.id, &metrics);
        completed.insert(sentence.id.clone(), metrics.to_json());
        run.record(
            sentence.id.clone(),
            if paired {
                Outcome::Passed
            } else {
                Outcome::failed(runner::Stage::Analyzer, "an arm produced no analysis")
            },
        );
    }
    if let Err(error) = resume.save(&completed) {
        println!("  WARNING: the checkpoint was not saved: {error}");
    }
    println!("\n=== UD_Japanese-GSD, standalone (study::run, production options) ===");
    level_table(&totals, &["short unit (SUW)", "long unit (LUW)"]);
    println!(
        "  component-contract faults (report-only) {:?}",
        totals.component_faults
    );
    println!(
        "    of which on selectable parts {:?}",
        totals.selectable_faults
    );
    println!(
        "  parts withheld from selection {:?}",
        totals.unselectable_parts
    );
    println!("  cases with a failed arm {:?}", totals.failed_cases);
    let summary = json!({
        "what_this_is": "Lindera and Ichiran against UD_Japanese-GSD, each through study::run with \
                         the production options. Boundaries only.",
        "identity": key.describe(),
        "cases": totals.cases,
        "levels": totals.levels.iter().map(|((arm, level), (gold, produced, matched))| json!({
            "arm": arm, "level": level, "gold": gold, "produced": produced, "matched": matched,
        })).collect::<Vec<_>>(),
        "cases_with_a_failed_arm": totals.failed_cases,
        "component_contract_faults_report_only": totals.component_faults,
        "component_faults_on_selectable_parts": totals.selectable_faults,
        "parts_withheld_from_selection": totals.unselectable_parts,
    });
    let path = publish(
        "standalone-reference-ud",
        "json",
        &serde_json::to_string_pretty(&summary).expect("serializes"),
    );
    println!("  wrote {}", path.display());
    run.accounted_for().expect_success();
}

#[cfg(test)]
mod records {
    use super::*;

    fn occurrence(nbest: NBest) -> Occurrence {
        Occurrence {
            start: 3,
            end: 9,
            gold: "もの".into(),
            lindera: Side {
                reading: "もの".into(),
                right: true,
                lemma: "者".into(),
                lemma_reading: "もの".into(),
            },
            ichiran: Side {
                reading: "しゃ".into(),
                right: false,
                lemma: "者".into(),
                lemma_reading: "しゃ".into(),
            },
            ichiran_stage: Some("conversion-tie-break".into()),
            raw_top: vec!["しゃ".into(), "もの".into()],
            raw_alternatives: vec![("しゃ".into(), 16.0), ("もの".into(), 16.0)],
            nbest,
        }
    }

    fn case() -> CaseMetrics {
        let mut metrics = CaseMetrics {
            gold_morphemes: 12,
            common_spans: 9,
            became_wrong: 1,
            differing: 2,
            differing_same_class: 1,
            ..CaseMetrics::default()
        };
        metrics
            .levels
            .insert(("ichiran".into(), "morpheme".into()), (12, 10, 8));
        metrics.readings.insert("lindera".into(), (9, 8));
        metrics.component_faults.insert("ichiran".into(), 0);
        metrics.occurrences = vec![
            occurrence(NBest::Present { rank: 2 }),
            occurrence(NBest::NotObserved {
                limit: 5,
                returned: 5,
            }),
            occurrence(NBest::Unavailable {
                reason: "timed out".into(),
            }),
            occurrence(NBest::NotAsked),
        ];
        metrics.ichiran_raw = Some("[]".into());
        metrics
    }

    /// Every field — the occurrences and all three N-best outcomes included — survives the round
    /// trip a checkpoint puts it through.
    #[test]
    fn a_record_round_trips_with_its_audit() {
        let metrics = case();
        let restored = CaseMetrics::from_json(&metrics.to_json()).expect("readable");
        assert_eq!(restored, metrics);
    }

    /// **A record that cannot supply the audit is refused**, even when it can supply every total —
    /// or a fully resumed run would publish the numbers with no occurrences behind them.
    #[test]
    fn a_record_without_occurrences_is_refused() {
        let mut record = case().to_json();
        record
            .as_object_mut()
            .expect("an object")
            .remove("occurrences");
        assert!(CaseMetrics::from_json(&record).is_none());

        let mut older = case().to_json();
        older["schema"] = json!(RECORD_SCHEMA + 1);
        assert!(CaseMetrics::from_json(&older).is_none());

        // Occurrences attributed against a raw response that was not kept cannot be re-derived.
        let mut unrecorded = case().to_json();
        unrecorded["ichiran_raw"] = serde_json::Value::Null;
        assert!(CaseMetrics::from_json(&unrecorded).is_none());
    }

    /// "Not observed within the limit" is its own outcome and never reads as present or as absent.
    #[test]
    fn nbest_outcomes_keep_their_three_meanings() {
        let labels: BTreeSet<&str> = case()
            .occurrences
            .iter()
            .map(|occurrence| occurrence.nbest.label())
            .collect();
        assert_eq!(
            labels,
            BTreeSet::from([
                "present-in-returned",
                "not-observed-within-limit",
                "diagnostic-unavailable",
                "not-asked",
            ])
        );
        assert!(!labels.iter().any(|label| label.contains("absent")));
    }
}
