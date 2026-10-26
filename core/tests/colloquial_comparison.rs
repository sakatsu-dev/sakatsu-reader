//! Where the two analyzers disagree on colloquial Japanese, and which pass introduced it.
//!
//! ```text
//! cargo test --release -p sakatsu-core --test colloquial_comparison -- --ignored --nocapture
//! ```
//!
//! # What this is and is not
//!
//! It is a **measurement of disagreement**, not of correctness. Nothing here knows which analysis
//! is right. Two engines differing tells you there is a question to answer; it does not answer it,
//! and this file never pretends otherwise — a disagreement is recorded as a disagreement, with the
//! evidence needed to review it, and the correctness judgment is left to the review queue.
//!
//! Three things are kept apart throughout, because collapsing them is how a measurement starts
//! reporting the wrong quantity:
//!
//! - **availability** — a dictionary had an entry;
//! - **agreement** — the two engines produced the same span or the same reading;
//! - **correctness** — the analysis is right about Japanese.
//!
//! Only the first two are computed here.
//!
//! # Capture evidence and chat discoveries are never pooled
//!
//! The captures are the domain the product is measured on. The chat corpora are where colloquial
//! examples are plentiful, and they are typed conversation, not manga panels or game script. A
//! finding in one is not evidence about the other, so every count is reported per corpus and the
//! two are never summed.
//!
//! The chat sample is also **equally weighted between its two sources while the pool is 80/20**, so
//! a figure over the whole chat sample is an equally weighted diagnostic and not an estimate of the
//! pooled corpus. Per-source figures are the ones that mean anything.
//!
//! # Attribution is a hint, and `analyzer` does not mean the tokenizer
//!
//! **This is the correction that matters most about reading these results.**
//!
//! `LinderaAnalyzer::analyze` returns
//! `attach_terminal_kke(rejoin_fixed_expressions(compose(pieces)))`. So what this file calls
//! `lindera-raw` is the analyzer's **API output**, which already contains this project's own
//! grouping logic — and a disagreement labelled [`Stage::Analyzer`] means only *present by the time
//! the analyzer returned*. It does **not** distinguish the UniDic tokenizer from the composition
//! built on top of it, and an earlier version of this comparison was read as exonerating the very
//! composition it was measuring.
//!
//! The trace that does separate them is
//! `analyzer::lindera::tests::each_stage_is_attributed_for_the_contested_groupings`, which walks
//! representative cases through tokenizer → `compose` → `rejoin_fixed_expressions` →
//! `attach_terminal_kke`. It establishes, for example, that the tokenizer emits `ん | です` —
//! agreeing with Ichiran — and that `compose` is what joins them.
//!
//! | Where the difference already exists | Stage | Means |
//! |---|---|---|
//! | in the analyzer's API output | [`Stage::Analyzer`] | tokenizer **or** our composition |
//! | introduced by the name pass | [`Stage::Names`] | inferred, see below |
//! | introduced by the compound pass | [`Stage::Compounds`] | inferred, see below |
//! | introduced by the reading pass | [`Stage::Readings`] | |
//! | none of the above can be established | [`Stage::Unknown`] | |
//!
//! Names and compounds are separated by **running the name pass standalone** over the analyzer's
//! tokens rather than by observing the production passes in place, so those two labels are
//! inferences about which pass would produce the observed spans. Anything the rule cannot place is
//! `Unknown`, which the runner supports precisely so a guessed attribution never enters the record.
//!
//! Every disagreement record carries `stage_is_a_hint: true` for this reason.
//!
//! # Holdout exposure, recorded rather than repaired
//!
//! The first run of this comparison swept every promoted capture transcript, and all seven captures
//! referenced by `corpus/reading-gold.json` — whose 23 cases are all `split: holdout` — have one.
//! So holdout material was read. Reseeding would not undo that, so the cases are kept, labelled
//! `partition: "holdout"`, reported separately, and are ineligible for the development review queue.
//!
//! # Why the session route is required
//!
//! Ichiran served one process per passage is a different measurement from Ichiran served by a live
//! session — ten times the latency, and a different code path in `ichiran::server`. Mixing the two
//! inside one run produces an average of two things. `analyzer_cold_start.rs` already refuses this
//! and the refusal is copied here rather than re-argued.

mod common;

#[path = "common/runner.rs"]
mod runner;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{dictionaries_dir, installed_dictionaries};
use runner::{Outcome, Profile, Requirement, Run, Span, Stage};
use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::{Analysis, Token};
use sakatsu_core::pipeline::Pipeline;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// One invocation's wall-clock allowance. Exceeding it leaves the remaining cases *incomplete*,
/// which fails the run — a partial measurement must never read as a finished one — and [`Resume`]
/// lets the next invocation finish them.
const BUDGET: Duration = Duration::from_secs(60 * 60);

/// How many targets additionally get analysed inside a larger block of their own source.
const CONTEXT_TARGETS: usize = 24;

/// The most source a context block may carry, in characters.
const CONTEXT_BLOCK: usize = 400;

fn scratch_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("scratch/colloquial")
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("corpus")
}

fn evaluation() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join("target/evaluation")
}

fn hex(bytes: &[u8]) -> String {
    sakatsu_core::hashing::hex(Sha256::digest(bytes))
}

/// Completed cases from an earlier invocation, reusable **only** under a matching identity.
///
/// The doc comment above `BUDGET` used to promise this while nothing implemented it, so a run that
/// exhausted its budget simply failed and the next one started from nothing.
///
/// **What makes a resume safe is the identity, not the file.** Reusing a case measured by a
/// different binary, against different dictionaries, over a different sample, or through a
/// different Ichiran route would silently mix two measurements into one report — and the resulting
/// artifact would name only the second. So every one of those is part of the key, a mismatch
/// refuses rather than resuming, and the refusal says which field differed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    executable: String,
    source_tree: String,
    freeze_id: String,
    dictionaries: Vec<String>,
    route: String,
}

impl Identity {
    fn describe(&self) -> Value {
        json!({
            "executable": self.executable,
            "source_tree": self.source_tree,
            "freeze_id": self.freeze_id,
            "dictionaries": self.dictionaries,
            "ichiran_route": self.route,
        })
    }

    /// Which fields differ, named individually so a refusal is diagnosable.
    fn differences(&self, other: &Identity) -> Vec<&'static str> {
        let mut differences = Vec::new();
        if self.executable != other.executable {
            differences.push("executable");
        }
        if self.source_tree != other.source_tree {
            differences.push("source tree");
        }
        if self.freeze_id != other.freeze_id {
            differences.push("freeze");
        }
        if self.dictionaries != other.dictionaries {
            differences.push("dictionaries");
        }
        if self.route != other.route {
            differences.push("ichiran route");
        }
        differences
    }
}

/// Per-case results carried between invocations.
struct Resume {
    path: PathBuf,
    identity: Identity,
    completed: BTreeMap<String, Value>,
    /// Why a resume was refused, when it was.
    refused: Option<String>,
}

impl Resume {
    fn open(path: PathBuf, identity: Identity) -> Resume {
        let mut resume = Resume {
            path,
            identity,
            completed: BTreeMap::new(),
            refused: None,
        };
        let Ok(raw) = std::fs::read(&resume.path) else {
            return resume;
        };
        let Ok(document) = serde_json::from_slice::<Value>(&raw) else {
            resume.refused = Some("the resume file could not be parsed".into());
            return resume;
        };
        let recorded = Identity {
            executable: document["identity"]["executable"]
                .as_str()
                .unwrap_or_default()
                .into(),
            source_tree: document["identity"]["source_tree"]
                .as_str()
                .unwrap_or_default()
                .into(),
            freeze_id: document["identity"]["freeze_id"]
                .as_str()
                .unwrap_or_default()
                .into(),
            dictionaries: document["identity"]["dictionaries"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            route: document["identity"]["ichiran_route"]
                .as_str()
                .unwrap_or_default()
                .into(),
        };
        let differences = resume.identity.differences(&recorded);
        if !differences.is_empty() {
            resume.refused = Some(format!(
                "the recorded run differs in: {}. Reusing it would mix two measurements into one \
                 report, so it is discarded rather than resumed",
                differences.join(", ")
            ));
            return resume;
        }
        if let Some(cases) = document["cases"].as_object() {
            for (id, record) in cases {
                resume.completed.insert(id.clone(), record.clone());
            }
        }
        resume
    }

    fn save(&self, cases: &BTreeMap<String, Value>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let document = json!({
            "identity": self.identity.describe(),
            "note": "reusable only under an identical identity; see Resume",
            "cases": cases,
        });
        let body = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("could not serialise the resume file: {error}"))?;
        std::fs::write(&self.path, body)
            .map_err(|error| format!("could not write {}: {error}", self.path.display()))
    }
}

/// Which corpus a case came from. **Never pooled**, so it is part of the case's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Corpus {
    Capture,
    Chat,
}

impl Corpus {
    fn slug(self) -> &'static str {
        match self {
            Corpus::Capture => "capture",
            Corpus::Chat => "chat",
        }
    }
}

#[derive(Debug, Clone)]
struct Case {
    id: String,
    corpus: Corpus,
    /// The chat source, or the capturing application. Reported separately; never summed across.
    source: String,
    original: String,
    family: Option<String>,
    stratum: String,
    /// Which side of its corpus's partition this case sits on. See the module docstring.
    partition: &'static str,
}

/// The frozen 240, read from the manifest's own sample file.
///
/// **The freeze is checked, not assumed.** If `chat-freeze.json` and `chat-sample.jsonl` disagree
/// about what was frozen, this run is measuring something other than the frozen sample and says so
/// rather than proceeding.
fn chat_cases() -> Result<(Vec<Case>, String), String> {
    let dir = scratch_dir();
    let manifest_path = dir.join("chat-freeze.json");
    let sample_path = dir.join("chat-sample.jsonl");
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(&manifest_path)
            .map_err(|error| format!("{}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("{}: {error}", manifest_path.display()))?;

    let raw = std::fs::read_to_string(&sample_path)
        .map_err(|error| format!("{}: {error}", sample_path.display()))?;

    let mut cases = Vec::new();
    let mut ids = Vec::new();
    let mut content = Vec::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let record: Value =
            serde_json::from_str(line).map_err(|error| format!("sample line: {error}"))?;
        let id = record["case_id"].as_str().unwrap_or_default().to_string();
        let digest = record["sha256"].as_str().unwrap_or_default().to_string();
        ids.push(id.clone());
        content.push(format!("{id}\u{1f}{digest}"));
        cases.push(Case {
            id: format!("chat/{id}"),
            corpus: Corpus::Chat,
            source: record["source"].as_str().unwrap_or_default().to_string(),
            original: record["text"].as_str().unwrap_or_default().to_string(),
            family: record["family"].as_str().map(str::to_string),
            stratum: record["stratum"].as_str().unwrap_or("?").to_string(),
            // Drawn from the development side by construction; the freeze is the record of that
            // and the digest check below enforces it.
            partition: "development",
        });
    }

    let sample = &manifest["sample"];
    for (field, computed) in [
        ("case_ids_sha256", hex(ids.join("\n").as_bytes())),
        ("content_sha256", hex(content.join("\n").as_bytes())),
    ] {
        let frozen = sample[field].as_str().unwrap_or_default();
        if frozen != computed {
            return Err(format!(
                "the sample file does not match the freeze ({field}: frozen {frozen}, computed \
                 {computed}). This run would be measuring a different selection from the one that \
                 was frozen before any engine ran, which is the one thing freezing exists to \
                 prevent."
            ));
        }
    }

    let freeze_id = manifest["freeze_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Ok((cases, freeze_id))
}

/// Which capture stems the existing reading holdout is built on.
///
/// `corpus/reading-gold.json` names a `source_image` per case and **all 23 of its cases are
/// `split: holdout`**. Every one of the seven images it references also has a promoted `.txt`, so
/// a sweep of promoted transcripts picks them all up. Read here so they can be labelled, not so
/// they can be quietly dropped.
fn reading_holdout_stems() -> BTreeSet<String> {
    let path = corpus().join("reading-gold.json");
    let Ok(raw) = std::fs::read(&path) else {
        return BTreeSet::new();
    };
    let Ok(document) = serde_json::from_slice::<Value>(&raw) else {
        return BTreeSet::new();
    };
    document["cases"]
        .as_array()
        .map(|cases| {
            cases
                .iter()
                .filter_map(|case| case["source_image"].as_str())
                .filter_map(|image| Path::new(image).file_stem())
                .map(|stem| stem.to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The promoted capture transcripts: `.txt`, never a `.mine` draft.
///
/// Two `.txt` files in `corpus/` are not captures and are excluded by name rather than by pattern
/// luck — `capture-inventory.py` documents why.
///
/// **Holdout captures are kept and labelled, not removed.** They have already been read once; a
/// later run pretending otherwise would misrepresent the record.
fn capture_cases() -> Vec<Case> {
    const NOT_CAPTURES: [&str; 2] = ["ocr-to-text-corpus.txt", "run-2026-08-25.txt"];
    let root = corpus();
    let holdout = reading_holdout_stems();
    let mut cases = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if path.extension().and_then(|e| e.to_str()) != Some("txt")
                || NOT_CAPTURES.contains(&name.as_str())
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            // `nexon_client_abc` splits on the LAST underscore: the application name may itself
            // contain one, and splitting on the first mis-groups the families.
            let application = stem
                .rsplit_once('_')
                .map(|(app, _)| app)
                .unwrap_or("?")
                .to_string();
            let partition = if holdout.contains(&stem) {
                "holdout"
            } else {
                "development"
            };
            cases.push(Case {
                id: format!("capture/{stem}"),
                corpus: Corpus::Capture,
                source: application,
                original: text,
                family: None,
                stratum: "promoted-transcript".into(),
                partition,
            });
        }
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    cases
}

/// What an engine's spans cover, and how they fail to, kept apart so one cannot stand for another.
#[derive(Debug, Default, Clone)]
struct Coverage {
    /// Offsets that cannot be believed. No comparison is possible.
    invalid: Vec<String>,
    /// Well-formed offsets whose surface is not the source slice. No comparison is possible.
    mismatched: Vec<String>,
    /// Byte ranges of the source that no span claims, including any trailing omission.
    /// **A finding, not a disqualification.**
    uncovered: Vec<(usize, usize)>,
}

impl Coverage {
    /// Whether the offsets themselves can be trusted. Uncovered source does not affect this.
    fn alignable(&self) -> bool {
        self.invalid.is_empty() && self.mismatched.is_empty()
    }

    fn uncovered_bytes(&self) -> usize {
        self.uncovered.iter().map(|(start, end)| end - start).sum()
    }

    fn why(&self) -> String {
        self.invalid
            .iter()
            .chain(self.mismatched.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// One analysis reduced to what the comparison needs.
#[derive(Debug, Clone)]
struct Shape {
    engine: String,
    spans: Vec<(usize, usize, String)>,
    /// Reading by span, so a reading comparison is only made where the spans already agree.
    readings: BTreeMap<(usize, usize), String>,
    looked_up: BTreeSet<(usize, usize)>,
}

impl Shape {
    fn of(engine: &str, analysis: &Analysis, dictionaries: &[Dictionary]) -> Shape {
        let mut spans = Vec::new();
        let mut readings = BTreeMap::new();
        let mut looked_up = BTreeSet::new();
        for token in &analysis.tokens {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                continue;
            };
            spans.push((start, end, token.surface.clone()));
            readings.insert((start, end), token.reading.clone());
            if token.is_word() && has_entry(token, dictionaries) {
                looked_up.insert((start, end));
            }
        }
        spans.sort();
        Shape {
            engine: engine.to_string(),
            spans,
            readings,
            looked_up,
        }
    }

    fn boundaries(&self) -> BTreeSet<(usize, usize)> {
        self.spans.iter().map(|(s, e, _)| (*s, *e)).collect()
    }

    /// Every position this segmentation cuts at.
    ///
    /// **This, not the span set, is what two segmentations agree or disagree about.** Comparing
    /// span sets counts one difference several times over: where Lindera writes それ|は and Ichiran
    /// writes それは, the symmetric difference of the span sets holds three entries for what a
    /// reader would call one disagreement, and a rate built on it is inflated by however many
    /// pieces each side happened to cut its side into.
    fn cuts(&self) -> BTreeSet<usize> {
        self.spans
            .iter()
            .flat_map(|(start, end, _)| [*start, *end])
            .collect()
    }

    /// The surfaces this segmentation produced inside `[start, end)`.
    fn within(&self, start: usize, end: usize) -> Vec<&str> {
        self.spans
            .iter()
            .filter(|(s, e, _)| *s >= start && *e <= end)
            .map(|(_, _, surface)| surface.as_str())
            .collect()
    }

    /// What the spans do and do not cover, as **three separate properties**.
    ///
    /// An earlier version returned one `Result` called `tiles` and claimed to establish exact
    /// tiling. It did not: it checked overlap, upper bound and character boundaries, and never
    /// checked for a **gap**, a **trailing omission**, or whether a span's surface is actually the
    /// source text it claims to span. So "0 unalignable" meant far less than it looked like it
    /// meant — and the run went on to find a dropped `・・` span that this check could not, by
    /// construction, have detected.
    ///
    /// The three are different findings and must not share a verdict:
    ///
    /// - **invalid** — the offsets cannot be trusted at all. Unalignable.
    /// - **mismatched** — well-formed offsets whose surface is not the source slice. Unalignable.
    /// - **uncovered** — well-formed spans leaving source unclaimed, trailing included. A
    ///   **measurable finding, not a disqualification**.
    fn coverage(&self, text: &str) -> Coverage {
        let mut coverage = Coverage::default();
        let mut at = 0usize;
        for (start, end, surface) in &self.spans {
            if *end < *start || *end > text.len() {
                coverage.invalid.push(format!(
                    "span {start}..{end} is out of bounds for {} bytes",
                    text.len()
                ));
                continue;
            }
            if !text.is_char_boundary(*start) || !text.is_char_boundary(*end) {
                coverage.invalid.push(format!(
                    "span {start}..{end} is not on a character boundary"
                ));
                continue;
            }
            if *start < at {
                coverage.invalid.push(format!(
                    "span {start}..{end} overlaps the previous, which ended at {at}"
                ));
                continue;
            }
            if *start > at {
                coverage.uncovered.push((at, *start));
            }
            if text.get(*start..*end) != Some(surface.as_str()) {
                coverage.mismatched.push(format!(
                    "span {start}..{end} reports a surface that is not the source slice"
                ));
            }
            at = *end;
        }
        if at < text.len() {
            coverage.uncovered.push((at, text.len()));
        }
        coverage
    }

    fn records(&self) -> Vec<Span> {
        self.spans
            .iter()
            .map(|(start, end, surface)| Span {
                engine: self.engine.clone(),
                start: *start,
                end: *end,
                surface: surface.clone(),
                reading: self
                    .readings
                    .get(&(*start, *end))
                    .cloned()
                    .unwrap_or_default(),
                looked_up: self.looked_up.contains(&(*start, *end)),
            })
            .collect()
    }
}

/// Availability only. A found entry is not a correct entry, and this makes no claim that it is.
fn has_entry(token: &Token, dictionaries: &[Dictionary]) -> bool {
    match sakatsu_core::lookup::word(token, dictionaries) {
        Ok(view) => !view.inline.is_empty() || view.name.is_some(),
        Err(_) => false,
    }
}

/// The stage a boundary disagreement inside `[start, end)` is attributable to.
///
/// The question is not "who drew this span" — an Ichiran-only span was drawn by no Lindera pass at
/// all. It is "did anything on the Lindera side change this stretch of text on the way to display".
fn attribute_region(
    start: usize,
    end: usize,
    raw: &Shape,
    named: &Shape,
    display: &Shape,
) -> Stage {
    let before = raw.within(start, end);
    let after = display.within(start, end);
    if before == after {
        return Stage::Analyzer;
    }
    if named.within(start, end) == after {
        return Stage::Names;
    }
    Stage::Compounds
}

/// The stage a reading disagreement on `span` is attributable to.
fn attribute_reading(span: (usize, usize), raw: &Shape, display: &Shape) -> Stage {
    match (raw.readings.get(&span), display.readings.get(&span)) {
        (Some(before), Some(after)) if before == after => Stage::Analyzer,
        (Some(_), Some(_)) => Stage::Readings,
        _ => Stage::Unknown,
    }
}

/// What one case amounts to, before a verdict is put on it.
struct Comparison {
    outcome: Outcome,
    tallies: Vec<(String, i64)>,
    spans: Vec<Span>,
    disagreements: Vec<Disagreement>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Disagreement {
    case: String,
    corpus: String,
    source: String,
    family: Option<String>,
    stratum: String,
    /// Which side of the partition the case sits on. A `holdout` record is historical evidence of
    /// an exposure, never an input to a development decision.
    partition: String,
    kind: String,
    stage: String,
    /// **A diagnostic hint, not an established attribution.** See the module docstring.
    stage_is_a_hint: bool,
    start: usize,
    end: usize,
    text: String,
    lindera: String,
    ichiran: String,
    /// What the evidence suggests, **as a proposal awaiting review**. Never a judgment.
    proposed: String,
    review_state: String,
}

fn compare(
    case: &Case,
    lindera: &LinderaAnalyzer,
    ichiran: &IchiranAnalyzer,
    dictionaries: &[Dictionary],
) -> Comparison {
    let pipeline = Pipeline::new(dictionaries);

    // **Both engines are given the same string, and it is the string offsets are checked against.**
    //
    // `LinderaAnalyzer::analyze` folds line breaks and trims before it tokenizes, so its byte
    // offsets index the *normalised* text; Ichiran grounds its offsets in whatever it was handed.
    // Comparing either against the original made both engines appear to report surfaces that were
    // not their source slice — on identical spans, which is what gave the harness away rather than
    // the engines.
    let text = sakatsu_core::analyzer::lines::normalise_breaks(&case.original)
        .trim()
        .to_string();

    let mut lindera_raw = match lindera.analyze(&text) {
        Ok(analysis) => analysis,
        Err(error) => {
            return Comparison {
                outcome: Outcome::incomplete(format!("lindera: {error:?}")),
                tallies: vec![],
                spans: vec![],
                disagreements: vec![],
            }
        }
    };
    let raw_shape = Shape::of("lindera-raw", &lindera_raw, dictionaries);

    // The name pass alone, so names and compounds can be told apart by what each actually did
    // rather than by the shape of the result.
    let named_tokens = sakatsu_core::dict::merge_names(&lindera_raw.tokens, |surface| {
        sakatsu_core::dict::look_up_name(dictionaries, surface)
    });
    let named_shape = Shape::of(
        "lindera-names-only",
        &Analysis {
            execution: None,
            text: text.clone(),
            tokens: named_tokens,
        },
        dictionaries,
    );

    pipeline.display(&mut lindera_raw);
    let lindera_shape = Shape::of("lindera-display", &lindera_raw, dictionaries);

    let mut ichiran_raw = match ichiran.analyze(&text) {
        Ok(analysis) => analysis,
        Err(error) => {
            return Comparison {
                outcome: Outcome::incomplete(format!("ichiran: {error:?}")),
                tallies: vec![],
                spans: raw_shape
                    .records()
                    .into_iter()
                    .chain(lindera_shape.records())
                    .collect(),
                disagreements: vec![],
            }
        }
    };
    let ichiran_raw_shape = Shape::of("ichiran-raw", &ichiran_raw, dictionaries);
    pipeline.display(&mut ichiran_raw);
    let ichiran_shape = Shape::of("ichiran-display", &ichiran_raw, dictionaries);

    let mut spans: Vec<Span> = Vec::new();
    for shape in [
        &raw_shape,
        &lindera_shape,
        &ichiran_raw_shape,
        &ichiran_shape,
    ] {
        spans.extend(shape.records());
    }

    // Coverage, per engine, as three separate properties. Untrustworthy offsets make the case
    // unalignable; **uncovered source does not**.
    let lindera_coverage = lindera_shape.coverage(&text);
    let ichiran_coverage = ichiran_shape.coverage(&text);
    let unalignable: Vec<String> = [
        (&lindera_shape, &lindera_coverage),
        (&ichiran_shape, &ichiran_coverage),
    ]
    .iter()
    .filter(|(_, coverage)| !coverage.alignable())
    .map(|(shape, coverage)| format!("{}: {}", shape.engine, coverage.why()))
    .collect();
    if !unalignable.is_empty() {
        return Comparison {
            outcome: Outcome::unalignable(unalignable.join("; ")),
            tallies: vec![("unalignable_engines".into(), unalignable.len() as i64)],
            spans,
            disagreements: vec![],
        };
    }
    if lindera_shape.spans.is_empty() || ichiran_shape.spans.is_empty() {
        return Comparison {
            outcome: Outcome::unalignable(format!(
                "lindera produced {} spans and ichiran {}; with nothing on one side there is no \
                 correspondence to make",
                lindera_shape.spans.len(),
                ichiran_shape.spans.len()
            )),
            tallies: vec![],
            spans,
            disagreements: vec![],
        };
    }

    let lindera_bounds = lindera_shape.boundaries();
    let ichiran_bounds = ichiran_shape.boundaries();
    let shared: BTreeSet<_> = lindera_bounds
        .intersection(&ichiran_bounds)
        .copied()
        .collect();

    let mut disagreements = Vec::new();

    // **Boundary disagreements are counted by region, not by span.** Positions both engines cut at
    // are fixed points; between two consecutive fixed points the two either agree or they do not,
    // and that stretch is one disagreement however many pieces each side cut it into.
    let mut fixed: Vec<usize> = lindera_shape
        .cuts()
        .intersection(&ichiran_shape.cuts())
        .copied()
        .collect();
    fixed.push(0);
    fixed.push(text.len());
    fixed.sort_unstable();
    fixed.dedup();

    let mut regions = 0usize;
    for window in fixed.windows(2) {
        let (start, end) = (window[0], window[1]);
        let left = lindera_shape.within(start, end);
        let right = ichiran_shape.within(start, end);
        if left == right {
            continue;
        }
        regions += 1;
        let stage = attribute_region(start, end, &raw_shape, &named_shape, &lindera_shape);
        // A region holding no kana and no kanji is a run of punctuation or symbols that one engine
        // groups and the other does not. Kept and labelled rather than counted alongside the
        // colloquial boundaries this pilot is about.
        let japanese = text
            .get(start..end)
            .unwrap_or_default()
            .chars()
            .any(|c| matches!(c, 'ぁ'..='ヿ' | '一'..='鿿'));
        disagreements.push(Disagreement {
            case: case.id.clone(),
            corpus: case.corpus.slug().into(),
            source: case.source.clone(),
            family: case.family.clone(),
            stratum: case.stratum.clone(),
            partition: case.partition.into(),
            kind: if japanese {
                "boundary".into()
            } else {
                "boundary-symbols".into()
            },
            stage: stage.to_string(),
            stage_is_a_hint: true,
            start,
            end,
            text: text.get(start..end).unwrap_or_default().to_string(),
            lindera: left.join(" | "),
            ichiran: right.join(" | "),
            proposed: "the two engines segment this stretch differently; neither is adjudicated"
                .into(),
            review_state: "proposed".into(),
        });
    }

    // Reading differences, only where the two already agree on the span.
    //
    // **Three different things were being counted as one here.** `しょぞく して` against
    // `しょぞくして` is not a disagreement about how a word is read; it is one engine putting a
    // space where the other does not. `てんパ` against `てんぱ` is not one either. Only what
    // survives both normalisations is a claim about the Japanese. None of the three is discarded:
    // each is reported under its own kind.
    for span in &shared {
        let left = lindera_shape
            .readings
            .get(span)
            .cloned()
            .unwrap_or_default();
        let right = ichiran_shape
            .readings
            .get(span)
            .cloned()
            .unwrap_or_default();
        if left == right || left.is_empty() || right.is_empty() {
            continue;
        }
        let despaced = |value: &str| {
            value
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
        };
        let folded = |value: &str| sakatsu_core::kana::to_hiragana(&despaced(value));
        let (kind, proposed) = if despaced(&left) == despaced(&right) {
            (
                "reading-spacing",
                "the readings are the same characters differently spaced; a presentation \
                 difference, not a reading one",
            )
        } else if folded(&left) == folded(&right) {
            (
                "reading-kana-form",
                "the readings differ only in whether a segment is written in katakana; a \
                 presentation difference, not a reading one",
            )
        } else {
            (
                "reading",
                "both readings are attested by an engine; neither is adjudicated",
            )
        };
        let stage = attribute_reading(*span, &raw_shape, &lindera_shape);
        disagreements.push(Disagreement {
            case: case.id.clone(),
            corpus: case.corpus.slug().into(),
            source: case.source.clone(),
            family: case.family.clone(),
            stratum: case.stratum.clone(),
            partition: case.partition.into(),
            kind: kind.into(),
            stage: stage.to_string(),
            stage_is_a_hint: true,
            start: span.0,
            end: span.1,
            text: text.get(span.0..span.1).unwrap_or_default().to_string(),
            lindera: left,
            ichiran: right,
            proposed: proposed.into(),
            review_state: "proposed".into(),
        });
    }

    // Source neither engine's spans claim, recorded per engine. A dropped span is a finding in its
    // own right, and it is **not** a boundary disagreement, so it gets its own kind.
    for (shape, coverage) in [
        (&lindera_shape, &lindera_coverage),
        (&ichiran_shape, &ichiran_coverage),
    ] {
        for (start, end) in &coverage.uncovered {
            let blank = text
                .get(*start..*end)
                .is_some_and(|slice| slice.chars().all(char::is_whitespace));
            disagreements.push(Disagreement {
                case: case.id.clone(),
                corpus: case.corpus.slug().into(),
                source: case.source.clone(),
                family: case.family.clone(),
                stratum: case.stratum.clone(),
                partition: case.partition.into(),
                kind: if blank {
                    // Neither engine tokenizes a space, and both were handed the same string.
                    // Expected, counted, and kept apart from the omissions that are not.
                    "uncovered-whitespace".into()
                } else {
                    "uncovered-source".into()
                },
                stage: Stage::Unknown.to_string(),
                stage_is_a_hint: true,
                start: *start,
                end: *end,
                text: text.get(*start..*end).unwrap_or_default().to_string(),
                lindera: if shape.engine.starts_with("lindera") {
                    "(no span)".into()
                } else {
                    String::new()
                },
                ichiran: if shape.engine.starts_with("ichiran") {
                    "(no span)".into()
                } else {
                    String::new()
                },
                proposed: format!(
                    "{} produced no span over this source range; whether the omission matters is \
                     not established here",
                    shape.engine
                ),
                review_state: "proposed".into(),
            });
        }
    }

    let agreed_regions = fixed.len().saturating_sub(1).saturating_sub(regions);
    let agreement = if fixed.len() < 2 {
        0
    } else {
        (agreed_regions * 1000 / (fixed.len() - 1)) as i64
    };

    let tallies = vec![
        ("lindera_spans".into(), lindera_shape.spans.len() as i64),
        ("ichiran_spans".into(), ichiran_shape.spans.len() as i64),
        ("shared_spans".into(), shared.len() as i64),
        ("regions".into(), (fixed.len().saturating_sub(1)) as i64),
        ("regions_disagreeing".into(), regions as i64),
        ("region_agreement_per_mille".into(), agreement),
        (
            "lindera_looked_up".into(),
            lindera_shape.looked_up.len() as i64,
        ),
        (
            "ichiran_looked_up".into(),
            ichiran_shape.looked_up.len() as i64,
        ),
        ("disagreements".into(), disagreements.len() as i64),
        ("characters".into(), text.chars().count() as i64),
        ("bytes".into(), text.len() as i64),
        (
            "lindera_uncovered_bytes".into(),
            lindera_coverage.uncovered_bytes() as i64,
        ),
        (
            "ichiran_uncovered_bytes".into(),
            ichiran_coverage.uncovered_bytes() as i64,
        ),
    ];

    let outcome = if disagreements.is_empty() {
        Outcome::Passed
    } else {
        // The stage most of this case's disagreements point at, taking the earliest: a later pass
        // working on a boundary the analyzer drew wrongly is not the pass at fault.
        let stage = disagreements
            .iter()
            .map(|d| d.stage.as_str())
            .min_by_key(|stage| match *stage {
                "analyzer" => 0,
                "compounds" => 1,
                "names" => 2,
                "readings" => 3,
                _ => 4,
            })
            .unwrap_or("unknown");
        Outcome::failed(
            match stage {
                "analyzer" => Stage::Analyzer,
                "compounds" => Stage::Compounds,
                "names" => Stage::Names,
                "readings" => Stage::Readings,
                _ => Stage::Unknown,
            },
            format!(
                "{} disagreement(s): {}",
                disagreements.len(),
                disagreements
                    .iter()
                    .take(3)
                    .map(|d| format!("{} {}@{}..{}", d.kind, d.text, d.start, d.end))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    };

    Comparison {
        outcome,
        tallies,
        spans,
        disagreements,
    }
}

/// Analyse a target inside a larger contiguous block of its own source, and project back.
///
/// **The question this answers is not the one the main comparison answers.** Every case above is
/// analysed as an isolated string, which is not how a reader meets it. If a token in the block
/// crosses the target's boundary, the isolated measurement was of a segmentation the reader would
/// never have seen.
///
/// # Where the block comes from, and where it cannot
///
/// **The promoted captures supply none.** All 141 are a single line — `corpus/REVIEW.md` records
/// the transcription convention as base text only — so there is no larger unchanged stretch of a
/// capture to put a target inside. That is a fact about the capture corpus worth reporting rather
/// than working around: the in-domain version of this measurement is not available until game
/// transcripts are promoted. Only 12 draft transcripts are multi-line, and a draft is not a
/// reviewed transcript.
///
/// So the blocks are drawn from **consecutive turns of one published chat dialogue**. Those turns
/// are contiguous in the source: the publisher ships them in that order in one file, and nothing is
/// reordered, edited or drawn from elsewhere. What *is* a decision is the separator between turns,
/// and it is recorded as one (`turn_separator`) rather than left implicit — a reader of this
/// artifact should not have to guess whether a newline came from the corpus.
///
/// The target text is preserved exactly. A target that cannot be located in its own block
/// byte-for-byte is reported rather than adjusted to fit.
///
/// Kept apart from the target-only results throughout: they are two measurements, and averaging
/// them would describe neither.
fn context_blocks(
    cases: &[Case],
    lindera: &LinderaAnalyzer,
    dictionaries: &[Dictionary],
) -> Vec<Value> {
    /// How consecutive turns are joined. A decision, so it is named and recorded.
    const TURN_SEPARATOR: &str = "\n";

    let pipeline = Pipeline::new(dictionaries);
    let dialogues = source_dialogues();
    let mut out = Vec::new();

    for case in cases {
        if out.len() >= CONTEXT_TARGETS {
            break;
        }
        if case.corpus != Corpus::Chat {
            continue;
        }
        // `chat/<source>/<dialogue>/<utterance>`
        let parts: Vec<&str> = case.id.splitn(4, '/').collect();
        let [_, source, dialogue_id, utterance_id] = parts[..] else {
            continue;
        };
        let Some(turns) = dialogues.get(&(source.to_string(), dialogue_id.to_string())) else {
            continue;
        };
        let Some(at) = turns.iter().position(|(id, _)| id == utterance_id) else {
            continue;
        };

        // A window of consecutive turns around the target, grown outward while it fits.
        let (mut first, mut last) = (at, at);
        let mut characters = turns[at].1.chars().count();
        loop {
            let mut grew = false;
            if first > 0 {
                let candidate = turns[first - 1].1.chars().count() + 1;
                if characters + candidate <= CONTEXT_BLOCK {
                    characters += candidate;
                    first -= 1;
                    grew = true;
                }
            }
            if last + 1 < turns.len() {
                let candidate = turns[last + 1].1.chars().count() + 1;
                if characters + candidate <= CONTEXT_BLOCK {
                    characters += candidate;
                    last += 1;
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        if first == last {
            // No room for a neighbour, so there is no larger block to compare against.
            continue;
        }

        let block: String = turns[first..=last]
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join(TURN_SEPARATOR);
        let block = sakatsu_core::analyzer::lines::normalise_breaks(&block)
            .trim()
            .to_string();
        let target = sakatsu_core::analyzer::lines::normalise_breaks(&turns[at].1)
            .trim()
            .to_string();

        let Some(offset) = block.find(&target) else {
            out.push(json!({
                "case": case.id,
                "outcome": "target not located in its own block byte-for-byte",
            }));
            continue;
        };
        let span = (offset, offset + target.len());

        let (Ok(mut alone), Ok(mut within)) = (lindera.analyze(&target), lindera.analyze(&block))
        else {
            continue;
        };
        pipeline.display(&mut alone);
        pipeline.display(&mut within);

        let mut crossing = Vec::new();
        let mut projected = Vec::new();
        for token in &within.tokens {
            let (Some(start), Some(end)) = (token.start, token.end) else {
                continue;
            };
            if start >= span.1 || end <= span.0 {
                continue;
            }
            if start < span.0 || end > span.1 {
                // A token the block analysed across the target's edge. This is the finding: in
                // isolation that boundary could not exist.
                crossing.push(json!({
                    "surface": token.surface,
                    "start": start,
                    "end": end,
                    "crosses": if start < span.0 { "the start" } else { "the end" },
                }));
                continue;
            }
            projected.push((start - span.0, end - span.0, token.surface.clone()));
        }

        let isolated: Vec<(usize, usize, String)> = alone
            .tokens
            .iter()
            .filter_map(|token| Some((token.start?, token.end?, token.surface.clone())))
            .collect();

        out.push(json!({
            "case": case.id,
            "source": case.source,
            "turns_in_block": last - first + 1,
            "target_is_turn": at - first,
            "turn_separator": TURN_SEPARATOR,
            "separator_is_a_rendering_decision": true,
            "block_characters": block.chars().count(),
            "target_characters": target.chars().count(),
            "target_span_in_block": [span.0, span.1],
            "isolated_tokens": isolated.len(),
            "projected_tokens": projected.len(),
            "crossing_tokens": crossing,
            "agrees_with_isolated": projected == isolated,
            "means": "a disagreement here means the target-only measurement used a segmentation \
                      the reader would not have seen",
        }));
    }
    out
}

/// Every sampled dialogue's turns, in published order, keyed by `(source, dialogue_id)`.
///
/// Read from the normalized corpus rather than from the sample, because the sample carries only
/// two preceding turns and one following, and a block may want more.
fn source_dialogues() -> BTreeMap<(String, String), Vec<(String, String)>> {
    let mut dialogues = BTreeMap::new();
    let dir = scratch_dir().join("chat");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return dialogues;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in raw.lines().filter(|line| !line.trim().is_empty()) {
            let Ok(record) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let (Some(source), Some(dialogue)) =
                (record["source"].as_str(), record["dialogue_id"].as_str())
            else {
                continue;
            };
            let turns: Vec<(String, String)> = record["utterances"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            Some((
                                item["utterance_id"].as_str()?.to_string(),
                                item["text"].as_str()?.to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            dialogues.insert((source.to_string(), dialogue.to_string()), turns);
        }
    }
    dialogues
}

fn report_context(context: &[Value], reused: usize) -> Result<(), String> {
    println!("\n=== context blocks ({} targets) ===", context.len());
    if reused > 0 {
        println!("  {reused} case(s) above were reused from a matching earlier invocation");
    }
    if context.is_empty() {
        println!("  none available: no source supplied a contiguous block around a target");
        return Ok(());
    }
    let crossing = context
        .iter()
        .filter(|entry| {
            entry["crossing_tokens"]
                .as_array()
                .is_some_and(|tokens| !tokens.is_empty())
        })
        .count();
    let agreeing = context
        .iter()
        .filter(|entry| entry["agrees_with_isolated"] == Value::Bool(true))
        .count();
    println!(
        "  {agreeing} of {} agree with the target-only segmentation",
        context.len()
    );
    println!("  {crossing} have a token crossing the target boundary");
    println!("  kept separate from the target-only results; the two are different measurements");

    let path = evaluation().join("colloquial-context-blocks.json");
    let written = write_json(
        &path,
        &json!({
            "note": "analysed inside contiguous published turns; kept separate from the \
                     target-only comparison",
            "establishes": "identical segmentation of the target for these blocks. It does NOT \
                            establish that readings, lookups or contextual sense ordering are \
                            unchanged — `in_context` ranking reads neighbouring tokens, and this \
                            compares spans and surfaces only.",
            "targets": context,
        }),
    );
    match &written {
        Ok(()) => println!("  wrote {}", path.display()),
        Err(error) => println!("  COULD NOT WRITE {}: {error}", path.display()),
    }
    written
}

#[test]
#[ignore = "needs the dictionaries, the Ichiran sidecar and the frozen sample"]
fn where_the_engines_disagree_on_colloquial_japanese() {
    let started = Instant::now();

    let dictionaries = installed_dictionaries();
    let chat = chat_cases();
    let captures = capture_cases();

    let (chat_selected, freeze_id) = match &chat {
        Ok((cases, id)) => (cases.clone(), id.clone()),
        Err(reason) => {
            println!("  frozen chat sample unavailable: {reason}");
            (Vec::new(), String::new())
        }
    };

    let mut cases: Vec<Case> = captures.clone();
    cases.extend(chat_selected);
    cases.sort_by(|a, b| a.id.cmp(&b.id));

    let ichiran = IchiranAnalyzer::new();
    sakatsu_core::analyzer::ichiran::server::set_persistent(true);
    ichiran.warm();
    let probe = ichiran.analyze("これって");
    let route = sakatsu_core::analyzer::ichiran::server::last_route();
    let ichiran_ready = probe.is_ok() && route.is_session();

    let mut run = Run::new(
        Profile::InstalledResources,
        cases.iter().map(|c| c.id.clone()),
    )
    .engine("lindera-raw, lindera-display, ichiran-raw, ichiran-display")
    .needs(
        Requirement::required("dictionaries", !dictionaries.is_empty())
            .because(format!("discovered under {}", dictionaries_dir().display())),
    )
    .needs(
        Requirement::required("frozen chat sample", chat.is_ok()).because(match &chat {
            Ok((cases, id)) => {
                format!("{} cases, freeze {}", cases.len(), &id[..16.min(id.len())])
            }
            Err(reason) => reason.clone(),
        }),
    )
    .needs(
        Requirement::required("promoted capture transcripts", !captures.is_empty())
            .because(format!("{} under {}", captures.len(), corpus().display())),
    )
    .needs(
        // **Required, not optional.** A comparison with one of its two engines missing is not
        // an incomplete comparison, it is a single-engine measurement wearing a comparison's
        // name.
        Requirement::required("ichiran via the session route", ichiran_ready).because(format!(
            "route {}, probe {}",
            route.name(),
            if probe.is_ok() { "ok" } else { "failed" }
        )),
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

    let lindera = match common::built_in() {
        Ok(analyzer) => analyzer,
        Err(error) => panic!(
            "the embedded UniDic dictionary would not load ({error:?}); a comparison cannot \
             report a Lindera result it never obtained"
        ),
    };
    let mut disagreements: Vec<Disagreement> = Vec::new();
    let mut budget_spent = false;

    // Identity first, then the resume: a resume file is only ever as good as the identity it is
    // keyed on, and every field here is one whose change would make two runs incomparable.
    let identity = Identity {
        executable: runner::executable_digest(),
        source_tree: runner::source_digest(),
        freeze_id: freeze_id.clone(),
        dictionaries: dictionaries
            .iter()
            .map(|dictionary| {
                dictionary
                    .content_fingerprint()
                    .unwrap_or_else(|_| "unfingerprinted".to_string())
            })
            .collect(),
        route: route.name().to_string(),
    };
    let resume = Resume::open(
        evaluation().join("colloquial-comparison-resume.json"),
        identity,
    );
    if let Some(reason) = &resume.refused {
        println!("  RESUME REFUSED: {reason}");
    } else if !resume.completed.is_empty() {
        println!(
            "  resuming: {} case(s) reusable under a matching identity",
            resume.completed.len()
        );
    }
    let mut completed: BTreeMap<String, Value> = resume.completed.clone();
    let mut reused = 0usize;

    for case in &cases {
        // A case already measured under this exact identity is not measured again.
        //
        // **It comes back as what it was.** An earlier version recorded every resumed case as
        // `Passed`, so a resumed run reported 379 passed and 0 failed where the measurement had
        // been 63 and 316 — an artifact reading greener than the run it claimed to continue, which
        // is the exact failure this harness exists to catch elsewhere.
        if let Some(record) = resume.completed.get(&case.id) {
            run.attribute(
                case.id.clone(),
                "lindera-display vs ichiran-display (resumed)",
            );
            if let Some(found) = record["disagreements"].as_array() {
                for entry in found {
                    if let Ok(parsed) = serde_json::from_value::<Disagreement>(entry.clone()) {
                        disagreements.push(parsed);
                    }
                }
            }
            run.record(case.id.clone(), restore_outcome(record));
            reused += 1;
            continue;
        }
        if started.elapsed() > BUDGET {
            run.record(
                case.id.clone(),
                Outcome::incomplete(
                    "the invocation's time budget was exhausted before this case was reached; \
                     run again to continue",
                ),
            );
            budget_spent = true;
            continue;
        }
        if !ichiran_ready || dictionaries.is_empty() {
            run.record(
                case.id.clone(),
                Outcome::incomplete("a required resource was absent, so the case was not compared"),
            );
            continue;
        }

        let comparison = compare(case, &lindera, &ichiran, &dictionaries);
        run.attribute(case.id.clone(), "lindera-display vs ichiran-display");
        // Persisted before the fields are moved out, so the record is of the whole comparison.
        let record = persistable(&comparison);
        run.tally(case.id.clone(), comparison.tallies);
        run.spans(case.id.clone(), comparison.spans);
        // Persisted per case, so an invocation that runs out of budget leaves the work it did
        // finish rather than discarding it.
        //
        // Only a case that reached a verdict is persisted. An `Incomplete` case ran out of budget
        // or lacked a resource, and an `Unalignable` one could not be compared — both are worth
        // attempting again, and freezing either would make a transient condition permanent.
        if let Some(record) = record {
            completed.insert(case.id.clone(), record);
        }
        disagreements.extend(comparison.disagreements);
        run.record(case.id.clone(), comparison.outcome);
    }

    if let Err(reason) = resume.save(&completed) {
        println!("  could not save the resume file: {reason}");
    }

    let context = context_blocks(&cases, &lindera, &dictionaries);
    let wrote_disagreements = summarise(&cases, &disagreements, &freeze_id, budget_spent);
    let wrote_context = report_context(&context, reused);

    // **Declared after the writes, because that is when the answer exists.** A run whose
    // disagreement list never reached disk has not produced one of its outputs, and the review
    // queue is built from that file rather than from the scrollback.
    for (name, result) in [
        ("disagreement artifact written", &wrote_disagreements),
        ("context-block artifact written", &wrote_context),
    ] {
        run = run.needs(
            Requirement::required(name, result.is_ok()).because(match result {
                Ok(()) => "written".to_string(),
                Err(error) => error.clone(),
            }),
        );
    }

    // **`accounted_for`, not `finish`.** A disagreement is the finding this run exists to produce,
    // not a failure of it. What must fail is a case that was never reached, a missing prerequisite,
    // or an empty selection — exactly what reachability judges.
    run.accounted_for().expect_success();
}

/// What a resumed record says the case was, restored faithfully.
///
/// A label that cannot be read back is `Unknown`-staged rather than a pass: an unreadable record is
/// not evidence that the case succeeded.
fn restore_outcome(record: &Value) -> Outcome {
    match record["outcome"].as_str() {
        Some("passed") => Outcome::Passed,
        Some("failed") => Outcome::failed(
            match record["stage"].as_str() {
                Some("analyzer") => Stage::Analyzer,
                Some("compounds") => Stage::Compounds,
                Some("names") => Stage::Names,
                Some("readings") => Stage::Readings,
                Some("lookup") => Stage::Lookup,
                _ => Stage::Unknown,
            },
            record["detail"].as_str().unwrap_or("resumed").to_string(),
        ),
        other => Outcome::failed(
            Stage::Unknown,
            format!(
                "a resumed record carried the unreadable outcome {other:?}; it is not treated as \
                 a pass"
            ),
        ),
    }
}

/// The record to persist, or `None` for a case worth attempting again.
fn persistable(comparison: &Comparison) -> Option<Value> {
    let (outcome, stage, detail) = match &comparison.outcome {
        Outcome::Passed => ("passed", None, None),
        Outcome::Failed { stage, detail } => {
            ("failed", Some(stage.to_string()), Some(detail.clone()))
        }
        // Incomplete and unalignable are transient or unresolved, not verdicts to freeze.
        _ => return None,
    };
    Some(json!({
        "outcome": outcome,
        "stage": stage,
        "detail": detail,
        "disagreements": comparison.disagreements,
    }))
}

fn summarise(
    cases: &[Case],
    disagreements: &[Disagreement],
    freeze_id: &str,
    budget_spent: bool,
) -> Result<(), String> {
    println!("\n=== colloquial engine comparison ===");
    if !freeze_id.is_empty() {
        println!("  freeze         {}", &freeze_id[..16.min(freeze_id.len())]);
    }
    if budget_spent {
        println!("  BUDGET EXHAUSTED — some cases were not reached; the run fails and resumes");
    }
    println!("  agreement and availability only; no correctness judgment is made or implied here");
    println!("  stage labels are DIAGNOSTIC HINTS: `analyzer` means present by the time the");
    println!("  analyzer returned, which already includes compose / rejoin_fixed_expressions /");
    println!("  terminal-kke - this project's own grouping, not the UniDic tokenizer alone.");

    // The exposure, printed every run rather than left in a document somebody has to find.
    let exposed: BTreeSet<&str> = cases
        .iter()
        .filter(|case| case.partition == "holdout")
        .map(|case| case.source.as_str())
        .collect();
    let held = cases.iter().filter(|c| c.partition == "holdout").count();
    if held > 0 {
        println!(
            "\n  HOLDOUT EXPOSURE: {held} capture(s) referenced by corpus/reading-gold.json (all"
        );
        println!("  23 of its cases are split: holdout) were analysed. Applications: {exposed:?}.");
        println!(
            "  Recorded, not undone; reported separately and ineligible for the review queue."
        );
    }

    for corpus in [Corpus::Capture, Corpus::Chat] {
        let mine: Vec<&Case> = cases.iter().filter(|c| c.corpus == corpus).collect();
        if mine.is_empty() {
            continue;
        }
        let found: Vec<&Disagreement> = disagreements
            .iter()
            .filter(|d| d.corpus == corpus.slug())
            .collect();

        // **Records, regions and cases are three different counts.** A case can contribute several
        // regions, and a reading record is not a region at all. One number called "disagreements"
        // let all three be read as each other.
        let regions = found
            .iter()
            .filter(|d| d.kind.starts_with("boundary"))
            .count();
        let readings = found
            .iter()
            .filter(|d| d.kind.starts_with("reading"))
            .count();
        let uncovered = found
            .iter()
            .filter(|d| d.kind == "uncovered-source")
            .count();
        let blank = found
            .iter()
            .filter(|d| d.kind == "uncovered-whitespace")
            .count();
        let touched: BTreeSet<&str> = found.iter().map(|d| d.case.as_str()).collect();
        println!("\n  --- {} — {} cases ---", corpus.slug(), mine.len());
        println!(
            "    {} records = {regions} boundary regions + {readings} reading records + \
             {uncovered} uncovered-source + {blank} whitespace",
            found.len()
        );
        println!("    across {} distinct cases", touched.len());

        let mut by_partition: BTreeMap<&str, usize> = BTreeMap::new();
        for case in &mine {
            *by_partition.entry(case.partition).or_default() += 1;
        }
        if by_partition.contains_key("holdout") {
            let held_records = found.iter().filter(|d| d.partition == "holdout").count();
            println!(
                "    partition {by_partition:?} - {held_records} record(s) from holdout sources, \
                 historical evidence only"
            );
        }

        // Per source, always. The chat sample is equally weighted and its pool is not; the captures
        // divide by capturing application. Neither totals to anything meaningful.
        let mut by_source: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
        for case in &mine {
            let entry = by_source.entry(case.source.as_str()).or_default();
            entry.0 += 1;
            // The same normalisation the comparison ran on, so the per-1k-character denominator
            // here is the text the engines were actually given rather than a slightly longer one.
            entry.2 += sakatsu_core::analyzer::lines::normalise_breaks(&case.original)
                .trim()
                .chars()
                .count();
        }
        // **Whitespace omissions are excluded from this table.** Neither engine tokenizes a space,
        // so counting one as a disagreement measures how many spaces a source happens to contain:
        // including them moved Multi-Party's rate from 87 to 125 per 1,000 characters while nothing
        // about its Japanese had changed.
        let substantive: Vec<&&Disagreement> = found
            .iter()
            .filter(|d| d.kind != "uncovered-whitespace")
            .collect();
        for entry in &substantive {
            by_source.entry(entry.source.as_str()).or_default().1 += 1;
        }
        println!(
            "    {:<36} {:>6} {:>8} {:>10} {:>12}   (whitespace omissions excluded)",
            "source", "cases", "disagr.", "per case", "per 1k ch"
        );
        for (source, (count, hits, chars)) in &by_source {
            println!(
                "    {source:<36} {count:>6} {hits:>8} {:>10.2} {:>12.2}",
                *hits as f64 / *count as f64,
                *hits as f64 * 1000.0 / (*chars).max(1) as f64
            );
        }

        let mut by_stage: BTreeMap<&str, usize> = BTreeMap::new();
        let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
        for d in &found {
            *by_stage.entry(d.stage.as_str()).or_default() += 1;
            *by_kind.entry(d.kind.as_str()).or_default() += 1;
        }
        println!("    stage {by_stage:?}");
        println!("    kind  {by_kind:?}");

        if corpus == Corpus::Chat {
            let mut by_family: BTreeMap<&str, usize> = BTreeMap::new();
            for d in &substantive {
                *by_family
                    .entry(d.family.as_deref().unwrap_or("(independent)"))
                    .or_default() += 1;
            }
            println!("    family {by_family:?}");

            // Family-sampled against independently sampled, per case, because the two answer
            // different questions: the first asks whether known constructions are handled, the
            // second whether anything else is.
            println!(
                "    {:<16} {:>6} {:>8} {:>10}",
                "stratum", "cases", "disagr.", "per case"
            );
            let mut strata: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for case in &mine {
                strata.entry(case.stratum.as_str()).or_default().0 += 1;
            }
            for d in &substantive {
                strata.entry(d.stratum.as_str()).or_default().1 += 1;
            }
            for (stratum, (count, hits)) in &strata {
                println!(
                    "    {stratum:<16} {count:>6} {hits:>8} {:>10.2}",
                    *hits as f64 / (*count).max(1) as f64
                );
            }
        }
    }

    // The full list goes to a file: it carries capture text, which must not travel into a committed
    // document, and it is the input to the review queue.
    let path = evaluation().join("colloquial-disagreements.json");
    let written = write_json(
        &path,
        &json!({
            "freeze_id": freeze_id,
            "note": "proposals awaiting human review; no label here is gold",
            "disagreements": disagreements,
        }),
    );
    match &written {
        Ok(()) => println!("\n  wrote {}", path.display()),
        Err(error) => println!("\n  COULD NOT WRITE {}: {error}", path.display()),
    }
    written
}

/// Write one artifact, returning the failure rather than printing it and carrying on.
///
/// **A failed artifact write must not become an apparent pass.** This used to `println!` the error
/// and return, so a run whose disagreement list never reached disk still concluded normally — and
/// the disagreement list is the input the review queue is built from, not a convenience. The
/// caller turns the error into a required resource, which is what makes the run fail.
fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let body = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("could not serialise: {error}"))?;
    std::fs::write(path, body).map_err(|error| format!("could not write: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spans that tile the text exactly: nothing invalid, nothing uncovered.
    fn shape(spans: &[(usize, usize, &str)]) -> Shape {
        Shape {
            engine: "test".into(),
            spans: spans
                .iter()
                .map(|(start, end, surface)| (*start, *end, (*surface).to_string()))
                .collect(),
            readings: BTreeMap::new(),
            looked_up: BTreeSet::new(),
        }
    }

    /// The baseline: exact tiling is clean on all three properties.
    #[test]
    fn exact_tiling_reports_nothing() {
        let coverage = shape(&[(0, 6, "それ"), (6, 9, "は")]).coverage("それは");
        assert!(coverage.alignable());
        assert!(coverage.uncovered.is_empty());
        assert_eq!(coverage.uncovered_bytes(), 0);
    }

    /// **A gap in the middle.** The predecessor of this check walked `at = end` and only rejected
    /// `start < at`, so a span starting *after* the previous one ended passed silently — which is
    /// exactly the shape of a dropped token, the thing the check existed to find.
    #[test]
    fn a_gap_between_spans_is_reported_and_does_not_block_comparison() {
        let coverage = shape(&[(0, 6, "それ"), (9, 15, "です")]).coverage("それはです");
        assert_eq!(coverage.uncovered, vec![(6, 9)]);
        assert_eq!(coverage.uncovered_bytes(), 3);
        assert!(
            coverage.alignable(),
            "a dropped span is a finding, not a reason to throw the case away"
        );
    }

    /// **A trailing omission.** Nothing looked past the last span at all, so an engine that stopped
    /// early looked identical to one that finished.
    #[test]
    fn source_after_the_last_span_is_reported() {
        let coverage = shape(&[(0, 6, "それ")]).coverage("それは");
        assert_eq!(coverage.uncovered, vec![(6, 9)]);
        assert!(coverage.alignable());
    }

    /// **A surface that is not its source slice.** Offsets can be perfectly well formed and still
    /// describe a different string. This is what caught the harness handing the engines a
    /// normalised string and then slicing the original.
    #[test]
    fn a_surface_that_is_not_the_source_slice_is_unalignable() {
        let coverage = shape(&[(0, 6, "これ"), (6, 9, "は")]).coverage("それは");
        assert!(!coverage.alignable());
        assert_eq!(coverage.mismatched.len(), 1);
        assert!(coverage.uncovered.is_empty(), "the offsets themselves tile");
    }

    /// Overlap, out of bounds and mid-character are all invalid rather than uncovered.
    #[test]
    fn malformed_offsets_are_invalid_not_uncovered() {
        let overlapping = shape(&[(0, 9, "それは"), (6, 9, "は")]).coverage("それは");
        assert!(!overlapping.alignable());

        let past_the_end = shape(&[(0, 99, "それは")]).coverage("それは");
        assert!(!past_the_end.alignable());

        // Byte 1 is inside そ.
        let mid_character = shape(&[(1, 9, "れは")]).coverage("それは");
        assert!(!mid_character.alignable());
    }

    /// The three properties are independent, and a case can have one without the others.
    #[test]
    fn the_three_properties_do_not_stand_in_for_each_other() {
        let only_uncovered = shape(&[(0, 6, "それ")]).coverage("それは");
        assert!(only_uncovered.alignable() && !only_uncovered.uncovered.is_empty());

        let only_mismatched = shape(&[(0, 9, "ちがう")]).coverage("それは");
        assert!(!only_mismatched.alignable() && only_mismatched.uncovered.is_empty());
    }

    /// **A failed artifact write is a failure, not a printed line.** The disagreement list is the
    /// review queue's input; a run that concluded without it would look like one that produced it.
    #[test]
    fn an_artifact_that_cannot_be_written_reports_the_failure() {
        let scratch = common::scratch::Scratch::new("colloquial-comparison", "artifact");
        let occupied = scratch.join("not-a-directory");
        std::fs::write(&occupied, b"a file sits where the directory would go").expect("occupy");

        let refused = write_json(&occupied.join("inside.json"), &json!({ "a": 1 }));
        assert!(refused.is_err(), "writing under a file must fail");

        let fine = write_json(&scratch.join("fine.json"), &json!({ "a": 1 }));
        assert!(fine.is_ok(), "an ordinary write still succeeds");
    }

    /// An interrupted or unresolved case is **not** persisted, so a later invocation redoes it.
    ///
    /// Freezing either would make a transient condition permanent: a budget exhaustion would become
    /// a recorded verdict, and an unalignable case would never be retried after the harness that
    /// made it unalignable was fixed.
    #[test]
    fn only_a_case_that_reached_a_verdict_is_persisted() {
        let comparison = |outcome: Outcome| Comparison {
            outcome,
            tallies: vec![],
            spans: vec![],
            disagreements: vec![],
        };
        assert!(persistable(&comparison(Outcome::Passed)).is_some());
        assert!(persistable(&comparison(Outcome::failed(Stage::Analyzer, "d"))).is_some());
        assert!(
            persistable(&comparison(Outcome::incomplete("budget"))).is_none(),
            "an interrupted case must be attempted again"
        );
        assert!(
            persistable(&comparison(Outcome::unalignable("no correspondence"))).is_none(),
            "an unalignable case must be attempted again"
        );
    }

    /// A resumed failure comes back as a failure, with its stage.
    #[test]
    fn a_resumed_record_restores_the_outcome_it_recorded() {
        let failed = persistable(&Comparison {
            outcome: Outcome::failed(Stage::Readings, "two readings differ"),
            tallies: vec![],
            spans: vec![],
            disagreements: vec![],
        })
        .expect("a failure is persisted");
        match restore_outcome(&failed) {
            Outcome::Failed { stage, detail } => {
                assert_eq!(stage, Stage::Readings);
                assert_eq!(detail, "two readings differ");
            }
            other => panic!("a resumed failure must not come back as {other:?}"),
        }
        assert_eq!(
            restore_outcome(&json!({ "outcome": "passed" })),
            Outcome::Passed
        );
    }

    /// **An unreadable record is not a pass.** Anything the restorer cannot interpret fails,
    /// because "we could not tell" and "it succeeded" are different answers.
    #[test]
    fn an_unreadable_resumed_record_is_not_treated_as_a_pass() {
        for record in [
            json!({}),
            json!({ "outcome": "nonsense" }),
            json!({ "outcome": 7 }),
        ] {
            assert!(
                matches!(restore_outcome(&record), Outcome::Failed { .. }),
                "{record} must not restore as a pass"
            );
        }
    }

    fn identity(route: &str) -> Identity {
        Identity {
            executable: "exe".into(),
            source_tree: "tree".into(),
            freeze_id: "freeze".into(),
            dictionaries: vec!["dict".into()],
            route: route.into(),
        }
    }

    /// **A resume is only as good as its identity.** Every field is one whose change would make two
    /// runs incomparable, and a mismatch must name which.
    #[test]
    fn a_resume_refuses_when_any_identity_field_differs() {
        let base = identity("session");
        let cases: Vec<(&str, Identity)> = vec![
            (
                "executable",
                Identity {
                    executable: "other".into(),
                    ..base.clone()
                },
            ),
            (
                "source tree",
                Identity {
                    source_tree: "other".into(),
                    ..base.clone()
                },
            ),
            (
                "freeze",
                Identity {
                    freeze_id: "other".into(),
                    ..base.clone()
                },
            ),
            (
                "dictionaries",
                Identity {
                    dictionaries: vec!["other".into()],
                    ..base.clone()
                },
            ),
            ("ichiran route", identity("one-shot")),
        ];
        for (field, other) in cases {
            assert_eq!(
                base.differences(&other),
                vec![field],
                "a differing {field} must be named on its own"
            );
        }
    }

    #[test]
    fn an_identical_identity_reports_no_difference() {
        assert!(identity("session")
            .differences(&identity("session"))
            .is_empty());
    }

    /// A resume file written under one identity is **discarded**, not partially reused, under
    /// another — and the reason says which field moved.
    #[test]
    fn a_resume_file_from_a_different_run_is_discarded() {
        let scratch = common::scratch::Scratch::new("colloquial-comparison", "resume");
        let path = scratch.join("resume.json");
        let recorded = Resume {
            path: path.clone(),
            identity: identity("session"),
            completed: BTreeMap::new(),
            refused: None,
        };
        let mut cases = BTreeMap::new();
        cases.insert("chat/a".to_string(), json!({ "disagreements": [] }));
        recorded.save(&cases).expect("save");

        let reopened = Resume::open(path.clone(), identity("session"));
        assert!(reopened.refused.is_none());
        assert_eq!(reopened.completed.len(), 1, "a matching identity resumes");

        let refused = Resume::open(path, identity("one-shot"));
        assert!(refused.completed.is_empty(), "a mismatch reuses nothing");
        assert!(refused
            .refused
            .is_some_and(|reason| reason.contains("ichiran route")));
    }
}
