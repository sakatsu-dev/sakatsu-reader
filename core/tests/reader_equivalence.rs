//! **Does any assistance mode change what a reader is shown?** Complete `StudyView` outcomes under
//! `Legacy`, `SharedLazy` and `SharedEager`, compared with a baseline the pre-change code recorded.
//!
//! ```text
//! # the baseline, recorded by the preserved pre-change source (see reader_equivalence_record.rs):
//! $env:READER_EQUIVALENCE_LABEL = 'pre-change'; cargo test … --test reader_equivalence_record …
//! # the comparison, replaying the same raw Ichiran answers:
//! $env:READER_EQUIVALENCE_RAW = '<raw artifact>'
//! $env:READER_EQUIVALENCE_BASELINE = '<pre-change outcomes>'
//! $env:READER_EQUIVALENCE_BASELINE_CONFIG = 'pre-change'
//! cargo test --release -p sakatsu-core --test reader_equivalence -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `READER_EQUIVALENCE_WORKLOAD=multiline` runs the multiline workload instead, with its own
//! `…_MULTILINE_RAW` and `…_MULTILINE_BASELINE`. There the run must show the joined-line questions
//! being asked under both engines, or it has not measured what it exists to measure.
//!
//! What is recorded and what a comparison refuses is in `common/equivalence.rs`. This file adds the
//! modes, Ichiran's conversion inside the request (so the mode under test is the mode its advice is
//! asked in), and the account of every question each mode asked.
//!
//! # Licence
//!
//! KWDLC is consult-only and the notebook is the reader's own: passages and raw answers go only to
//! the ignored private artifacts; what is printed and published carries identifiers and digests.

mod common;

#[path = "common/heteronyms.rs"]
mod heteronyms;

use std::collections::BTreeMap;

use common::equivalence::{self, Runner, Workload};
use sakatsu_core::analyzer::assist::{Assistance, AssistanceMode, Stage};
use sakatsu_core::analyzer::ichiran::{self, raw};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, LinderaAnalyzer, Traced};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Analysis;
use sakatsu_core::study::{self, AnalysisOptions, AssistanceAccount};
use serde_json::{json, Value};

/// Ichiran's recorded answer for one passage, converted exactly as
/// `IchiranAnalyzer::analyze_in` converts a live one — inside the request, so the mode under test
/// is the mode the conversion's advice is asked in.
struct Replayed(String);

impl Analyzer for Replayed {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }
    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        self.analyze_in(text, &Assistance::new(AssistanceMode::Legacy))
    }
    fn analyze_in(&self, text: &str, assistance: &Assistance<'_>) -> Result<Traced, AnalyzeError> {
        assistance.refuse_if_advising()?;
        let text = sakatsu_core::analyzer::lines::normalise_breaks(text);
        let text = text.trim();
        if text.is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }
        let segments = raw::parse(&self.0)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        Ok(Traced {
            analysis: ichiran::to_analysis_in(
                text,
                &segments,
                ichiran::TiePolicy::SHIPPED,
                assistance,
            )?,
            raw: Some(self.0.clone()),
        })
    }
    fn engine(&self) -> &str {
        "ichiran"
    }
}

type Outcome = Result<study::Run, study::RunFailure>;
type ModeRunner = Box<dyn Fn(&str, &dyn Analyzer, &[Dictionary]) -> Outcome>;

/// The modes compared. `READER_EQUIVALENCE_MODES` narrows them, as a comma-separated list of the
/// names below.
fn modes() -> Vec<(&'static str, AssistanceMode)> {
    let all = [
        ("legacy", AssistanceMode::Legacy),
        ("shared-lazy", AssistanceMode::SharedLazy),
        ("shared-eager", AssistanceMode::SharedEager),
    ];
    let wanted = std::env::var("READER_EQUIVALENCE_MODES").ok();
    all.into_iter()
        .filter(|(name, _)| {
            wanted
                .as_deref()
                .is_none_or(|wanted| wanted.split(',').any(|one| one.trim() == *name))
        })
        .collect()
}

/// Every question a configuration's requests asked, summed over the workload, per engine.
#[derive(Default)]
struct Tally {
    requests: usize,
    asked: [usize; 4],
    executed: [usize; 4],
    remembered: [usize; 4],
    failed: usize,
    unavailable: usize,
    refused: usize,
    reads_requested: usize,
    reads_executed: usize,
    reads_remembered: usize,
    reads_failed: usize,
}

impl Tally {
    fn add(&mut self, account: &AssistanceAccount) {
        self.requests += 1;
        for (slot, stage) in Stage::ALL.into_iter().enumerate() {
            let counted = account.advice.stage(stage);
            self.asked[slot] += counted.requested;
            self.executed[slot] += counted.executed;
            self.remembered[slot] += counted.remembered;
            self.failed += counted.failed;
            self.unavailable += counted.unavailable;
        }
        self.refused += account.advice.refused;
        for operation in [
            account.evidence.lookup,
            account.evidence.lookup_reading,
            account.evidence.same_entry,
        ] {
            self.reads_requested += operation.requested;
            self.reads_executed += operation.executed;
            self.reads_remembered += operation.remembered;
            self.reads_failed += operation.failed;
        }
    }

    fn to_json(&self) -> Value {
        let stages = |counts: &[usize; 4]| {
            Stage::ALL
                .into_iter()
                .zip(counts)
                .map(|(stage, count)| (format!("{stage:?}"), json!(count)))
                .collect::<serde_json::Map<String, Value>>()
        };
        json!({
            "requests": self.requests,
            "advice_asked": stages(&self.asked),
            "advice_executed": stages(&self.executed),
            "advice_remembered": stages(&self.remembered),
            "advice_failed": self.failed,
            "advice_unavailable": self.unavailable,
            "refused": self.refused,
            "dictionary_requested": self.reads_requested,
            "dictionary_executed": self.reads_executed,
            "dictionary_remembered": self.reads_remembered,
            "dictionary_failed": self.reads_failed,
        })
    }
}

#[test]
#[ignore = "needs the dictionaries, the kanji table, KWDLC, UD and the notebook, and Ichiran unless replaying"]
fn every_assistance_mode_serves_the_recorded_output() {
    let workload = Workload::from_env();
    let heteronym_texts: Vec<&str> = heteronyms::HETERONYMS
        .iter()
        .map(|case| case.text)
        .collect();
    let modes = modes();
    let runners: Vec<ModeRunner> = modes
        .iter()
        .map(|(_, mode)| {
            let options = AnalysisOptions::PRODUCTION.with_assistance(*mode);
            Box::new(
                move |text: &str, analyzer: &dyn Analyzer, dictionaries: &[Dictionary]| {
                    study::run(text, analyzer, dictionaries, options)
                },
            ) as ModeRunner
        })
        .collect();
    let configurations: Vec<(&str, Runner<'_>)> = modes
        .iter()
        .zip(&runners)
        .map(|((name, _), runner)| (*name, runner.as_ref() as Runner<'_>))
        .collect();

    let lindera = LinderaAnalyzer::new().expect("compiled in");
    let mut tallies: BTreeMap<(String, String), Tally> = BTreeMap::new();
    let mut recorded = equivalence::drive(
        workload,
        &heteronym_texts,
        &configurations,
        &|raw| Box::new(Replayed(raw)),
        &lindera,
        &mut |configuration, engine, run| {
            tallies
                .entry((configuration.to_string(), engine.to_string()))
                .or_default()
                .add(&run.assistance);
        },
    );

    let mut accounts = serde_json::Map::new();
    for ((configuration, engine), tally) in &tallies {
        accounts
            .entry(configuration.clone())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("an object")
            .insert(engine.clone(), tally.to_json());
    }
    recorded.value["accounts"] = Value::Object(accounts);

    // **Teeth, for the workload that exists to ask them**: a multiline run in which no join was
    // put to the parser has compared nothing about joins.
    if workload == Workload::Multiline {
        let slot = Stage::ALL
            .iter()
            .position(|stage| *stage == Stage::JoinedLines)
            .expect("a stage");
        for engine in ["ichiran", "lindera"] {
            let asked = tallies
                .iter()
                .filter(|((_, from), _)| from == engine)
                .map(|(_, tally)| tally.asked[slot])
                .min()
                .unwrap_or(0);
            assert!(
                asked > 0,
                "{engine}: no joined-line question was asked in some mode, so the multiline workload \
                 measured nothing about joins"
            );
        }
    }
    equivalence::finish(recorded);
}
