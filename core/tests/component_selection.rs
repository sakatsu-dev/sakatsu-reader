//! **A word's grammar and the pieces of the text it offers are two different things.**
//!
//! Ichiran decomposes a contracted auxiliary into its *expanded* grammatical parts: ほどけてる is
//! ほどけて + いる, and ついちゃった is ついて + ちゃった. Neither いる nor ついて is written in the
//! source — they are the forms the contraction stands for. The adapter used to expose them as
//! ordinary parts, so the reader's parts row would offer a "piece of the passage" that is not in
//! the passage, and a contextual lookup for a part put it into the context while dropping the
//! text it stood for. The capture notebook held ten such tokens across eight lines.
//!
//! What must hold, over real Ichiran output with nothing installed:
//!
//! - **the decomposition is kept**, so the grammar layer still finds 〜ている;
//! - **only verified slices of the source are selectable**, and nothing is trimmed or re-spanned
//!   to make an expanded form fit;
//! - **every selectable part satisfies the component contract**, which is the guarantee the parts
//!   row relies on;
//! - **no passage is refused** because a grammatical part cannot be pointed at.
//!
//! The fixtures are real `ichiran-cli -f` output for two Tatoeba sentences bundled by Jitendex
//! (CC BY-SA 4.0): ネクタイがほどけてるよ。"Your tie has come undone." and 寝癖がついちゃった。
//! "My hair got kinky." Captured by `scripts/capture-ichiran-fixtures.sh`.

use std::path::Path;

use sakatsu_core::analyzer::ichiran::{self, raw};
use sakatsu_core::analyzer::{AnalyzeError, Analyzer, Traced};
use sakatsu_core::model::{Analysis, Token};
use sakatsu_core::study::{self, AnalysisOptions};

/// Ichiran's conversion over a committed fixture, shaped as the adapter is.
struct Fixture(&'static str);

impl Analyzer for Fixture {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.analyze_traced(text).map(|traced| traced.analysis)
    }

    fn analyze_traced(&self, text: &str) -> Result<Traced, AnalyzeError> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ichiran")
            .join(format!("{}.json", self.0));
        let json = std::fs::read_to_string(&path)
            .map_err(|error| AnalyzeError::EngineUnavailable(error.to_string()))?;
        let segments = raw::parse(&json)
            .map_err(|error| AnalyzeError::BadOutput(format!("unexpected JSON: {error}")))?;
        Ok(Traced {
            analysis: ichiran::to_analysis(text, &segments),
            raw: Some(json),
        })
    }

    fn engine(&self) -> &str {
        "ichiran"
    }
}

/// The reader's path, with nothing installed. It must not refuse the passage.
fn served(fixture: &'static str, text: &str) -> Analysis {
    study::run(text, &Fixture(fixture), &[], AnalysisOptions::PRODUCTION)
        .unwrap_or_else(|failure| panic!("{fixture} was refused: {}", failure.error))
        .analysis
}

fn token<'a>(analysis: &'a Analysis, surface: &str) -> &'a Token {
    analysis
        .tokens
        .iter()
        .find(|token| token.surface == surface)
        .unwrap_or_else(|| panic!("{surface} is one token in {:?}", analysis.tokens))
}

fn parts(token: &Token) -> Vec<(&str, bool)> {
    token
        .components
        .iter()
        .map(|part| (part.surface.as_str(), part.selectable))
        .collect()
}

/// Every part the reader may select satisfies the component contract; whatever the contract
/// reports is on a part that is not offered.
fn selectable_parts_are_sound(analysis: &Analysis) {
    let faults = sakatsu_core::analyzer::validate_selectable(analysis);
    assert!(
        faults.is_empty(),
        "the offered projection broke its contract: {faults:?}"
    );
}

#[test]
fn an_expanded_te_iru_is_grammar_and_not_a_piece_of_the_text() {
    let analysis = served("hodoketeru", "ネクタイがほどけてるよ。");
    let contracted = token(&analysis, "ほどけてる");

    assert!(contracted.selectable, "the word itself is in the text");
    assert_eq!(
        parts(contracted),
        [("ほどけて", true), ("いる", false)],
        "the decomposition is kept whole; only the slice the text spells is offered"
    );
    let expanded = &contracted.components[1];
    assert_eq!(
        (expanded.start, expanded.end),
        (None, None),
        "no offset is invented for a form the text does not spell"
    );
    assert_eq!(expanded.surface, "いる", "and it is not trimmed to fit");

    // The grammar still sees 〜ている, which is the reason the decomposition is kept.
    let found: Vec<&str> = sakatsu_core::grammar::compounds::detect(contracted)
        .iter()
        .map(|auxiliary| auxiliary.pattern_id)
        .collect();
    assert!(
        found.contains(&"te-iru"),
        "the grammar layer lost 〜ている: {found:?}"
    );
    selectable_parts_are_sound(&analysis);
}

/// ついちゃった: the *first* part is the expanded one — ついて, where the text spells つい — and the
/// second, ちゃった, is really there.
#[test]
fn an_expanded_te_form_before_a_real_contraction_is_not_offered() {
    let analysis = served("tsuichatta", "寝癖がついちゃった。");
    let contracted = token(&analysis, "ついちゃった");
    assert!(contracted.selectable);
    assert_eq!(
        parts(contracted),
        [("ついて", false), ("ちゃった", true)],
        "ついて is not in the text; ちゃった is"
    );
    selectable_parts_are_sound(&analysis);
}

/// **Before, the contract failed on both; now it fails on nothing a reader can select**, and on
/// nothing else of these passages except the expanded parts themselves.
#[test]
fn the_only_contract_faults_left_are_the_parts_not_offered() {
    for (fixture, text) in [
        ("hodoketeru", "ネクタイがほどけてるよ。"),
        ("tsuichatta", "寝癖がついちゃった。"),
    ] {
        let analysis = served(fixture, text);
        let faults = sakatsu_core::analyzer::validate_components(&analysis);
        assert_eq!(faults.len(), 1, "{fixture}: {faults:?}");
        assert!(matches!(
            faults[0].kind,
            sakatsu_core::analyzer::ComponentFaultKind::Ungrounded { .. }
        ));
        selectable_parts_are_sound(&analysis);
    }
}

/// The uncontracted form is untouched: every part is written in the text and every part is
/// offered. This is the control that the rule is about spelling, not about auxiliaries.
#[test]
fn an_uncontracted_compound_offers_every_part() {
    let analysis = served("kimochi-tsutawatte", "気持ちが伝わってきます");
    let whole = token(&analysis, "伝わってきます");
    assert!(whole.components.len() >= 2);
    assert!(
        whole.components.iter().all(|part| part.selectable),
        "{:?}",
        parts(whole)
    );
    assert!(sakatsu_core::analyzer::validate_components(&analysis).is_empty());
}
