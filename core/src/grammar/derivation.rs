//! The derivation ladder: the order a word is built up in.
//!
//! Ichiran tags each transformation with its depth in the `via` chain. Sorting
//! **deepest-first** gives the order a learner builds the word in — 食べる, then
//! causative-passive, then te, then past — which is the opposite of the order the
//! transformations are reported in (`project notes` §5).
//!
//! For a compound the ladder runs across the components in order, because that is
//! how the word reads: 食べさせられて carries the voice and the te, and いた carries
//! the past.
//!
//! This never invents intermediate spellings. Ichiran gives the base form and the
//! surface, not the steps between, so a step shows the transformation it applies and
//! — where [`formation`](super::formation) can build it honestly — how that step is
//! formed for this particular word. Anything else would be fabrication.

use crate::model::Token;

use super::form::Form;
use super::formation::{contractions, formation, Formation};

/// One rung of the ladder.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Step {
    pub form: Form,
    /// The analyzer's own wording, kept so an unclassified form can still be shown.
    pub label: String,
    /// The conjugation class this step applies to, e.g. `"v1"`.
    pub pos: String,
    pub neg: bool,
    pub fml: bool,
    /// The dictionary form this step's chain resolves to, e.g. 食べる.
    pub lemma: String,
    /// The dictionary form's reading in kana, on the base step.
    pub base_reading: String,
    /// Depth in the analyzer's chain; larger is deeper (closer to the lemma).
    pub via_depth: u32,
    /// True for the transformation applied directly to the dictionary word.
    pub is_base: bool,
    /// How this word builds this form, when it can be derived without guessing.
    pub formation: Option<Formation>,
    /// For a compound, which component this step came from.
    pub component: Option<usize>,
}

impl Step {
    /// The plain non-past applies nothing: it *is* the dictionary form.
    fn is_identity(&self) -> bool {
        self.form == Form::NonPast && !self.neg && !self.fml
    }

    /// The reading this step transforms, preferring what the analyzer attached to
    /// the base transformation.
    fn base_reading_or(&self, conjugation: &crate::model::Conjugation) -> String {
        if self.base_reading.is_empty() {
            conjugation.reading.clone()
        } else {
            self.base_reading.clone()
        }
    }
}

/// Build the derivation ladder for a token, deepest transformation first.
///
/// **A rejoined contraction is explained as one word.** Its parts are the two slices the source
/// spells — 間違っ | とる — and neither is a complete form: 間違っ is not a te-form and 怒ん is not
/// a negative conditional. So a token whose own conjugation records a [`Form::Contraction`] is
/// laddered from that conjugation, not from its parts: the base form, then the contraction, each
/// with the formation that builds it.
pub fn ladder(token: &Token) -> Vec<Step> {
    if token.is_compound() && !records_a_contraction(token) {
        return token
            .components
            .iter()
            .enumerate()
            .flat_map(|(index, component)| {
                ladder(component).into_iter().map(move |mut step| {
                    step.component = Some(index);
                    step
                })
            })
            .collect();
    }

    let Some(conjugation) = &token.conjugation else {
        return Vec::new();
    };

    let mut steps: Vec<Step> = conjugation
        .props
        .iter()
        .map(|prop| Step {
            form: Form::classify(&prop.form),
            label: prop.form.clone(),
            pos: prop.pos.clone(),
            neg: prop.neg,
            fml: prop.fml,
            lemma: conjugation.lemma.clone(),
            base_reading: prop.base_reading.clone(),
            via_depth: prop.via_depth,
            is_base: prop.is_base,
            formation: None,
            component: None,
        })
        .collect();

    // Deepest first: the transformation on the dictionary verb, then outwards.
    // `sort_by_key` is stable, so props at equal depth keep the analyzer's order.
    steps.sort_by_key(|step| std::cmp::Reverse(step.via_depth));

    fill_formations(
        &mut steps,
        conjugation,
        &crate::kana::to_hiragana(&token.reading),
    );
    steps
}

fn records_a_contraction(token: &Token) -> bool {
    token.conjugation.as_ref().is_some_and(|conjugation| {
        conjugation
            .props
            .iter()
            .any(|prop| Form::classify(&prop.form) == Form::Contraction)
    })
}

/// Thread the formations through the ladder: each rung is built from the **result of
/// the rung below it**, not from the dictionary form.
///
/// This matters more than it looks. 言わせて is the te-form of 言わせる — an ichidan
/// verb — not of 言う, so building it from 言う yields 言って, a real word that is
/// the wrong one. Carrying the previous rung's result forwards is the only way to
/// get 言わせる → 言わせて, and it is derivation rather than invention: every rung's
/// own POS says which class it is transforming.
///
/// When a rung cannot be built honestly the chain stops: every rung above it also
/// reports nothing, because its input is unknown. Better a silent ladder than one
/// that names words the sentence does not contain.
///
/// A [`Form::Contraction`] rung is built from the rung below it by the shared table
/// ([`contractions`]), and only the rewrite whose result is **the word's own reading** is shown:
/// the contraction is a fact about this surface, so it is read from it rather than chosen.
fn fill_formations(
    steps: &mut [Step],
    conjugation: &crate::model::Conjugation,
    surface_reading: &str,
) {
    // The chain starts at the dictionary word: the base rung's reading if the
    // analyzer supplied one, otherwise the lemma's.
    let mut carried = steps
        .iter()
        .find(|step| step.is_base)
        .map(|step| step.base_reading_or(conjugation))
        .unwrap_or_else(|| conjugation.reading.clone());

    let mut broken = carried.is_empty();
    let mut below: Option<(Form, bool)> = None;

    for step in steps.iter_mut() {
        if broken {
            continue;
        }
        step.formation = match (step.form, below) {
            (Form::Contraction, Some((form, neg))) => contractions(&carried, form, neg, &step.pos)
                .into_iter()
                .find(|contracted| contracted.formation.result == surface_reading)
                .map(|contracted| contracted.formation),
            (Form::Contraction, None) => None,
            _ => formation(&carried, &step.pos, step.form, step.neg, step.fml),
        };
        below = Some((step.form, step.neg));
        match &step.formation {
            Some(built) => carried = built.result.clone(),
            // The plain non-past is the dictionary form: no formation to show, and
            // the carried word is unchanged rather than unknown.
            None if step.is_identity() => {}
            None => broken = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Conjugation, Prop};

    fn prop(form: &str, via_depth: u32, is_base: bool) -> Prop {
        Prop {
            pos: "v1".into(),
            form: form.into(),
            via_depth,
            is_base,
            base_reading: if is_base {
                "たべる".into()
            } else {
                String::new()
            },
            ..Default::default()
        }
    }

    fn verb(props: Vec<Prop>) -> Token {
        Token {
            surface: "食べさせられて".into(),
            dict_form: "食べる".into(),
            conjugation: Some(Conjugation {
                lemma: "食べる".into(),
                reading: "たべる".into(),
                props,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn the_ladder_runs_deepest_first() {
        // Ichiran reports the outermost form first; a learner builds the word from
        // the dictionary form outwards, so the order is reversed.
        let token = verb(vec![
            prop("Conjunctive (~te)", 0, false),
            prop("Causative-Passive", 1, true),
        ]);
        let steps = ladder(&token);
        assert_eq!(
            steps.iter().map(|s| s.form).collect::<Vec<_>>(),
            [Form::CausativePassive, Form::Conjunctive]
        );
        assert!(steps[0].is_base);
        assert_eq!(steps[0].lemma, "食べる");
    }

    #[test]
    fn the_base_step_shows_how_this_word_builds_it() {
        let token = verb(vec![prop("Causative-Passive", 1, true)]);
        let formation = ladder(&token)[0].formation.clone().expect("formation");
        assert_eq!(formation.from, "たべる");
        assert_eq!(formation.result, "たべさせられる");
    }

    #[test]
    fn a_compound_ladder_spans_its_components_in_reading_order() {
        // 食べさせられていた: the voice and te live on the first component, the past
        // on the second. The ladder is 食べる → causative-passive → te → past.
        let first = verb(vec![
            prop("Conjunctive (~te)", 0, false),
            prop("Causative-Passive", 1, true),
        ]);
        let second = Token {
            surface: "いた".into(),
            dict_form: "居る".into(),
            conjugation: Some(Conjugation {
                lemma: "居る".into(),
                reading: "いる".into(),
                props: vec![Prop {
                    pos: "v1".into(),
                    form: "Past (~ta)".into(),
                    is_base: true,
                    base_reading: "いる".into(),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };
        let compound = Token {
            surface: "食べさせられていた".into(),
            components: vec![first, second],
            ..Default::default()
        };

        let steps = ladder(&compound);
        assert_eq!(
            steps.iter().map(|s| s.form).collect::<Vec<_>>(),
            [Form::CausativePassive, Form::Conjunctive, Form::Past]
        );
        assert_eq!(
            steps.iter().map(|s| s.component).collect::<Vec<_>>(),
            [Some(0), Some(0), Some(1)]
        );
    }

    #[test]
    fn a_word_with_no_conjugation_has_no_ladder() {
        let token = Token {
            surface: "水".into(),
            dict_form: "水".into(),
            ..Default::default()
        };
        assert!(ladder(&token).is_empty());
    }
}
