//! **The debug fault that lets a person observe both-fail preservation**, checked as a sequence.
//!
//! `SAKATSU_ANALYZER_FAULT=both` fails every passage, the first included, so a window launched
//! with it never shows a passage whose survival could be watched. `first-then-both` gives the
//! built-in analyzer one passage and then fails it too. This pins that sequence through the same
//! retry the commands run (`study::analyse_resilient`), so the checklist's row 8 is performable.
//!
//! Debug builds only, as the module is.
#![cfg(debug_assertions)]

use std::sync::Arc;

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::model::{AnalyzerPolicy, FallbackScope};
use sakatsu_core::study::{self, AnalysisOptions, Unanswered};
use sakatsu_desktop_lib::fault::{self, Mode};

fn analyse(
    route: &sakatsu_desktop_lib::route::Route,
    text: &str,
) -> Result<study::Run, Unanswered> {
    study::analyse_resilient(
        text,
        &route.plan(),
        &[],
        AnalysisOptions::PRODUCTION,
        &|| true,
    )
}

#[test]
fn first_then_both_reads_one_passage_and_then_fails_on_both_engines() {
    let built_in: Arc<dyn Analyzer> = Arc::new(LinderaAnalyzer::new().expect("compiled in"));
    let route = fault::route(AnalyzerPolicy::Automatic, built_in, Mode::FirstThenBoth);

    let first = analyse(&route, "気持ちが伝わってきます。").expect("the first passage is read");
    let execution = first.analysis.execution.expect("recorded");
    assert_eq!(execution.engine, "lindera");
    assert_eq!(
        execution.fallback.map(|fallback| fallback.scope),
        Some(FallbackScope::Passage),
        "read by the built-in analyzer after Ichiran failed on it"
    );

    for text in ["水を飲む", "気持ち"] {
        match analyse(&route, text) {
            Err(Unanswered::BothFailed { .. }) => {}
            other => panic!("{text}: expected both engines to fail, got {other:?}"),
        }
    }
}

#[test]
fn both_fails_from_the_first_passage() {
    let built_in: Arc<dyn Analyzer> = Arc::new(LinderaAnalyzer::new().expect("compiled in"));
    let route = fault::route(AnalyzerPolicy::Automatic, built_in, Mode::Both);
    assert!(matches!(
        analyse(&route, "気持ちが伝わってきます。"),
        Err(Unanswered::BothFailed { .. })
    ));
}

#[test]
fn empty_input_does_not_spend_the_one_passage() {
    let built_in: Arc<dyn Analyzer> = Arc::new(LinderaAnalyzer::new().expect("compiled in"));
    let route = fault::route(AnalyzerPolicy::Automatic, built_in, Mode::FirstThenBoth);
    assert!(analyse(&route, "   ").is_err(), "empty input is refused");
    assert!(
        analyse(&route, "水を飲む").is_ok(),
        "the one passage is still there after an empty submission"
    );
}
