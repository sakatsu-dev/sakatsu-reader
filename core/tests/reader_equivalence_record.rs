//! **Record the reader's output as the code stands**, for `reader_equivalence.rs` to compare with.
//!
//! ```text
//! $env:READER_EQUIVALENCE_LABEL = 'pre-change'      # required: what this recording is
//! $env:READER_EQUIVALENCE_WORKLOAD = 'full'         # or 'multiline'
//! cargo test --release -p sakatsu-core --test reader_equivalence_record -- --ignored --nocapture --test-threads=1
//! ```
//!
//! **Old API only, on purpose.** This file and `common/equivalence.rs` use nothing the analysis path
//! lacked before shared assistance, so they compile in the preserved pre-change source — restored
//! from its hash-verified backup into an isolated copy — and the baseline is recorded by the code
//! being compared *against*, not by the code under test. In this tree the same command records the
//! code as it now stands, under whatever label it is given.
//!
//! Ichiran's raw answers are replayed when `READER_EQUIVALENCE_RAW` (or `…_MULTILINE_RAW`) names a
//! recording, and asked for once otherwise. See `common/equivalence.rs` for what is recorded.

mod common;

#[path = "common/heteronyms.rs"]
mod heteronyms;

use common::equivalence::{self, Runner, Workload};
use sakatsu_core::analyzer::ichiran::{self, raw};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, LinderaAnalyzer, Traced};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Analysis;
use sakatsu_core::study::{self, AnalysisOptions};

/// Ichiran's recorded answer, converted as the adapter converts a live one, through the entry
/// point every version of the code has: `ichiran::to_analysis`.
struct Replayed(String);

impl Analyzer for Replayed {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }
    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        let text = sakatsu_core::analyzer::lines::normalise_breaks(text);
        let text = text.trim();
        if text.is_empty() {
            return Err(AnalyzeError::EmptyInput);
        }
        let segments = raw::parse(&self.0)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        Ok(Traced {
            analysis: ichiran::to_analysis(text, &segments),
            raw: Some(self.0.clone()),
        })
    }
    fn engine(&self) -> &str {
        "ichiran"
    }
}

#[test]
#[ignore = "needs the dictionaries, the kanji table, KWDLC, UD and the notebook, and Ichiran unless replaying"]
fn the_reader_output_is_recorded_as_the_code_stands() {
    let label = std::env::var("READER_EQUIVALENCE_LABEL")
        .expect("name what this recording is in READER_EQUIVALENCE_LABEL");
    let heteronym_texts: Vec<&str> = heteronyms::HETERONYMS
        .iter()
        .map(|case| case.text)
        .collect();
    let production = |text: &str, analyzer: &dyn Analyzer, dictionaries: &[Dictionary]| {
        study::run(text, analyzer, dictionaries, AnalysisOptions::PRODUCTION)
    };
    let lindera = LinderaAnalyzer::new().expect("compiled in");
    let recorded = equivalence::drive(
        Workload::from_env(),
        &heteronym_texts,
        &[(label.as_str(), &production as Runner<'_>)],
        &|raw| Box::new(Replayed(raw)),
        &lindera,
        &mut |_, _, _| {},
    );
    equivalence::finish(recorded);
}
