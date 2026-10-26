//! **The reader's path, paired: what each change did, over the same raw Ichiran output.**
//!
//! ```text
//! cargo test --release -p sakatsu-core --test paired_reader -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Two changes reached the Ichiran path in this programme: the **contraction repairs**
//! (`lexical::repair`, slice 3) and the **contextual tie-break** (`ichiran::TiePolicy`, slice 4,
//! provisional). `AnalysisOptions::without_repairs()` turns off only the first, so a run without
//! repairs is **not** the pre-change path. This harness keeps the two apart by capturing each
//! sentence's raw Ichiran output **once** and replaying it through `study::run` under four
//! configurations:
//!
//! | configuration | tie-break | repairs |
//! |---|---|---|
//! | `baseline` | bare word (before slice 4) | off (before slice 3) |
//! | `repairs-only` | bare word | on |
//! | `tie-break-only` | contextual | off |
//! | `shipped` | contextual | on |
//!
//! and reporting three comparisons, each changing one thing or naming that it changes both:
//!
//! - **A. repairs off → on**, under the shipped conversion: `tie-break-only` → `shipped`;
//! - **B. tie-break old → new**, with repairs held on: `repairs-only` → `shipped`;
//! - **C. the combined reader path**: `baseline` → `shipped`, over the same raw output.
//!
//! The comparison against the **recorded** §1 baseline is `standalone_reference.rs` rerun, whose
//! metric code and workload are the ones that recorded it.
//!
//! **Same raw candidates, by construction**: Ichiran is asked once per sentence, and every
//! configuration converts that one answer. Nothing here can differ because the sidecar answered
//! differently the second time.
//!
//! # Timing
//!
//! Conversion and `study::run`, per configuration, **excluding** the sidecar (asked once, timed
//! apart) and **excluding scoring**, which happens after the clock stops. Three interleaved
//! repetitions, configuration order rotated; the first repetition carries warm-up. This is a
//! comparison of the configurations inside this harness, **not** reader latency: it stops short
//! of the complete `StudyView`, the scheduler and the sidecar. `src-tauri/tests/ichiran_latency.rs`
//! measures those.
//!
//! # Determinism, of the output and not only of the scores
//!
//! Each repetition is compared with the first **twice**: its scoring summaries (reading
//! correctness at gold spans, matched gold boundaries, counts), and — separately — a digest of
//! the **complete** analysis each configuration produced ([`Output`]: every token's spelling,
//! span, reading, lemma, conjugation, senses and parts, nested). A reading that changes from one
//! wrong answer to another, boundaries that move while their count stays the same, or a changed
//! lemma or part all leave the scores alone and change the digest; the always-run controls at the
//! bottom prove each is caught. The comparisons A–C report changed boundaries and changed output
//! beside the scores, so "boundary F1 unchanged" is never read as "boundaries unchanged".
//!
//! # Licence
//!
//! KWDLC is consult-only: sentences and readings go only to the ignored private artifact; what is
//! printed carries identifiers, spans and counts.

mod common;

#[path = "common/heteronyms.rs"]
mod heteronyms;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use common::knp::{self, level};
use sakatsu_core::analyzer::ichiran::{self, raw, TiePolicy};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, IchiranAnalyzer, Traced};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Analysis;
use sakatsu_core::study::{self, AnalysisOptions};
use serde_json::json;

#[derive(Clone, Copy)]
struct Config {
    name: &'static str,
    policy: TiePolicy,
    repairs: bool,
}

const CONFIGS: [Config; 4] = [
    Config {
        name: "baseline",
        policy: TiePolicy::BareWord,
        repairs: false,
    },
    Config {
        name: "repairs-only",
        policy: TiePolicy::BareWord,
        repairs: true,
    },
    Config {
        name: "tie-break-only",
        policy: TiePolicy::Contextual,
        repairs: false,
    },
    Config {
        name: "shipped",
        policy: TiePolicy::Contextual,
        repairs: true,
    },
];

const COMPARISONS: [(&str, &str, &str); 3] = [
    (
        "A. repairs off → on (contextual tie-break)",
        "tie-break-only",
        "shipped",
    ),
    (
        "B. tie-break bare → contextual (repairs on)",
        "repairs-only",
        "shipped",
    ),
    ("C. combined: baseline → shipped", "baseline", "shipped"),
];

impl Config {
    fn options(self) -> AnalysisOptions {
        if self.repairs {
            AnalysisOptions::PRODUCTION
        } else {
            AnalysisOptions::PRODUCTION.without_repairs()
        }
    }
}

/// Ichiran's one answer for one sentence, converted under a chosen tie-break.
struct Replayed<'a> {
    raw: &'a str,
    policy: TiePolicy,
}

impl Analyzer for Replayed<'_> {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }

    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        let segments = raw::parse(self.raw)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        Ok(Traced {
            analysis: ichiran::to_analysis_under(text, &segments, self.policy),
            raw: Some(self.raw.to_string()),
        })
    }

    fn engine(&self) -> &str {
        // The repairs are gated on the engine; this is Ichiran's output.
        "ichiran"
    }
}

type Span = (usize, usize);

struct Case {
    source: &'static str,
    id: String,
    text: String,
    readings: BTreeMap<Span, String>,
    levels: Vec<(&'static str, BTreeSet<Span>)>,
}

fn hiragana(text: &str) -> String {
    sakatsu_core::kana::to_hiragana(text)
}

fn kwdlc_cases(evaluation: &std::path::Path) -> Vec<Case> {
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
        "the reservation must be readable; {audit}"
    );
    // The selection `standalone_reference.rs` makes, so these are the §1 sentences.
    let (all_files, walk_faults) = knp::collect(&corpus);
    let development: Vec<std::path::PathBuf> = all_files
        .into_iter()
        .filter(|path| {
            !reserved.contains(
                &path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect();
    let sentences =
        knp::load_files(&development, walk_faults).unwrap_or_else(|damaged| panic!("{damaged}"));
    let stride = sentences.len().div_ceil(2000).max(1);
    sentences
        .iter()
        .step_by(stride)
        .take(300)
        .map(|sentence| Case {
            source: "kwdlc",
            id: sentence.id(),
            text: sentence.text.clone(),
            readings: sentence
                .morphemes
                .iter()
                .map(|morpheme| ((morpheme.start, morpheme.end), hiragana(&morpheme.reading)))
                .collect(),
            levels: vec![
                (
                    "morpheme",
                    knp::spans_of(&sentence.morphemes, level::morpheme)
                        .into_iter()
                        .collect(),
                ),
                (
                    "基本句",
                    knp::spans_of(&sentence.morphemes, level::kihonku)
                        .into_iter()
                        .collect(),
                ),
                (
                    "文節",
                    knp::spans_of(&sentence.morphemes, level::bunsetsu)
                        .into_iter()
                        .collect(),
                ),
            ],
        })
        .collect()
}

fn ud_cases() -> Vec<Case> {
    let sentences = common::gold::test_and_dev();
    // The selection `standalone_reference.rs` makes.
    let stride = sentences.len().div_ceil(300).max(1);
    sentences
        .iter()
        .step_by(stride)
        .map(|sentence| Case {
            source: "ud",
            id: sentence.id.clone(),
            text: sentence.text.clone(),
            readings: BTreeMap::new(),
            levels: vec![
                (
                    "SUW",
                    sentence
                        .tokens
                        .iter()
                        .map(|token| (token.start, token.end))
                        .collect(),
                ),
                (
                    "LUW",
                    common::gold::long_units(&sentence.tokens)
                        .into_iter()
                        .collect(),
                ),
            ],
        })
        .collect()
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
                readings: BTreeMap::from([(
                    (start, start + case.term.len()),
                    hiragana(case.reading),
                )]),
                levels: Vec::new(),
            })
        })
        .collect()
}

/// One configuration's answer for one case, reduced to what is scored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Scored {
    failure: Option<String>,
    /// Gold spans a token aligned to, and whether its reading was right.
    readings: BTreeMap<Span, bool>,
    /// Per level: produced spans that match gold.
    matched: BTreeMap<&'static str, BTreeSet<Span>>,
    produced: usize,
    component_faults: usize,
    selectable_faults: usize,
    repairs: usize,
    abstentions: BTreeMap<&'static str, usize>,
}

/// What a configuration actually produced, beyond what is scored: the boundaries, and a digest of
/// the complete analysis.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Output {
    spans: BTreeSet<Span>,
    /// SHA-256 of the serialized [`Analysis`]: every field of every token, parts included.
    digest: String,
}

fn output(analysis: &Analysis) -> Output {
    use sha2::{Digest, Sha256};
    let serialized = serde_json::to_string(analysis).expect("an analysis serializes");
    Output {
        spans: analysis
            .tokens
            .iter()
            .filter_map(|token| Some((token.start?, token.end?)))
            .collect(),
        digest: sakatsu_core::hashing::hex(Sha256::digest(serialized.as_bytes())),
    }
}

fn score(case: &Case, outcome: &Result<study::Run, study::RunFailure>) -> (Scored, Output) {
    match outcome {
        Ok(run) => (
            score_analysis(case, &run.analysis, &run.repairs),
            output(&run.analysis),
        ),
        Err(failure) => (
            Scored {
                failure: Some(failure.error.to_string()),
                ..Scored::default()
            },
            Output::default(),
        ),
    }
}

fn score_analysis(
    case: &Case,
    analysis: &Analysis,
    repairs: &sakatsu_core::lexical::repair::Account,
) -> Scored {
    let produced: BTreeSet<Span> = analysis
        .tokens
        .iter()
        .filter_map(|token| Some((token.start?, token.end?)))
        .collect();
    let mut readings = BTreeMap::new();
    for token in &analysis.tokens {
        let (Some(start), Some(end)) = (token.start, token.end) else {
            continue;
        };
        if let Some(gold) = case.readings.get(&(start, end)) {
            readings.insert((start, end), hiragana(&token.reading) == *gold);
        }
    }
    let mut abstentions = BTreeMap::new();
    for (_, reason) in repairs.abstentions() {
        *abstentions.entry(reason.name()).or_default() += 1;
    }
    Scored {
        failure: None,
        readings,
        matched: case
            .levels
            .iter()
            .map(|(name, gold)| (*name, produced.intersection(gold).copied().collect()))
            .collect(),
        produced: produced.len(),
        component_faults: sakatsu_core::analyzer::validate_components(analysis).len(),
        selectable_faults: sakatsu_core::analyzer::validate_selectable(analysis).len(),
        repairs: repairs.notes().len(),
        abstentions,
    }
}

fn run_config(
    case: &Case,
    raw: &str,
    config: Config,
    dictionaries: &[Dictionary],
) -> Result<study::Run, study::RunFailure> {
    let replayed = Replayed {
        raw,
        policy: config.policy,
    };
    study::run(&case.text, &replayed, dictionaries, config.options())
}

fn f1(reference: usize, produced: usize, matched: usize) -> f64 {
    if reference == 0 || produced == 0 {
        return 0.0;
    }
    let precision = matched as f64 / produced as f64;
    let recall = matched as f64 / reference as f64;
    if precision + recall == 0.0 {
        0.0
    } else {
        200.0 * precision * recall / (precision + recall)
    }
}

#[test]
#[ignore = "needs Ichiran, KWDLC, UD_Japanese-GSD, the reservation and the dictionaries"]
fn the_reader_path_paired_change_by_change() {
    let evaluation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("target/evaluation");
    let dictionaries = common::installed_dictionaries();
    let ichiran = IchiranAnalyzer::new();
    ichiran::server::set_persistent(true);
    ichiran.warm();
    assert!(ichiran.analyze("これって").is_ok(), "Ichiran must answer");
    let route = ichiran::server::last_route();

    let mut cases = kwdlc_cases(&evaluation);
    cases.extend(ud_cases());
    cases.extend(heteronym_cases());

    // Ichiran, once per sentence.
    let asked = Instant::now();
    let mut raws: Vec<Option<String>> = Vec::with_capacity(cases.len());
    let mut sidecar_failures = Vec::new();
    for case in &cases {
        match ichiran.analyze_traced(&case.text) {
            Ok(Traced { raw: Some(raw), .. }) => raws.push(Some(raw)),
            Ok(_) => {
                sidecar_failures.push(format!("{} returned no raw output", case.id));
                raws.push(None);
            }
            Err(error) => {
                sidecar_failures.push(format!("{} {error}", case.id));
                raws.push(None);
            }
        }
    }
    let sidecar_time = asked.elapsed();

    // Three interleaved repetitions, configuration order rotated; the first is scored and the
    // others must reproduce it exactly.
    let mut scored: BTreeMap<&'static str, Vec<Scored>> = BTreeMap::new();
    let mut outputs: BTreeMap<&'static str, Vec<Output>> = BTreeMap::new();
    let mut timings: BTreeMap<&'static str, Vec<Duration>> = BTreeMap::new();
    let mut scores_differed = Vec::new();
    let mut outputs_differed = Vec::new();
    for repetition in 0..3 {
        for offset in 0..CONFIGS.len() {
            let config = CONFIGS[(offset + repetition) % CONFIGS.len()];
            // Timed: the runs only. Scoring happens after the clock stops.
            let started = Instant::now();
            let runs: Vec<Option<Result<study::Run, study::RunFailure>>> = cases
                .iter()
                .zip(&raws)
                .map(|(case, raw)| {
                    raw.as_ref()
                        .map(|raw| run_config(case, raw, config, &dictionaries))
                })
                .collect();
            timings
                .entry(config.name)
                .or_default()
                .push(started.elapsed());
            let (results, produced): (Vec<Scored>, Vec<Output>) = cases
                .iter()
                .zip(&runs)
                .map(|(case, run)| match run {
                    Some(run) => score(case, run),
                    None => (
                        Scored {
                            failure: Some("the sidecar did not answer".into()),
                            ..Scored::default()
                        },
                        Output::default(),
                    ),
                })
                .unzip();
            match scored.get(config.name) {
                None => {
                    scored.insert(config.name, results);
                }
                Some(first) if *first != results => {
                    scores_differed.push(format!("{} repetition {repetition}", config.name))
                }
                Some(_) => {}
            }
            match outputs.get(config.name) {
                None => {
                    outputs.insert(config.name, produced);
                }
                Some(first) if *first != produced => {
                    outputs_differed.push(format!("{} repetition {repetition}", config.name))
                }
                Some(_) => {}
            }
        }
    }

    // ---- private artifact: per case, per configuration.
    let mut private = String::new();
    for (index, case) in cases.iter().enumerate() {
        let per: BTreeMap<&str, serde_json::Value> = CONFIGS
            .iter()
            .map(|config| {
                let result = &scored[config.name][index];
                (
                    config.name,
                    json!({
                        "failure": result.failure,
                        "readings": result.readings.iter().map(|(span, right)| json!([span.0, span.1, right])).collect::<Vec<_>>(),
                        "repairs": result.repairs,
                        "abstentions": result.abstentions,
                        "component_faults": result.component_faults,
                        "selectable_faults": result.selectable_faults,
                    }),
                )
            })
            .collect();
        private.push_str(
            &json!({"source": case.source, "id": case.id, "text": case.text, "configs": per})
                .to_string(),
        );
        private.push('\n');
    }
    let artifact = common::reference::publish(&evaluation, "paired-reader", "jsonl", &private)
        .unwrap_or_else(|error| panic!("could not publish: {error}"));

    // ---- report
    println!("\n=== the reader path, paired over the same raw Ichiran output ===");
    println!("  route          {}", route.name());
    println!(
        "  executable     {}",
        common::reference::executable_identity()
    );
    for dictionary in &dictionaries {
        println!(
            "  dictionary     {} (sha256 {})",
            dictionary.meta().title,
            dictionary.content_fingerprint().unwrap_or_default()
        );
    }
    let sources: BTreeSet<&str> = cases.iter().map(|case| case.source).collect();
    for source in &sources {
        println!(
            "  {source:<14} {} sentences",
            cases.iter().filter(|case| case.source == *source).count()
        );
    }
    println!(
        "  sidecar        {} failures, {:.1?} for all sentences (once)",
        sidecar_failures.len(),
        sidecar_time
    );
    for failure in &sidecar_failures {
        println!("    {failure}");
    }

    for source in &sources {
        let indices: Vec<usize> = (0..cases.len())
            .filter(|&i| cases[i].source == *source)
            .collect();
        println!("\n  [{source}]");
        for config in CONFIGS {
            let results: Vec<&Scored> = indices.iter().map(|&i| &scored[config.name][i]).collect();
            let failures = results
                .iter()
                .filter(|result| result.failure.is_some())
                .count();
            let gold_spans: usize = indices.iter().map(|&i| cases[i].readings.len()).sum();
            let aligned: usize = results.iter().map(|result| result.readings.len()).sum();
            let agreed: usize = results
                .iter()
                .map(|result| result.readings.values().filter(|right| **right).count())
                .sum();
            let mut line = format!("    {:<15} failures {failures}", config.name);
            if gold_spans > 0 {
                line.push_str(&format!(
                    " · readings {agreed}/{aligned} ({:.1}%) over {aligned}/{gold_spans} aligned",
                    100.0 * agreed as f64 / aligned.max(1) as f64
                ));
            }
            for (name, _) in &cases[indices[0]].levels {
                let reference: usize = indices
                    .iter()
                    .map(|&i| {
                        cases[i]
                            .levels
                            .iter()
                            .find(|(level, _)| level == name)
                            .map_or(0, |(_, spans)| spans.len())
                    })
                    .sum();
                let produced: usize = results.iter().map(|result| result.produced).sum();
                let matched: usize = results
                    .iter()
                    .map(|result| result.matched.get(name).map_or(0, BTreeSet::len))
                    .sum();
                line.push_str(&format!(
                    " · {name} F1 {:.1}",
                    f1(reference, produced, matched)
                ));
            }
            let repairs: usize = results.iter().map(|result| result.repairs).sum();
            let components: usize = results.iter().map(|result| result.component_faults).sum();
            let selectable: usize = results.iter().map(|result| result.selectable_faults).sum();
            line.push_str(&format!(
                " · repairs {repairs} · component faults {components} · offered-part faults {selectable}"
            ));
            println!("{line}");
            let mut abstentions: BTreeMap<&str, usize> = BTreeMap::new();
            for result in &results {
                for (reason, count) in &result.abstentions {
                    *abstentions.entry(reason).or_default() += count;
                }
            }
            if !abstentions.is_empty() {
                println!("      repair abstentions {abstentions:?}");
            }
        }

        for (title, from, to) in COMPARISONS {
            let (mut fixed, mut broken, mut appeared, mut vanished) =
                (Vec::new(), Vec::new(), 0usize, 0usize);
            let mut boundary: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            let (mut spans_changed, mut output_changed) = (Vec::new(), 0usize);
            for &i in &indices {
                let (was, now) = (&outputs[from][i], &outputs[to][i]);
                if was.spans != now.spans {
                    spans_changed.push(cases[i].id.clone());
                }
                output_changed += usize::from(was.digest != now.digest);
                let (before, after) = (&scored[from][i], &scored[to][i]);
                for (span, right) in &after.readings {
                    match before.readings.get(span) {
                        Some(was) if !was && *right => {
                            fixed.push(format!("{} [{},{})", cases[i].id, span.0, span.1))
                        }
                        Some(was) if *was && !right => {
                            broken.push(format!("{} [{},{})", cases[i].id, span.0, span.1))
                        }
                        Some(_) => {}
                        None => appeared += 1,
                    }
                }
                vanished += before
                    .readings
                    .keys()
                    .filter(|span| !after.readings.contains_key(*span))
                    .count();
                for (name, spans) in &after.matched {
                    let was = before.matched.get(name).cloned().unwrap_or_default();
                    let entry = boundary.entry(name).or_default();
                    entry.0 += spans.difference(&was).count();
                    entry.1 += was.difference(spans).count();
                }
            }
            println!("    {title}");
            println!(
                "      readings: {} wrong→right, {} right→wrong; aligned spans +{appeared} −{vanished}",
                fixed.len(),
                broken.len()
            );
            for (name, (gained, lost)) in &boundary {
                println!("      {name} gold boundaries matched: +{gained} −{lost}");
            }
            println!(
                "      cases whose produced boundaries changed: {} · cases whose complete output changed: {output_changed}",
                spans_changed.len()
            );
            for id in &spans_changed {
                println!("        boundaries changed  {id}");
            }
            for id in &fixed {
                println!("        fixed   {id}");
            }
            for id in &broken {
                println!("        broken  {id}");
            }
        }
    }

    println!(
        "\n  determinism: scoring summaries {} · complete outputs {} (repetitions 2 and 3 against 1)",
        if scores_differed.is_empty() { "identical" } else { "DIFFERED" },
        if outputs_differed.is_empty() { "identical" } else { "DIFFERED" }
    );
    println!("\n  timing (study::run only; sidecar and scoring excluded; first repetition carries warm-up), 3 interleaved repetitions:");
    for config in CONFIGS {
        let runs = &timings[config.name];
        let mut millis: Vec<f64> = runs.iter().map(|run| run.as_secs_f64() * 1000.0).collect();
        millis.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        println!(
            "    {:<15} min {:.0} ms · median {:.0} ms · max {:.0} ms over {} sentences",
            config.name,
            millis[0],
            millis[millis.len() / 2],
            millis[millis.len() - 1],
            cases.len()
        );
    }
    println!(
        "\n  artifact       {} (private: holds the sentences)",
        artifact.display()
    );

    assert!(
        scores_differed.is_empty(),
        "scoring summaries differed between repetitions: {scores_differed:?}"
    );
    assert!(
        outputs_differed.is_empty(),
        "complete outputs differed between repetitions: {outputs_differed:?}"
    );
    assert!(
        sidecar_failures.is_empty(),
        "{} sentences unanswered",
        sidecar_failures.len()
    );
}

// ---------------------------------------------------------------------------
// Controls: changes the scores cannot see, which the output comparison must.

fn control_case() -> Case {
    Case {
        source: "control",
        id: "control".into(),
        text: "ABCDEF".into(),
        readings: BTreeMap::from([((0, 3), "gold".to_string())]),
        levels: vec![("level", BTreeSet::from([(0, 6)]))],
    }
}

fn token(
    surface: &str,
    start: usize,
    end: usize,
    reading: &str,
    lemma: &str,
) -> sakatsu_core::model::Token {
    sakatsu_core::model::Token {
        surface: surface.into(),
        reading: reading.into(),
        dict_form: lemma.into(),
        start: Some(start),
        end: Some(end),
        ..Default::default()
    }
}

fn analysis(tokens: Vec<sakatsu_core::model::Token>) -> Analysis {
    Analysis {
        text: "ABCDEF".into(),
        tokens,
        execution: None,
    }
}

/// Both analyses score the same and are different: the output comparison must say so.
fn assert_only_the_output_differs(before: &Analysis, after: &Analysis, what: &str) {
    let case = control_case();
    let account = sakatsu_core::lexical::repair::Account::default();
    assert_eq!(
        score_analysis(&case, before, &account),
        score_analysis(&case, after, &account),
        "{what}: the premise is that the scores cannot see it"
    );
    assert_ne!(
        output(before),
        output(after),
        "{what}: the output comparison must"
    );
}

#[test]
fn one_wrong_reading_becoming_another_is_detected() {
    let before = analysis(vec![
        token("ABC", 0, 3, "wrong", "x"),
        token("DEF", 3, 6, "", "y"),
    ]);
    let after = analysis(vec![
        token("ABC", 0, 3, "other", "x"),
        token("DEF", 3, 6, "", "y"),
    ]);
    assert_only_the_output_differs(&before, &after, "wrong → other wrong");
}

#[test]
fn boundaries_moving_while_their_count_holds_are_detected() {
    let before = analysis(vec![
        token("AB", 0, 2, "", "x"),
        token("CDEF", 2, 6, "", "y"),
    ]);
    let after = analysis(vec![
        token("ABCD", 0, 4, "", "x"),
        token("EF", 4, 6, "", "y"),
    ]);
    assert_only_the_output_differs(&before, &after, "unmatched boundaries moved");
    assert_ne!(output(&before).spans, output(&after).spans);
}

#[test]
fn a_changed_lemma_is_detected() {
    let before = analysis(vec![
        token("ABC", 0, 3, "wrong", "x"),
        token("DEF", 3, 6, "", "y"),
    ]);
    let after = analysis(vec![
        token("ABC", 0, 3, "wrong", "z"),
        token("DEF", 3, 6, "", "y"),
    ]);
    assert_only_the_output_differs(&before, &after, "lemma");
}

#[test]
fn a_changed_part_is_detected() {
    // A valid part either way — it spells its own source slice — whose lemma changes: nothing the
    // scores or the component contract look at.
    let whole = |lemma: &str| {
        let mut word = token("ABC", 0, 3, "wrong", "x");
        word.components = vec![token("A", 0, 1, "", lemma), token("BC", 1, 3, "", "BC")];
        analysis(vec![word, token("DEF", 3, 6, "", "y")])
    };
    assert_only_the_output_differs(&whole("A"), &whole("Q"), "component");
}
