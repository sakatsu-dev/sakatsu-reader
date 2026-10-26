//! The compound / auxiliary detector.
//!
//! 伝わってきます is one token with two components, and the second one — 来る — is
//! not the verb "to come" here: it is the auxiliary of the 〜てくる pattern. Naming
//! that pattern is the whole point of this module.
//!
//! The rule that stops it over-firing (`project notes` §5): a component is an auxiliary
//! **only when it is not first and the component before it is a te-form**. Without
//! that guard, いる and ある fire on any compound that merely contains them, and the
//! reader claims a 〜ている that is not there.
//!
//! This module holds only the *matching*: lemma → pattern id. Every word of
//! explanation lives in the [grammar pack](super::pack), so the vocabulary breakdown
//! and the grammar view quote one source and cannot drift apart.

use crate::model::Token;

use super::form::Form;
use super::pack::{self, GrammarPattern};

/// An auxiliary found in a compound.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Auxiliary {
    /// Index of the auxiliary component within the token.
    pub component: usize,
    /// The pattern it forms, as a [grammar pack](super::pack) id.
    pub pattern_id: &'static str,
}

impl Auxiliary {
    /// The pattern's entry in the complete pack set. Imported entries are valid annotations too.
    pub fn pattern(&self) -> Option<&'static GrammarPattern> {
        pack::find(self.pattern_id)
    }
}

/// Auxiliary lemma → pattern id.
///
/// Both spellings are listed because Ichiran returns whichever the dictionary entry
/// uses: 居る for いた, 貰う for もらいます.
const AUXILIARIES: &[(&str, &str)] = &[
    ("来る", "te-kuru"),
    ("くる", "te-kuru"),
    ("行く", "te-iku"),
    ("いく", "te-iku"),
    ("しまう", "te-shimau"),
    ("置く", "te-oku"),
    ("おく", "te-oku"),
    ("見る", "te-miru"),
    ("みる", "te-miru"),
    ("居る", "te-iru"),
    ("いる", "te-iru"),
    ("有る", "te-aru"),
    ("ある", "te-aru"),
    ("くれる", "te-kureru"),
    ("貰う", "te-morau"),
    ("もらう", "te-morau"),
    ("上げる", "te-ageru"),
    ("あげる", "te-ageru"),
];

/// Find the auxiliary patterns in a compound token.
///
/// Returns nothing for a non-compound, and nothing for a compound whose parts do
/// not fit the te-form rule — silence is correct when there is no pattern.
pub fn detect(token: &Token) -> Vec<Auxiliary> {
    let mut found = Vec::new();
    for annotation in crate::construction::resolved(token)
        .into_iter()
        .filter(|annotation| annotation.grammar == crate::model::ConstructionGrammar::Auxiliary)
    {
        let Some(pattern_id) = crate::construction::canonical_pattern(&annotation.grammar_pattern)
        else {
            continue;
        };
        let Some(component) = annotation
            .components
            .last()
            .and_then(|index| usize::try_from(*index).ok())
        else {
            continue;
        };
        if pack::find(pattern_id).is_some() {
            found.push(Auxiliary {
                component,
                pattern_id,
            });
        }
    }
    for (index, component) in token.components.iter().enumerate() {
        // Not first: an auxiliary needs something to attach to.
        if index == 0 {
            continue;
        }
        if !is_te_form(&token.components[index - 1]) && !carries_te(component) {
            continue;
        }
        if let Some(pattern_id) = lookup(&component.dict_form) {
            if found
                .iter()
                .any(|known| known.component == index && known.pattern_id == pattern_id)
            {
                continue;
            }
            found.push(Auxiliary {
                component: index,
                pattern_id,
            });
        }
    }
    found
}

fn lookup(dict_form: &str) -> Option<&'static str> {
    AUXILIARIES
        .iter()
        .find(|(lemma, _)| *lemma == dict_form)
        .map(|(_, id)| *id)
}

/// Whether a helper is a contraction that already contains its て, so the te-form rule is met
/// inside it rather than by the component before it.
///
/// ちゃう is て+しまう and とく is て+おく: in 握っちゃう the verb's part is 握っ, which ends in no
/// て, because the て was fused into the helper. The built-in analyzer normalises such a helper to
/// its full lemma (`analyzer::lindera`, `contracted_auxiliary_components`), so this asks for both
/// together — the lemma, and a surface opening with the contraction rather than the lemma itself.
/// That pairing is what keeps the te-form guard: a component spelled しまう or おく still needs a
/// te-form before it, and a word merely starting with ち or と is never one of these lemmas.
fn carries_te(component: &Token) -> bool {
    let Some(first) = component.surface.chars().next() else {
        return false;
    };
    match component.dict_form.as_str() {
        "しまう" => matches!(first, 'ち' | 'じ'),
        "おく" | "置く" => matches!(first, 'と' | 'ど'),
        _ => false,
    }
}

/// Whether a component is a te-form: either its conjugation says so, or its surface
/// ends in て/で. The surface check catches te-forms the analyzer did not label.
fn is_te_form(component: &Token) -> bool {
    let labelled = component.conjugation.as_ref().is_some_and(|conjugation| {
        conjugation
            .props
            .iter()
            .any(|prop| Form::classify(&prop.form) == Form::Conjunctive)
    });
    labelled || component.surface.ends_with('て') || component.surface.ends_with('で')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Conjugation, Prop};

    fn te_form(surface: &str, dict_form: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: dict_form.into(),
            conjugation: Some(Conjugation {
                lemma: dict_form.into(),
                reading: String::new(),
                props: vec![Prop {
                    form: "Conjunctive (~te)".into(),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        }
    }

    fn plain(surface: &str, dict_form: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: dict_form.into(),
            ..Default::default()
        }
    }

    fn compound(parts: Vec<Token>) -> Token {
        Token {
            surface: parts.iter().map(|p| p.surface.as_str()).collect(),
            components: parts,
            ..Default::default()
        }
    }

    #[test]
    fn an_auxiliary_after_a_te_form_names_its_pattern() {
        let token = compound(vec![te_form("伝わって", "伝わる"), plain("きます", "来る")]);
        let found = detect(&token);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].component, 1);
        assert_eq!(found[0].pattern_id, "te-kuru");

        // The text comes from the pack, not from this module.
        let pattern = found[0].pattern().expect("te-kuru is in the core pack");
        assert_eq!(pattern.pattern, "〜てくる");
        assert!(!pattern.explanation.is_empty());
    }

    #[test]
    fn every_auxiliary_resolves_to_a_pattern_in_the_core_pack() {
        // The "one source of text" invariant: a lemma mapped to an id that the pack
        // does not define would show a pattern with no explanation.
        for (lemma, id) in AUXILIARIES {
            assert!(
                pack::core().get(id).is_some(),
                "{lemma} maps to {id}, which the core pack does not define"
            );
        }
    }

    #[test]
    fn both_spellings_of_an_auxiliary_are_recognized() {
        // Ichiran returns 貰う for もらいます and 居る for いた.
        let morau = compound(vec![
            te_form("言わせて", "言う"),
            plain("もらいます", "貰う"),
        ]);
        assert_eq!(detect(&morau)[0].pattern_id, "te-morau");

        let iru = compound(vec![
            te_form("食べさせられて", "食べる"),
            plain("いた", "居る"),
        ]);
        assert_eq!(detect(&iru)[0].pattern_id, "te-iru");
    }

    #[test]
    fn an_auxiliary_that_is_not_preceded_by_a_te_form_does_not_fire() {
        // The guard that stops いる/ある claiming a 〜ている in any compound that
        // happens to contain them.
        let token = compound(vec![plain("走り", "走る"), plain("いる", "居る")]);
        assert!(detect(&token).is_empty());
    }

    #[test]
    fn an_auxiliary_in_first_position_is_just_a_verb() {
        let token = compound(vec![plain("いた", "居る"), te_form("食べて", "食べる")]);
        assert!(detect(&token).is_empty());
    }

    #[test]
    fn a_te_form_surface_is_enough_when_the_analyzer_did_not_label_it() {
        let token = compound(vec![plain("読んで", "読む"), plain("おく", "置く")]);
        assert_eq!(detect(&token)[0].pattern_id, "te-oku");
    }

    #[test]
    fn a_contraction_that_carries_its_own_te_names_its_pattern() {
        // 握っちゃう and やっとく as the built-in analyzer cuts them: the verb's part ends in no
        // て, because the て is fused into the helper, which is normalised to its full lemma.
        let shimau = compound(vec![plain("握っ", "握る"), plain("ちゃう", "しまう")]);
        assert_eq!(detect(&shimau)[0].pattern_id, "te-shimau");
        let oku = compound(vec![plain("やっ", "やる"), plain("とく", "おく")]);
        assert_eq!(detect(&oku)[0].pattern_id, "te-oku");
    }

    #[test]
    fn an_uncontracted_helper_still_needs_the_te_form_before_it() {
        // The guard survives: しまう and おく spelled out are not contractions, so a
        // non-te-form before them still means no pattern.
        let token = compound(vec![plain("走り", "走る"), plain("しまう", "しまう")]);
        assert!(detect(&token).is_empty());
        let token = compound(vec![plain("書き", "書く"), plain("おく", "置く")]);
        assert!(detect(&token).is_empty());
        // And a word that merely starts with と is not one of these lemmas.
        let token = compound(vec![plain("写真", "写真"), plain("とる", "取る")]);
        assert!(detect(&token).is_empty());
    }

    #[test]
    fn a_non_compound_and_an_unknown_auxiliary_are_silent() {
        assert!(detect(&plain("食べる", "食べる")).is_empty());
        let token = compound(vec![te_form("食べて", "食べる"), plain("走る", "走る")]);
        assert!(detect(&token).is_empty());
    }
}
