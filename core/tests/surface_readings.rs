//! The displayed surface reading, against reviewed expectations.
//!
//! # Why this gates an ordinary `cargo test`
//!
//! It needs **nothing installed**. The fixture is committed, the analyzer is the embedded UniDic,
//! and the readings under test are produced before any dictionary is consulted — which is exactly
//! where the defects it pins live. `project notes` names that property as the one worth copying, and
//! `heteronym_readings.rs` is the pattern: a machine with no models and no dictionaries shows green
//! while testing almost nothing, unless the test was built not to need them.
//!
//! # What it is really guarding
//!
//! UniDic records 語彙素読み — the reading of the **lexeme** — and 書字形基本形, the written base.
//! The adapter aligns one against the other, which is correct only while they are the same word.
//! For a potential verb they are not: 走れる's lexeme is 走る, so ハシル was aligned against 走れる,
//! lost one mora to the る, and 走れた was shown as はした.
//!
//! **The failure was never uniform, and that is the point.** 泳げます came out right, because およぐ
//! has no る to strip and the alignment declined; 走れる came out wrong because はしる does. Whether
//! a reader saw the right reading turned on whether an unrelated suffix happened to match. So this
//! file carries both — the ones that were wrong and the ones that were accidentally right — because
//! a repair that fixes the first while moving the second has not understood either.
//!
//! Every `Control` here is a case that some slightly wider version of the repair breaks, and its
//! `evidence` line says which one.

#[path = "common/reading_targets.rs"]
mod reading_targets;
#[path = "common/runner.rs"]
mod runner;

use reading_targets::{Kind, ReadingTarget, READING_TARGETS};
use runner::{Outcome, Profile, Run, Stage};
use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};

/// How many cases must be reached before a rate means anything.
///
/// Copied from `heteronym_readings.rs` for the reason given there: a suite that quietly stops
/// selecting cases reports a clean sweep over nothing.
const MIN_CHECKED: usize = 25;

#[test]
fn reviewed_surface_readings_are_produced_by_the_analyzer() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    let ids: Vec<String> = READING_TARGETS
        .iter()
        .map(|target| target.id.to_string())
        .collect();
    assert_eq!(
        ids.len(),
        READING_TARGETS.len(),
        "every target needs an id, and they are the run's manifest"
    );
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        ids.len(),
        "two targets share an id, so one of them would never be scored"
    );

    let mut run = Run::new(Profile::Hermetic, ids).engine("lindera");
    let mut conversions = 0usize;
    let mut controls = 0usize;

    for target in READING_TARGETS {
        match outcome_for(&analyzer, target) {
            Ok(()) => {
                match target.kind {
                    Kind::Conversion => conversions += 1,
                    Kind::Control => controls += 1,
                }
                run.record(target.id, Outcome::Passed);
            }
            Err(detail) => run.record(target.id, Outcome::failed(Stage::Analyzer, detail)),
        }
    }

    println!(
        "  reviewed       {} conversion targets, {} controls, {} cases",
        conversions,
        controls,
        READING_TARGETS.len()
    );
    assert!(
        READING_TARGETS.len() >= MIN_CHECKED,
        "only {} targets are declared; below {MIN_CHECKED} the result says little",
        READING_TARGETS.len()
    );
    run.finish().expect_success();
}

/// What one target asks of the analyzer, and what its failure should say.
///
/// The token is found by **exact surface**, not by containment: a repair that changed segmentation
/// so that 走れない stopped being one token would otherwise keep passing against whatever fragment
/// still matched, which is the failure mode this most needs to avoid.
fn outcome_for(analyzer: &LinderaAnalyzer, target: &ReadingTarget) -> Result<(), String> {
    let analysis = analyzer
        .analyze(target.text)
        .map_err(|error| format!("the analyzer failed: {error}"))?;

    let Some(token) = analysis
        .tokens
        .iter()
        .find(|token| token.surface == target.surface)
        .or_else(|| {
            // A composed token that *ends* at the same place still answers the question, as long as
            // it contains the surface — 来てくれません carries 来て. Reported either way, so a
            // segmentation change is visible in the failure rather than silently absorbed.
            analysis
                .tokens
                .iter()
                .find(|token| token.surface.contains(target.surface))
        })
    else {
        let surfaces: Vec<&str> = analysis
            .tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        return Err(format!(
            "{:?} is not a token of {:?}; the analyzer produced {surfaces:?}",
            target.surface, target.text
        ));
    };

    if token.reading == target.reading {
        return Ok(());
    }
    Err(format!(
        "{:?} read as {:?}, expected {:?} ({}; {}; source: {})",
        token.surface,
        token.reading,
        target.reading,
        match target.kind {
            Kind::Conversion => "a demonstrated conversion defect",
            Kind::Control => "a control that must not move",
        },
        target.evidence,
        target.source,
    ))
}

/// **The fixture has to be about this analyzer's output, or it is about nothing.**
///
/// A target naming a surface the analyzer never emits would pass through the containment fallback
/// above and quietly test a different token. This asserts every surface is either a token exactly
/// or a proper part of one, so the fallback is a convenience rather than a hiding place.
#[test]
fn every_target_names_a_token_the_analyzer_actually_produces() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");
    let mut absent = Vec::new();

    for target in READING_TARGETS {
        let Ok(analysis) = analyzer.analyze(target.text) else {
            absent.push(format!("{}: the analyzer failed", target.id));
            continue;
        };
        if !analysis
            .tokens
            .iter()
            .any(|token| token.surface.contains(target.surface))
        {
            absent.push(format!(
                "{}: {:?} appears in no token of {:?}",
                target.id, target.surface, target.text
            ));
        }
    }

    assert!(absent.is_empty(), "unreachable targets: {absent:?}");
}

/// Each expectation says where it came from, and neither field may be blank.
///
/// `evidence` is the reason to believe the reading, and `source` is where the sentence came from.
/// A fixture row without both is an assertion with nobody behind it, and this project has already
/// recorded what that costs: `local documentation` describes a reading suite reporting 100%
/// while exempting the words most likely to be wrong.
#[test]
fn every_expectation_records_its_provenance() {
    for target in READING_TARGETS {
        assert!(
            !target.evidence.trim().is_empty(),
            "{}: no evidence recorded for expecting {:?}",
            target.id,
            target.reading
        );
        assert!(
            !target.source.trim().is_empty(),
            "{}: no source recorded for the sentence",
            target.id
        );
        assert!(
            !target.reading.trim().is_empty(),
            "{}: an empty expected reading asserts nothing",
            target.id
        );
    }
}

/// Both kinds must be present, because each is checking the other.
///
/// Targets alone would pass for a rule that rewrote every reading; controls alone would pass for a
/// rule that did nothing. A run that lost one kind would still be green.
#[test]
fn the_fixture_holds_both_targets_and_controls() {
    let conversions = READING_TARGETS
        .iter()
        .filter(|target| target.kind == Kind::Conversion)
        .count();
    let controls = READING_TARGETS
        .iter()
        .filter(|target| target.kind == Kind::Control)
        .count();
    assert!(
        conversions >= 5,
        "only {conversions} conversion targets; the repair is barely described"
    );
    assert!(
        controls >= conversions,
        "{controls} controls against {conversions} targets: a repair is only as trustworthy as \
         what it is shown not to reach"
    );
}
