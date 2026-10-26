//! The grammar engine: the normalized model → a breakdown a learner can read.
//!
//! It consumes [`Token`](crate::model::Token) and nothing else — no Ichiran types
//! reach this layer. That is what lets the analyzer be swapped without touching any
//! of this (`project notes` §2).
//!
//! The pieces, each testable alone:
//!
//! - [`form`] classifies a form label into a stable identity.
//! - [`derivation`] orders a word's transformations the way it is built up.
//! - [`compounds`] names the 〜てくる-style patterns an auxiliary forms.
//! - [`formation`] shows how *this* word builds *that* form, in kana.
//! - [`matcher`] finds grammar patterns across a token sequence.
//! - [`pack`] holds every word of explanatory text — code decides *whether* a
//!   pattern applies, the pack decides what is *said* about it.

pub mod compounds;
pub mod derivation;
pub mod form;
pub mod formation;
pub mod matcher;
pub mod occurrence;
pub mod pack;
pub mod particles;

pub use compounds::Auxiliary;
pub use derivation::{ladder, Step};
pub use form::Form;
pub use formation::Formation;
pub use matcher::{ElementSpan, Match};
pub use occurrence::Validated;
pub use pack::{GrammarPattern, Pack};

use crate::model::{Analysis, ConstructionGrammar, Token};

/// Everything the grammar view shows for one token.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Breakdown {
    /// The transformations, in the order the word is built up: the dictionary form
    /// first, then each layer applied to it.
    pub steps: Vec<Step>,
    /// Auxiliary patterns, when the token is a compound.
    pub auxiliaries: Vec<Auxiliary>,
}

impl Breakdown {
    /// Whether there is anything to show. A plain dictionary word has no grammar to
    /// explain, and the view should stay closed rather than show an empty panel.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.auxiliaries.is_empty()
    }
}

/// Break a token down into its grammar.
pub fn breakdown(token: &Token) -> Breakdown {
    Breakdown {
        steps: ladder(token),
        auxiliaries: compounds::detect(token),
    }
}

/// Find the grammar patterns in a whole passage.
///
/// Sentence-level, because that is the level patterns live at: a particle is a
/// relationship *between* tokens, not a property of one.
///
/// Both packs are searched, curated first so it wins where the two describe the same
/// point. The imported pack is what carries N2 and N1 — the curated one stops at N3.
pub fn patterns(analysis: &Analysis) -> Vec<Match> {
    let mut found = matcher::find_across(&analysis.tokens, &pack::all());
    found.extend(structural(&analysis.tokens));
    // `find_across` yields left to right and the panel reads in that order, so a structurally
    // found pattern has to take its place in the sentence rather than being appended after it.
    found.sort_by_key(|found| (found.first_token, found.last_token_end));
    found
}

/// Patterns identified by a token's shape rather than by matching surfaces.
///
/// **The same door the 〜て auxiliaries already come through**, and for the same reason: a rule
/// listing surfaces cannot say "the attributive copula followed by the nominaliser" without also
/// claiming every なんか on the page. [`crate::model::is_explanatory_nan`] asks the components,
/// which is the only place that distinction exists, and `pack::core()` is asked for the id so a
/// pattern that was renamed or removed stops being reported rather than being reported blind.
fn structural(tokens: &[Token]) -> Vec<Match> {
    tokens
        .iter()
        .enumerate()
        .flat_map(|(index, token)| {
            crate::construction::resolved(token)
                .into_iter()
                .filter(|annotation| annotation.grammar == ConstructionGrammar::Sentence)
                .filter(|annotation| pack::find(&annotation.grammar_pattern).is_some())
                .map(move |annotation| Match {
                    pattern_id: annotation.grammar_pattern,
                    first_token: index,
                    last_token_end: index + 1,
                    start: Some(annotation.source.start as usize),
                    end: Some(annotation.source.end as usize),
                    // **No rule ran here**, so there are no rule positions to report. The
                    // construction was established by composition and placed from its validated
                    // annotation; inventing a single element span covering the token would claim
                    // a rule structure that does not exist.
                    elements: Vec::new(),
                })
        })
        .collect()
}

/// The pack entry for な + ん. Named once, because the analyzer, the panel and the vocabulary
/// answer all have to mean the same entry.
pub const EXPLANATORY_NAN: &str = crate::construction::EXPLANATORY_NAN_PATTERN;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Conjugation, Prop};

    /// A structurally found pattern is a pattern like any other, and resolves to a real entry.
    ///
    /// **`pattern_id` on its own proves nothing** — a string can name an entry that was renamed
    /// or never existed, and the panel would then show an id to a learner. So this asks the pack
    /// for the entry and reads its prose, which is the same thing the interface does.
    ///
    /// The control is the whole reason the recogniser looks at components: a token spelled なん
    /// with nothing underneath it is the pronoun, and must come back with no pattern at all.
    #[test]
    fn the_explanatory_group_matches_a_real_pack_entry_and_the_pronoun_does_not() {
        let group = Token {
            surface: "なん".into(),
            dict_form: "だ".into(),
            start: Some(0),
            end: Some(6),
            components: vec![
                Token {
                    surface: "な".into(),
                    dict_form: "だ".into(),
                    start: Some(0),
                    end: Some(3),
                    ..Default::default()
                },
                Token {
                    surface: "ん".into(),
                    lemma_reading: "の".into(),
                    start: Some(3),
                    end: Some(6),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let analysis = Analysis {
            execution: None,
            text: "学生なんだ".into(),
            tokens: vec![group.clone()],
        };
        let found = patterns(&analysis);
        assert_eq!(
            found
                .iter()
                .map(|m| m.pattern_id.as_str())
                .collect::<Vec<_>>(),
            vec![EXPLANATORY_NAN],
            "the group is reported"
        );
        assert_eq!(found[0].first_token, 0);
        assert_eq!(
            found[0].last_token_end, 1,
            "it covers the one grouped token"
        );

        let entry = pack::core()
            .get(&found[0].pattern_id)
            .expect("the id names a shipped entry, not a dangling string");
        assert!(
            !entry.explanation.trim().is_empty(),
            "and that entry has something to say"
        );
        assert!(
            entry
                .formation
                .as_deref()
                .is_some_and(|formation| formation.contains('な') && formation.contains('の')),
            "the formation names both roles, got {:?}",
            entry.formation
        );

        // The pronoun: same spelling, no components, no pattern.
        let pronoun = Analysis {
            execution: None,
            text: "なんか".into(),
            tokens: vec![Token {
                surface: "なん".into(),
                dict_form: "何".into(),
                ..Default::default()
            }],
        };
        assert!(
            patterns(&pronoun)
                .iter()
                .all(|m| m.pattern_id != EXPLANATORY_NAN),
            "spelling alone must never reach the construction"
        );
    }

    #[test]
    fn a_plain_word_has_nothing_to_explain() {
        let token = Token {
            surface: "水".into(),
            dict_form: "水".into(),
            ..Default::default()
        };
        assert!(breakdown(&token).is_empty());
    }

    #[test]
    fn a_conjugated_word_yields_steps_but_no_pattern() {
        let token = Token {
            surface: "食べた".into(),
            dict_form: "食べる".into(),
            conjugation: Some(Conjugation {
                lemma: "食べる".into(),
                reading: "たべる".into(),
                props: vec![Prop {
                    pos: "v1".into(),
                    form: "Past (~ta)".into(),
                    via_depth: 0,
                    base_reading: "たべる".into(),
                    is_base: true,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };
        let breakdown = breakdown(&token);
        assert_eq!(breakdown.steps.len(), 1);
        assert_eq!(breakdown.steps[0].form, Form::Past);
        assert!(breakdown.auxiliaries.is_empty());
        assert!(!breakdown.is_empty());
    }
}
