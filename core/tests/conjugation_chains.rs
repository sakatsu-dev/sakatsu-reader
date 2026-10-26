//! Every derivation the reader can name, pinned so it cannot quietly stop being named.
//!
//! ```text
//! cargo test -p sakatsu-core --test conjugation_chains -- --nocapture
//! ```
//!
//! # Why this runs without anything installed, on purpose
//!
//! `project notes` names the worst property of this suite plainly: **78 of the 165 tests in
//! `core/tests` are `#[ignore]`d**, and the ones asking whether the *Japanese* is right were
//! ignored almost to a file, so "a machine with nothing installed shows green while testing almost
//! nothing." `heteronym_readings.rs` is the exception it points to as the pattern to copy — its
//! fixture is committed and UniDic is embedded, so it needs nothing and gates an ordinary run.
//!
//! Conjugation had no such test. The measurement that says the form chain is right —
//! `kwdlc_gold::conjugation_correctness_against_juman`, 96.1% against JUMAN — needs a corpus with
//! **no licence**, which can never be committed. So the number exists and guards nothing.
//!
//! This closes that. It asserts far less than the KWDLC score does and it asserts it on every run.
//!
//! # What it can and cannot say
//!
//! See `common/conjugations.rs`: the expectations are hand-written, so this **validates nothing**.
//! A hand-written table scores my beliefs about Japanese, which is the circularity `ud_gold.rs`
//! refuses when it declines to map GSD's auxiliaries. What it does is hold behaviour that an
//! independent corpus has already found correct, so a refactor cannot undo it unnoticed.
//!
//! Read the two together: KWDLC says the analyzer is right, and this says it still is.

mod common;
#[path = "common/conjugations.rs"]
mod conjugations;

use conjugations::CONJUGATIONS;
use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;

#[test]
fn every_derivation_survives_composition() {
    let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    let mut failures: Vec<String> = Vec::new();

    for (sentence, word, expected) in CONJUGATIONS {
        let analysis = analyzer.analyze(sentence).expect("analyzes");
        let Some(token) = analysis.tokens.iter().find(|token| token.surface == *word) else {
            let got: Vec<&str> = analysis
                .tokens
                .iter()
                .map(|token| token.surface.as_str())
                .collect();
            failures.push(format!("{sentence}: {word} is not one token — got {got:?}"));
            continue;
        };
        let Some(conjugation) = token.conjugation.as_ref() else {
            failures.push(format!("{sentence}: {word} carries no derivation at all"));
            continue;
        };
        let named: Vec<&str> = conjugation
            .props
            .iter()
            .map(|prop| prop.form.as_str())
            .collect();
        // Everything expected must be named; naming more is allowed, because a derivation may
        // legitimately grow a step and this should not be brittle against that.
        let missing: Vec<&&str> = expected
            .iter()
            .filter(|want| !named.contains(want))
            .collect();
        if !missing.is_empty() {
            failures.push(format!(
                "{sentence}: {word} should name {missing:?}, chain was {named:?}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} derivations are wrong:\n  {}",
        failures.len(),
        CONJUGATIONS.len(),
        failures.join("\n  "),
    );
}

/// The word each case names has to be one token, or the case is testing nothing.
///
/// **Separate from the assertion above because it fails differently.** A derivation that stops
/// being named is a bug in the composition; a word that stops being *one token* is a bug in the
/// segmentation, and it would make every expectation below it vacuous rather than wrong. Reporting
/// them together would let a segmentation change quietly empty this file out.
#[test]
fn every_case_still_names_a_real_token() {
    let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    for (sentence, word, _) in CONJUGATIONS {
        let analysis = analyzer.analyze(sentence).expect("analyzes");
        assert!(
            analysis.tokens.iter().any(|token| token.surface == *word),
            "{sentence} no longer contains {word} as one token, so its case tests nothing",
        );
    }
}
