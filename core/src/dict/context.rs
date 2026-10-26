//! Conservative, sentence-local ordering for the Analysis page.
//!
//! A dictionary lookup knows what a word *can* mean. This module asks the narrower
//! question the Analysis page has enough context to answer: whether morphology or a
//! nearby construction supports one of those senses more strongly than the others.
//! It deliberately does not inspect English gloss text, corpus frequency, JLPT level,
//! dictionary position, translation output, or an analyzer's whole-word confidence.
//! None of those is evidence for a particular sense.

use std::cmp::Reverse;

use serde::{Deserialize, Serialize};

use crate::model::{LexicalRole, Token};

use super::{
    merge::{Retrieved, Sense},
    WordView,
};

/// One primary sense's evidence, parallel to the reordered [`WordView::inline`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(rename = "VocabularySenseRank"))]
pub struct RankedSense {
    /// Where this sense appeared before contextual ordering.
    pub original_index: usize,
    /// Additive evidence. Zero means the context supplied no usable distinction.
    pub score: i32,
    /// Short, learner-readable statements describing only evidence that actually fired.
    pub reasons: Vec<String>,
}

/// The complete ranking decision for a contextual lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct VocabularyRanking {
    /// Parallel to the returned word's `inline` senses, after ordering.
    pub senses: Vec<RankedSense>,
    /// Index in that reordered list, only for a unique positive winner.
    pub favoured: Option<usize>,
}

/// One Analysis-page lookup: the ordinary lossless word plus its contextual decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ContextualWordView {
    pub word: WordView,
    pub ranking: VocabularyRanking,
}

#[derive(Debug)]
struct Candidate {
    original_index: usize,
    sense: Sense,
    score: i32,
    reasons: Vec<String>,
}

/// Reorder a word's primary senses by attributable sentence evidence.
///
/// Positive senses lead, strongest first. Equal scores and unsupported senses retain
/// source order. `also_in`, examples, names, and every other field remain byte-for-byte
/// untouched. A favourite is made only when one positive score is strictly greater than
/// every other one; otherwise the result explicitly abstains.
pub fn contextualize(word: WordView, tokens: &[Token], index: usize) -> ContextualWordView {
    let trustworthy_lookup = !word.likely_name;
    contextualize_with_lookup(word, tokens, index, trustworthy_lookup)
}

/// Rank with the lookup layer's exact-versus-folded provenance.
///
/// Analysis uses this entry point because `WordView` intentionally does not expose dictionary
/// search mechanics to every caller. A folded-only promoted entry may still be shown, but its
/// morphology is not allowed to turn that fallback into a "Likely here" claim.
pub fn contextualize_with_lookup(
    mut word: WordView,
    tokens: &[Token],
    index: usize,
    trustworthy_lookup: bool,
) -> ContextualWordView {
    // `likely_name` means every dictionary hit came from a folded spelling. Morphology may
    // still describe the token, but it cannot turn an entry that was not actually spelled this
    // way into a context-supported meaning. Keep the caveated definitions losslessly and
    // abstain until the lookup layer has an exact entry to rank.
    // A component answer may be useful, but the selected token's morphology describes the whole
    // token. It cannot establish which sense of a different component was used. Historical
    // responses with unknown scope abstain for the same reason.
    let selected = (trustworthy_lookup && matches!(word.inline_retrieved, Retrieved::Whole { .. }))
        .then(|| tokens.get(index))
        .flatten();
    let mut candidates: Vec<Candidate> = word
        .inline
        .drain(..)
        .enumerate()
        .map(|(original_index, sense)| {
            let (score, reasons) = selected
                .map(|token| evidence(&sense, token, tokens, index))
                .unwrap_or_default();
            Candidate {
                original_index,
                sense,
                score,
                reasons,
            }
        })
        .collect();

    // Stable sorting plus the original index makes the preservation rule obvious at the
    // boundary: support may move a sense, but a tie never silently rewrites dictionary order.
    candidates.sort_by_key(|candidate| {
        (
            candidate.score <= 0,
            Reverse(candidate.score.max(0)),
            candidate.original_index,
        )
    });

    let favoured_original = unique_positive_winner(&candidates);
    let favoured = favoured_original.and_then(|winner| {
        candidates
            .iter()
            .position(|candidate| candidate.original_index == winner)
    });

    let senses = candidates
        .iter()
        .map(|candidate| RankedSense {
            original_index: candidate.original_index,
            score: candidate.score,
            reasons: candidate.reasons.clone(),
        })
        .collect();
    word.inline = candidates
        .into_iter()
        .map(|candidate| candidate.sense)
        .collect();

    ContextualWordView {
        word,
        ranking: VocabularyRanking { senses, favoured },
    }
}

fn unique_positive_winner(candidates: &[Candidate]) -> Option<usize> {
    let best = candidates.iter().map(|candidate| candidate.score).max()?;
    if best <= 0
        || candidates
            .iter()
            .filter(|candidate| candidate.score == best)
            .count()
            != 1
    {
        return None;
    }
    candidates
        .iter()
        .find(|candidate| candidate.score == best)
        .map(|candidate| candidate.original_index)
}

/// Evidence that can distinguish a sense without reading its English definition.
fn evidence(sense: &Sense, token: &Token, tokens: &[Token], index: usize) -> (i32, Vec<String>) {
    let sense_tags: Vec<String> = sense.tags.iter().flat_map(|tag| parse_tags(tag)).collect();
    let mut score = 0;
    let mut reasons = Vec::new();

    // An inflection is the strongest evidence currently shared by both analyzers. It says
    // what kind of predicate the surface is without guessing from an English gloss.
    if let Some(conjugation) = &token.conjugation {
        let classes: Vec<String> = conjugation
            .props
            .iter()
            .flat_map(|prop| parse_tags(&prop.pos))
            .filter(|tag| verb_class(tag) || adjective_class(tag))
            .collect();
        if classes.iter().any(|class| sense_tags.contains(class)) {
            score += 5;
            reasons.push("The analyzed inflection matches this word class.".into());
        } else if classes.iter().any(|class| verb_class(class))
            && sense_tags.iter().any(|tag| verb_tag(tag))
        {
            score += 3;
            reasons.push("The selected word is inflected as a verb.".into());
        } else if classes.iter().any(|class| adjective_class(class))
            && sense_tags.iter().any(|tag| adjective_tag(tag))
        {
            score += 3;
            reasons.push("The selected word is inflected as an adjective.".into());
        }
    }

    // Ichiran may state per-sense POS even for an uninflected word. Use it only when the
    // selected analysis has one unambiguous broad category; a union spanning noun and verb
    // would merely repeat the dictionary's ambiguity back as evidence.
    let analyzed = analyzed_categories(token);
    if analyzed.len() == 1 && sense_matches_category(&sense_tags, analyzed[0]) {
        score += 2;
        reasons.push(format!(
            "The analyzer identifies this use as {}.",
            analyzed[0].learner_label()
        ));
    }

    let previous = previous_word(tokens, index).map(|token| token.surface.as_str());
    let next = next_word(tokens, index).map(|token| token.surface.as_str());

    // を immediately before an inflected verb is direct structural support for a
    // transitive reading. The same particle farther away is not used: it may belong to a
    // different predicate, and pretending otherwise would turn proximity into syntax.
    if previous == Some("を") && sense_tags.iter().any(|tag| tag == "vt") && token_is_verb(token) {
        score += 4;
        reasons.push("The nearby を supports a transitive verb reading.".into());
    }

    // A noun directly followed by する is the lexical half of a する construction.
    // This can separate a verbal-noun sense from unrelated readings without inspecting
    // the definition text.
    if next.is_some_and(is_suru)
        && sense_tags
            .iter()
            .any(|tag| tag == "vs" || tag.starts_with("vs-"))
    {
        score += 4;
        reasons.push("It is followed by する as a verbal noun.".into());
    }

    // These two attributive shapes are explicit enough to be useful and narrow enough not
    // to turn every nearby noun into a guess. A な-adjective must supply な; an inflected
    // い-adjective directly modifies the following word.
    if next == Some("な")
        && sense_tags
            .iter()
            .any(|tag| tag == "adj-na" || tag == "形容動詞")
    {
        score += 4;
        reasons.push("The following な supports a な-adjective reading.".into());
    }
    if next.is_some_and(is_content_word)
        && token.surface.ends_with('い')
        // A spelling ending in い is not morphological evidence: ぽいと投げる is an
        // adverbial sound, not the adjective suffix っぽい. Prose-tag normalization exposed
        // that false favourite in the filed-entry evaluation. Require an analyzer claim too.
        && (analyzed == [Category::IAdjective]
            || token.conjugation.as_ref().is_some_and(|conjugation| {
                conjugation.props.iter().flat_map(|prop| parse_tags(&prop.pos))
                    .any(|tag| matches!(tag.as_str(), "adj-i" | "形容詞"))
            }))
        && sense_tags
            .iter()
            .any(|tag| tag == "adj-i" || tag == "形容詞")
    {
        score += 3;
        reasons.push("Its form directly modifies the following word as an い-adjective.".into());
    }

    (score, reasons)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Verb,
    IAdjective,
    NaAdjective,
    Noun,
    Adverb,
    Expression,
    Pronoun,
    Suffix,
}

impl Category {
    fn learner_label(self) -> &'static str {
        match self {
            Self::Verb => "a verb",
            Self::IAdjective => "an い-adjective",
            Self::NaAdjective => "a な-adjective",
            Self::Noun => "a noun",
            Self::Adverb => "an adverb",
            Self::Expression => "an expression",
            Self::Pronoun => "a pronoun",
            Self::Suffix => "a suffix",
        }
    }
}

fn analyzed_categories(token: &Token) -> Vec<Category> {
    let mut categories = Vec::new();
    if let Some(category) = match token.lexical_role {
        Some(LexicalRole::Pronoun) => Some(Category::Pronoun),
        Some(LexicalRole::Suffix) => Some(Category::Suffix),
        // A noun is the unmarked case for sense ranking: every dictionary's first senses are
        // nominal, so promoting them would be promoting everything. The role is carried for the
        // selector, which asks a different question.
        Some(LexicalRole::Noun) | None => None,
    } {
        categories.push(category);
    }
    for sense in &token.senses {
        for tag in parse_tags(&sense.pos) {
            if let Some(category) = category(&tag) {
                if !categories.contains(&category) {
                    categories.push(category);
                }
            }
        }
    }
    categories
}

fn category(tag: &str) -> Option<Category> {
    if verb_tag(tag) {
        Some(Category::Verb)
    } else if matches!(tag, "adj-i" | "形容詞") {
        Some(Category::IAdjective)
    } else if matches!(tag, "adj-na" | "形容動詞") {
        Some(Category::NaAdjective)
    } else if matches!(tag, "pn" | "pron" | "pronoun" | "代名詞") {
        Some(Category::Pronoun)
    } else if tag == "suf" || tag == "n-suf" || tag == "suffix" || tag == "接尾辞" {
        Some(Category::Suffix)
    } else if tag == "n" || tag.starts_with("n-") || tag == "名詞" {
        Some(Category::Noun)
    } else if tag == "adv" || tag.starts_with("adv-") || tag == "副詞" {
        Some(Category::Adverb)
    } else if matches!(tag, "exp" | "expression" | "連語") {
        Some(Category::Expression)
    } else {
        None
    }
}

fn sense_matches_category(tags: &[String], wanted: Category) -> bool {
    tags.iter()
        .filter_map(|tag| category(tag))
        .any(|found| found == wanted)
}

fn parse_tags(value: &str) -> Vec<String> {
    let value = value.trim();
    // Resolve a complete prose label before splitting the analyzer's compact code lists.
    // Display tags stay as the dictionary wrote them; only the evidence uses this vocabulary.
    if let Some(code) = super::tags::canonical_code(value) {
        return vec![code.to_string()];
    }
    // Preserve the previously accepted parenthesized code-list form without treating the
    // parentheses inside a prose label, such as noun (temporal), as separators.
    let value = value
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(value);
    value
        .split([',', ';', '[', ']'])
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .flat_map(|tag| {
            if let Some(code) = super::tags::canonical_code(tag) {
                return vec![code.to_string()];
            }
            // Whitespace separates compact code lists, not words inside an unknown prose
            // label. Otherwise "not a noun" would quietly acquire noun evidence.
            let codes: Vec<_> = tag.split_whitespace().collect();
            if codes.iter().all(|code| {
                super::tags::canonical_code(code)
                    .is_some_and(|known| known.eq_ignore_ascii_case(code))
                    || matches!(*code, "v" | "v5")
            }) {
                codes.iter().map(|code| code.to_ascii_lowercase()).collect()
            } else {
                vec![tag.to_ascii_lowercase()]
            }
        })
        .collect()
}

fn verb_class(tag: &str) -> bool {
    tag == "vk"
        || tag == "vz"
        || tag.starts_with("v1")
        || tag.starts_with("v5")
        || tag.starts_with("vs")
}

fn adjective_class(tag: &str) -> bool {
    matches!(tag, "adj-i" | "adj-na" | "形容詞" | "形容動詞")
}

fn verb_tag(tag: &str) -> bool {
    verb_class(tag) || matches!(tag, "v" | "verb" | "動詞" | "vt" | "vi" | "aux-v")
}

fn adjective_tag(tag: &str) -> bool {
    adjective_class(tag) || tag.starts_with("adj-")
}

fn token_is_verb(token: &Token) -> bool {
    token.conjugation.is_some()
        || token
            .senses
            .iter()
            .flat_map(|sense| parse_tags(&sense.pos))
            .any(|tag| verb_tag(&tag))
}

fn previous_word(tokens: &[Token], index: usize) -> Option<&Token> {
    tokens
        .get(index.checked_sub(1)?)
        .filter(|token| is_content_word(&token.surface))
}

fn next_word(tokens: &[Token], index: usize) -> Option<&Token> {
    tokens
        .get(index.checked_add(1)?)
        .filter(|token| is_content_word(&token.surface))
}

fn is_content_word(surface: &str) -> bool {
    surface.chars().any(|character| {
        !character.is_whitespace()
            && !matches!(
                character,
                '。' | '、' | '！' | '？' | '!' | '?' | '「' | '」' | '『' | '』' | '（' | '）'
            )
    })
}

fn is_suru(surface: &str) -> bool {
    matches!(surface, "する" | "します" | "した" | "して" | "し")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dict::merge::{AlsoIn, NameHit, Retrieved},
        model::{Conjugation, Prop, Sense as TokenSense},
    };

    fn sense(tags: &str, gloss: &str) -> Sense {
        Sense {
            tags: tags.split(',').map(str::to_owned).collect(),
            glosses: vec![gloss.into()],
            sequence: None,
        }
    }

    fn word(inline: Vec<Sense>) -> WordView {
        WordView {
            headword: "伝える".into(),
            reading: Some("つたえる".into()),
            inline,
            inline_source: None,
            inline_retrieved: Retrieved::Whole {
                query: "伝える".into(),
            },
            also_in: vec![AlsoIn {
                title: "another source".into(),
                retrieved: Retrieved::Unknown,
                senses: vec![sense("n", "untouched")],
            }],
            examples: Vec::new(),
            name: Some(NameHit {
                glosses: vec!["name".into()],
                tags: vec!["given".into()],
                source: "names".into(),
                retrieved: Retrieved::Unknown,
                leads: false,
            }),
            likely_name: false,
            romaji: None,
        }
    }

    fn verb(surface: &str, class: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: "伝える".into(),
            senses: vec![TokenSense {
                pos: class.into(),
                gloss: String::new(),
                sequence: None,
            }],
            conjugation: Some(Conjugation {
                lemma: "伝える".into(),
                reading: "つたえる".into(),
                props: vec![Prop {
                    pos: class.into(),
                    form: "Past".into(),
                    is_base: true,
                    ..Prop::default()
                }],
            }),
            ..Token::default()
        }
    }

    #[test]
    fn unique_morphology_support_leads_and_names_a_favourite() {
        let original = word(vec![sense("n", "noun"), sense("v1,vt", "verb")]);
        let context = contextualize(original, &[verb("伝えた", "v1")], 0);
        assert_eq!(context.word.inline[0].glosses, ["verb"]);
        assert_eq!(context.ranking.favoured, Some(0));
        assert!(context.ranking.senses[0].score > 0);
        assert_eq!(context.ranking.senses[0].original_index, 1);
    }

    #[test]
    fn promoted_prose_tags_reach_the_same_evidence_without_analyzer_senses() {
        let mut token = verb("伝えた", "v1");
        token.senses.clear(); // Lindera supplies morphology, but no dictionary senses.
        let codes = contextualize(
            word(vec![sense("n", "noun"), sense("v1,vt", "verb")]),
            &[token.clone()],
            0,
        );
        let prose = contextualize(
            word(vec![
                sense("noun", "noun"),
                sense("1-dan,transitive", "verb"),
            ]),
            &[token],
            0,
        );
        assert_eq!(prose.ranking, codes.ranking);
        assert_eq!(prose.word.inline[0].glosses, ["verb"]);
        assert_eq!(prose.word.inline[0].tags, ["1-dan", "transitive"]);
        assert_eq!(parse_tags("noun or verb acting prenominally"), ["adj-f"]);
        assert_eq!(parse_tags("not a noun"), ["not a noun"]);
        assert_eq!(parse_tags("[v1,vt]"), ["v1", "vt"]);
        assert_eq!(parse_tags("(v1,vt)"), ["v1", "vt"]);
        assert_eq!(parse_tags("[noun (temporal),vt]"), ["n-t", "vt"]);
        assert_eq!(parse_tags("n vs vt"), ["n", "vs", "vt"]);
    }

    #[test]
    fn an_i_ending_without_adjective_analysis_does_not_assert_an_adjective_sense() {
        let tokens = [
            Token {
                surface: "ぽい".into(),
                ..Token::default()
            },
            Token {
                surface: "と".into(),
                ..Token::default()
            },
        ];
        let context = contextualize(
            word(vec![
                sense("suffix,adjective", "adjective suffix"),
                sense("adverb", "a tossing sound"),
            ]),
            &tokens,
            0,
        );
        assert_eq!(context.ranking.favoured, None);
        assert!(context.ranking.senses.iter().all(|sense| sense.score == 0));
    }

    #[test]
    fn an_explicit_suffix_role_promotes_suffix_senses_without_hiding_alternatives() {
        let original = word(vec![
            sense("n", "long sword"),
            sense("suf", "pluralizing suffix"),
            sense("n", "other noun"),
        ]);
        let token = Token {
            surface: "たち".into(),
            dict_form: "たち".into(),
            lexical_role: Some(LexicalRole::Suffix),
            ..Token::default()
        };
        let context = contextualize(original, &[token], 0);
        assert_eq!(context.word.inline[0].glosses, ["pluralizing suffix"]);
        assert_eq!(
            context.word.inline.len(),
            3,
            "ranking never filters alternatives"
        );
        assert_eq!(context.ranking.favoured, Some(0));
        assert_eq!(context.ranking.senses[0].original_index, 1);
    }

    #[test]
    fn pronoun_role_is_not_inferred_from_a_homophone_or_from_missing_morphology() {
        let original = || word(vec![sense("n", "difficulty"), sense("pn", "what")]);
        let pronoun = Token {
            surface: "なん".into(),
            dict_form: "なん".into(),
            lexical_role: Some(LexicalRole::Pronoun),
            ..Token::default()
        };
        let ranked = contextualize(original(), &[pronoun], 0);
        assert_eq!(ranked.word.inline[0].glosses, ["what"]);
        assert_eq!(ranked.ranking.favoured, Some(0));

        for token in [
            Token {
                surface: "難".into(),
                dict_form: "難".into(),
                ..Token::default()
            },
            Token {
                surface: "なん".into(),
                dict_form: "なん".into(),
                ..Token::default()
            },
        ] {
            let unranked = contextualize(original(), &[token], 0);
            assert_eq!(unranked.word.inline[0].glosses, ["difficulty"]);
            assert_eq!(unranked.ranking.favoured, None);
        }
    }

    #[test]
    fn every_supported_sense_leads_in_score_then_source_order() {
        let original = word(vec![
            sense("n", "noun"),
            sense("v1,vi", "verb one"),
            sense("v1,vt", "verb two"),
        ]);
        let tokens = vec![
            Token {
                surface: "を".into(),
                ..Token::default()
            },
            verb("伝えた", "v1"),
        ];
        let context = contextualize(original, &tokens, 1);
        let glosses: Vec<_> = context
            .word
            .inline
            .iter()
            .map(|sense| sense.glosses[0].as_str())
            .collect();
        assert_eq!(glosses, ["verb two", "verb one", "noun"]);
        assert_eq!(context.ranking.favoured, Some(0));
    }

    #[test]
    fn inflected_adjective_supports_the_adjective_sense() {
        let original = word(vec![sense("n", "noun"), sense("adj-i", "adjective")]);
        let mut token = verb("珍しく", "adj-i");
        token.dict_form = "珍しい".into();
        token.conjugation.as_mut().expect("conjugation").lemma = "珍しい".into();
        let context = contextualize(original, &[token], 0);
        assert_eq!(context.word.inline[0].glosses, ["adjective"]);
        assert_eq!(context.ranking.favoured, Some(0));
    }

    #[test]
    fn noun_ambiguity_abstains_instead_of_using_dictionary_position_as_evidence() {
        let original = word(vec![sense("n", "first noun"), sense("n", "second noun")]);
        let token = Token {
            surface: "物".into(),
            senses: vec![TokenSense {
                pos: "n".into(),
                gloss: String::new(),
                sequence: None,
            }],
            ..Token::default()
        };
        let context = contextualize(original, &[token], 0);
        let glosses: Vec<_> = context
            .word
            .inline
            .iter()
            .map(|sense| sense.glosses[0].as_str())
            .collect();
        assert_eq!(glosses, ["first noun", "second noun"]);
        assert_eq!(context.ranking.favoured, None);
    }

    #[test]
    fn a_folded_spelling_is_never_promoted_as_contextual_proof() {
        let mut original = word(vec![sense("n", "folded dictionary hit")]);
        original.likely_name = true;
        let token = Token {
            surface: "シロコ".into(),
            senses: vec![TokenSense {
                pos: "n".into(),
                gloss: String::new(),
                sequence: None,
            }],
            ..Token::default()
        };
        let context = contextualize(original, &[token], 0);
        assert_eq!(context.word.inline[0].glosses, ["folded dictionary hit"]);
        assert_eq!(context.ranking.favoured, None);
        assert_eq!(context.ranking.senses[0].score, 0);
    }

    #[test]
    fn an_exact_spelling_may_use_the_same_analyzer_evidence() {
        let original = word(vec![sense("n", "exact dictionary hit")]);
        let token = Token {
            surface: "物".into(),
            senses: vec![TokenSense {
                pos: "n".into(),
                gloss: String::new(),
                sequence: None,
            }],
            ..Token::default()
        };
        let context = contextualize(original, &[token], 0);
        assert_eq!(context.ranking.favoured, Some(0));
        assert!(context.ranking.senses[0].score > 0);
    }

    #[test]
    fn component_and_unknown_answers_do_not_borrow_the_whole_tokens_morphology() {
        let token = Token {
            surface: "神姫".into(),
            lexical_role: Some(LexicalRole::Pronoun),
            ..Token::default()
        };
        for scope in [
            Retrieved::Component {
                query: "神".into()
            },
            Retrieved::Unknown,
        ] {
            let mut answer = word(vec![sense("pn", "pronoun"), sense("n", "noun")]);
            answer.inline_retrieved = scope;
            let result = contextualize(answer, std::slice::from_ref(&token), 0);
            assert_eq!(result.ranking.favoured, None);
            assert!(result.ranking.senses.iter().all(|sense| sense.score == 0));
            assert_eq!(result.word.inline[0].glosses, ["pronoun"]);
        }
    }

    #[test]
    fn suru_construction_supports_a_verbal_noun_without_hiding_other_senses() {
        let original = word(vec![sense("n", "noun"), sense("vs", "verbal noun")]);
        let tokens = vec![
            Token {
                surface: "勉強".into(),
                ..Token::default()
            },
            Token {
                surface: "する".into(),
                ..Token::default()
            },
        ];
        let context = contextualize(original, &tokens, 0);
        assert_eq!(context.word.inline[0].glosses, ["verbal noun"]);
        assert_eq!(context.word.inline[1].glosses, ["noun"]);
        assert_eq!(context.ranking.favoured, Some(0));
    }

    #[test]
    fn punctuation_blocks_a_proximity_cue() {
        let original = word(vec![sense("n", "noun"), sense("vs", "verbal noun")]);
        let tokens = vec![
            Token {
                surface: "勉強".into(),
                ..Token::default()
            },
            Token {
                surface: "!".into(),
                ..Token::default()
            },
            Token {
                surface: "する".into(),
                ..Token::default()
            },
        ];
        let context = contextualize(original, &tokens, 0);
        assert_eq!(context.word.inline[0].glosses, ["noun"]);
        assert_eq!(context.ranking.favoured, None);
    }

    #[test]
    fn positive_tie_abstains_and_preserves_source_order() {
        let original = word(vec![sense("v1,vi", "first"), sense("v1,vt", "second")]);
        let context = contextualize(original, &[verb("伝えた", "v1")], 0);
        let glosses: Vec<_> = context
            .word
            .inline
            .iter()
            .map(|sense| sense.glosses[0].as_str())
            .collect();
        assert_eq!(glosses, ["first", "second"]);
        assert_eq!(context.ranking.favoured, None);
        assert!(context.ranking.senses.iter().all(|sense| sense.score > 0));
    }

    #[test]
    fn all_zero_abstains_and_preserves_everything() {
        let original = word(vec![sense("n", "first"), sense("n", "second")]);
        let elsewhere = original.also_in.clone();
        let name = original.name.clone();
        let context = contextualize(
            original,
            &[Token {
                surface: "物".into(),
                ..Token::default()
            }],
            0,
        );
        let glosses: Vec<_> = context
            .word
            .inline
            .iter()
            .map(|sense| sense.glosses[0].as_str())
            .collect();
        assert_eq!(glosses, ["first", "second"]);
        assert_eq!(context.ranking.favoured, None);
        assert!(context.ranking.senses.iter().all(|sense| sense.score == 0));
        assert_eq!(context.word.also_in, elsewhere);
        assert_eq!(context.word.name, name);
    }

    #[test]
    fn each_input_index_survives_exactly_once_for_small_lists() {
        for length in 0..8 {
            let original = word(
                (0..length)
                    .map(|index| sense(if index % 2 == 0 { "n" } else { "v1" }, &index.to_string()))
                    .collect(),
            );
            let context = contextualize(original, &[verb("伝えた", "v1")], 0);
            let mut indices: Vec<_> = context
                .ranking
                .senses
                .iter()
                .map(|sense| sense.original_index)
                .collect();
            indices.sort_unstable();
            assert_eq!(indices, (0..length).collect::<Vec<_>>());
        }
    }

    #[test]
    fn out_of_range_context_is_lossless_and_unranked() {
        let original = word(vec![sense("n", "first"), sense("v1", "second")]);
        let expected = original.clone();
        let context = contextualize(original, &[], 10);
        assert_eq!(context.word, expected);
        assert_eq!(context.ranking.favoured, None);
    }
}
