//! The dictionary entry for an expression the grammar layer already recognised.
//!
//! # The gap this closes
//!
//! A reader who selects `の` in 顔は可愛い**のに** is shown の's own entry — "indicates possessive",
//! "nominalizes verbs and adjectives" — while the dictionaries hold an entry for のに reading
//! "although, despite, even though" that nothing ever asks for. The construction *is* recognised:
//! `grammar::patterns` returns a `noni` match over exactly those two tokens. Recognition and
//! retrieval were simply never connected.
//!
//! This connects them, for two expressions and no others, and **adds to** the component answer
//! rather than replacing it. The reader keeps every existing way of reading の and に.
//!
//! # What is checked before anything is offered
//!
//! Finding the characters is not enough, and neither is overlapping a match:
//!
//! 1. the pattern id is one of [`OFFERED`] — a deliberate two-entry list, not "any pattern";
//! 2. the id **resolves** in the grammar pack, so a dangling reference offers nothing;
//! 3. the match covers the selected token;
//! 4. the matched tokens are **byte-contiguous** and concatenate to exactly the expression, and the
//!    match's own source span agrees with them — so a match whose span drifted from its tokens is
//!    refused rather than trusted;
//! 5. the shape is not the known coincidence, below.
//!
//! # The coincidence, and what excluding it costs
//!
//! `洗面器みたいなのに入れて` — "put it in one like a washbasin" — is not のに at all: の is a
//! nominaliser standing for a thing and に marks where it goes. `noni` fires on it anyway.
//!
//! Its token shape is `みたい` `な`〈だ〉 `の` `に`, which is **identical** to two genuine
//! concessives found in the same corpus: `はず` `な`〈だ〉 `の` `に` and `こと` `な`〈だ〉 `の` `に`.
//! No local structural test separates them; the difference is lexical, between a similarity noun
//! and a formal one.
//!
//! So the whole な+の+に shape is refused. **The cost is asymmetric and that is why it is the right
//! trade**: a refused offer leaves the reader exactly where they are today — の and に, both still
//! there — while a wrong offer would be a new confident error. Two of five concessives in the
//! development sample are given up to remove one false positive, and a blocklist of similarity
//! nouns was rejected as the kind of rule that is wrong wherever nobody checked it.
//!
//! # An occurrence inside a larger construction
//!
//! `AとかBとか` cannot satisfy precondition 4 and never will: the match spans the whole list, and
//! its tokens concatenate to 映画とか買い物とか, not to とか. Recognition is not the problem — the
//! enumerative rule fires — but there is nothing whole-span to offer.
//!
//! So an expression may also be offered for **one occurrence inside** a match, on the same terms
//! with one substitution: the occurrence is named by the **rule position that consumed it**
//! ([`crate::grammar::ElementSpan`]), never by scanning the matched run for the characters. That
//! distinction is the whole point. A scan would find the とか in a word that merely contains one;
//! a rule position is where the recogniser itself decided the endpoint was.
//!
//! Three things follow, and each is checked:
//!
//! - **The selection must be inside that element's own run.** Being somewhere in the construction
//!   is not enough, or the noun between the two endpoints would be offered とか — it is a listed
//!   item, and it has its own entry already.
//! - **The occurrence claims only its own span**, never the parent's. The construction's
//!   explanation is preserved and carried in [`ExpressionOffer::occurrence`], so the interface can
//!   say which list this とか belongs to without the offer pretending to be the list.
//! - **The span is verified from the tokens**, exactly as the whole-span path verifies its own:
//!   the consumed run must be grounded and byte-contiguous and must end with the expression, and
//!   the located ending must be exactly as long as the expression.
//!
//! The hearsay `～とか` needs no exclusion here, and that is worth saying plainly: the imported
//! pack gives `hanabira-n2-to-ka` no `match` rule at all, so it never produces a match and cannot
//! be swept in. The endpoints come from the enumerative rule's own positions or from nowhere.
//!
//! # What it does not claim
//!
//! [`ExpressionOffer::senses`] is every sense the entry records, in the dictionary's own order. Nothing here
//! ranks them and nothing here picks one: `のに` carries both the concessive and the "in order to"
//! reading, and which applies is exactly what the matcher does not know. The interface shows the
//! possibilities, as [`grammar::particles::Reading`] does for a particle, and claims no favourite.

use crate::dict::index::Dictionary;
use crate::dict::merge::Sense;
use crate::grammar::pack;
use crate::model::Token;

/// The expressions this offers, and the surface each one must spell.
///
/// **Deliberately two.** The pilot adjudicated のに and には against real contexts; every other
/// candidate — とか, よね, かな — is outside this slice and gets nothing until it has been through
/// the same reading.
const OFFERED: &[(&str, &str)] = &[("noni", "のに"), ("particle-niwa", "には")];

/// Expressions offered for an **occurrence inside** a recognised construction: the pattern, the
/// rule positions that hold an occurrence, and the expression each one must spell.
///
/// `hanabira-n4-a-b` is `[{surface_ends とか}, {0,6}, {surface_ends とか}]`, so positions 0 and 2
/// are its two endpoints and position 1 is the list item between them. Naming the positions is
/// what keeps the item out: it is not a place the rule put a とか.
const OFFERED_WITHIN: &[(&str, &[usize], &str)] = &[("hanabira-n4-a-b", &[0, 2], "とか")];

/// An expression entry offered beside the token the reader selected.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExpressionOffer {
    /// The expression as written, e.g. `のに`.
    pub expression: String,
    /// The grammar pattern that recognised it, so the interface can link the two.
    pub pattern_id: String,
    /// The pack entry's own title, already resolved — an unresolvable id never gets this far.
    pub title: String,
    /// Byte range in the analysed text.
    pub start: usize,
    pub end: usize,
    /// How the expression's span stands to the selected token's.
    ///
    /// Decided by comparing validated spans, never by comparing spelling: two tokens can spell
    /// the same characters and cover different source, and the label the reader sees is a claim
    /// about *this* selection's extent.
    ///
    /// **One field rather than a pair of booleans**, because the relationship has four states and
    /// two booleans can encode a fifth that does not exist. See [`SelectionExtent`].
    #[serde(default)]
    pub extent: SelectionExtent,
    /// Set when this offer is one **occurrence inside** a larger recognised construction, naming
    /// the construction's own extent. `None` for a whole-span offer.
    #[serde(default)]
    pub occurrence: Option<Occurrence>,
    /// Every sense the dictionary records, in its order. **No favourite**: see the module docs.
    pub senses: Vec<Sense>,
    /// Which dictionary answered.
    pub source: String,
}

/// How an expression's span stands to the span of the token the reader selected.
///
/// **Four states, because there are four.** Equality and containment in either direction do not
/// exhaust it: `surface_ends` deliberately allows an ending to begin inside a token, so an ending
/// can start inside one fused token that also holds text before it and finish in the next — and
/// then neither span contains the other. A pair of booleans reports that case as "false, false",
/// which is indistinguishable from "the expression contains the selection" and produces a label
/// that is simply untrue.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionExtent {
    /// The selection **is** the expression. An analyzer that joined のに into one token makes
    /// this the ordinary case; today both whole-span expressions are two tokens.
    Same,
    /// The expression covers more than the selection — `の` selected inside のに.
    ContainsSelection,
    /// The selection covers more than the expression — Ichiran fuses 映画とか into one token, so
    /// selecting it selects something the expression sits inside.
    WithinSelection,
    /// Neither contains the other: the ending begins inside a token that also holds text before
    /// it and finishes in a later one.
    Overlapping,
    /// Not established. The selection is not grounded, or it does not touch the expression at
    /// all — and there is nothing true to say about how a selection stands to text it does not
    /// touch.
    #[default]
    Unknown,
}

impl SelectionExtent {
    pub fn name(self) -> &'static str {
        match self {
            SelectionExtent::Same => "same",
            SelectionExtent::ContainsSelection => "contains_selection",
            SelectionExtent::WithinSelection => "within_selection",
            SelectionExtent::Overlapping => "overlapping",
            SelectionExtent::Unknown => "unknown",
        }
    }
}

/// Where an occurrence sits inside the construction that recognised it.
///
/// The offer's own `start`/`end` are the occurrence's; these are the parent's. Keeping them apart
/// is the point: the reader is being shown an entry for this とか, **inside** this list, and an
/// offer that claimed the whole list would be a different and wrong claim.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Occurrence {
    /// The rule position that consumed this occurrence.
    pub element: usize,
    /// The construction's own byte span, which the offer deliberately does not claim.
    pub start: usize,
    pub end: usize,
}

/// Whether this run of tokens is the known `な`+`の`+`に` coincidence.
///
/// Looks at the token *before* the match, which is where the ambiguity lives. Both the coincidence
/// and two genuine concessives have it, so this refuses all three — see the module docs for why
/// that trade is the right way round.
fn is_the_na_no_ni_shape(tokens: &[Token], first: usize) -> bool {
    first
        .checked_sub(1)
        .and_then(|before| tokens.get(before))
        .is_some_and(|token| token.surface == "な" && token.dict_form == "だ")
}

/// The matched tokens, if they are contiguous and spell `expression` exactly.
fn exact_span(
    tokens: &[Token],
    first: usize,
    end: usize,
    expression: &str,
) -> Option<(usize, usize)> {
    let (spelled, start, stop) = crate::grammar::occurrence::spelled_run(tokens, first, end)?;
    if spelled != expression {
        return None;
    }
    Some((start, stop))
}

/// How the selected token's span stands to the expression's.
///
/// **From the validated spans, never from the spelling.** Two tokens can read the same characters
/// over different source. An ungrounded token makes no claim, and neither does a selection that
/// does not touch the expression at all.
fn extent(tokens: &[Token], index: usize, start: usize, end: usize) -> SelectionExtent {
    let Some(token) = tokens.get(index) else {
        return SelectionExtent::Unknown;
    };
    let (Some(at), Some(to)) = (token.start, token.end) else {
        return SelectionExtent::Unknown;
    };
    if (at, to) == (start, end) {
        return SelectionExtent::Same;
    }
    if at <= start && end <= to {
        return SelectionExtent::WithinSelection;
    }
    if start <= at && to <= end {
        return SelectionExtent::ContainsSelection;
    }
    if at < end && start < to {
        return SelectionExtent::Overlapping;
    }
    SelectionExtent::Unknown
}

/// The expression entry for the token at `index`, when one is warranted.
///
/// `None` is the ordinary answer and means *no offer*, which is distinct from *a lookup failed* —
/// a dictionary error is reported by [`entry_for`] and surfaces as an error to the caller rather
/// than as an absence.
///
/// **Lazy by design.** Nothing calls this during passage analysis; it runs when a reader selects a
/// token, so a page of text costs no dictionary work it did not already do.
pub fn offered(
    tokens: &[Token],
    index: usize,
    matches: &[crate::grammar::Match],
    dictionaries: &[Dictionary],
) -> Result<Option<ExpressionOffer>, crate::dict::DictError> {
    for found in matches {
        let Some((_, expression)) = OFFERED
            .iter()
            .find(|(id, _)| *id == found.pattern_id.as_str())
        else {
            continue;
        };
        // The match must actually cover the selection, not merely sit in the same sentence.
        if index < found.first_token || index >= found.last_token_end {
            continue;
        }
        // A pattern id with no pack entry behind it is a dangling reference. Nothing is offered
        // for one, and the absence is not papered over with the raw id.
        let Some(entry) = pack::find(&found.pattern_id) else {
            continue;
        };
        let Some((start, end)) =
            exact_span(tokens, found.first_token, found.last_token_end, expression)
        else {
            continue;
        };
        // The match's own span has to agree with the tokens it named. Where it does not, something
        // upstream has drifted and the offer would be anchored to the wrong text.
        if found.start.is_some_and(|at| at != start) || found.end.is_some_and(|at| at != end) {
            continue;
        }
        if is_the_na_no_ni_shape(tokens, found.first_token) {
            continue;
        }

        let Some((senses, source)) = entry_for(expression, dictionaries)? else {
            continue;
        };
        return Ok(Some(ExpressionOffer {
            expression: (*expression).to_string(),
            pattern_id: found.pattern_id.clone(),
            title: entry.title.clone(),
            start,
            end,
            extent: extent(tokens, index, start, end),
            occurrence: None,
            senses,
            source,
        }));
    }

    // Nothing whole-span applied. An occurrence inside a construction is the other way an
    // expression can be reached, and it is tried second so the whole-span path keeps its
    // precedence exactly as before.
    for found in matches {
        if let Some(offer) = occurrence(tokens, index, found, dictionaries)? {
            return Ok(Some(offer));
        }
    }
    Ok(None)
}

/// The expression entry for an occurrence **inside** `found`, when the selection is in one.
///
/// Every check the whole-span path makes is made here, with "the matched tokens spell the
/// expression" replaced by "the element's consumed run ends with it, and the ending the matcher
/// located is exactly that long". The element's span is the authority for *where*; the tokens are
/// the authority for *what*, because a span nobody verified is a highlight over arbitrary text.
fn occurrence(
    tokens: &[Token],
    index: usize,
    found: &crate::grammar::Match,
    dictionaries: &[Dictionary],
) -> Result<Option<ExpressionOffer>, crate::dict::DictError> {
    let Some((_, positions, expression)) = OFFERED_WITHIN
        .iter()
        .find(|(id, _, _)| *id == found.pattern_id.as_str())
    else {
        return Ok(None);
    };
    // A dangling pattern id offers nothing here either, and the parent's own explanation is what
    // the offer will carry — so it has to exist.
    let Some(entry) = pack::find(&found.pattern_id) else {
        return Ok(None);
    };
    // The parent must know its own extent, since the offer reports it as the containing
    // construction. A match with no span cannot say what the occurrence is inside of.
    let (Some(parent_start), Some(parent_end)) = (found.start, found.end) else {
        return Ok(None);
    };

    let Some((position, start, end)) = occurrence_span(tokens, index, found, positions, expression)
    else {
        return Ok(None);
    };
    let Some((senses, source)) = entry_for(expression, dictionaries)? else {
        return Ok(None);
    };
    Ok(Some(ExpressionOffer {
        expression: (*expression).to_string(),
        pattern_id: found.pattern_id.clone(),
        title: entry.title.clone(),
        start,
        end,
        extent: extent(tokens, index, start, end),
        occurrence: Some(Occurrence {
            element: position,
            start: parent_start,
            end: parent_end,
        }),
        senses,
        source,
    }))
}

/// Which rule position holds the selection, and where its occurrence is written.
///
/// **The validation is shared** ([`crate::grammar::occurrence`]); what this adds is the one
/// question only an offer asks — whether the token the reader selected is inside *that* element's
/// run. Being somewhere in the construction is not enough, or the noun between two endpoints
/// would be offered the endpoints' entry.
///
/// Split out from the dictionary lookup so each precondition can be tested without a dictionary
/// on disk: an offer refused for the right reason and one refused because nothing was installed
/// look identical from outside, and only one of them is a guarantee.
fn occurrence_span(
    tokens: &[Token],
    index: usize,
    found: &crate::grammar::Match,
    positions: &[usize],
    expression: &str,
) -> Option<(usize, usize, usize)> {
    for &position in positions {
        let Some(valid) = crate::grammar::occurrence::validate(tokens, found, position, expression)
        else {
            continue;
        };
        if index < valid.first_token || index >= valid.last_token_end {
            continue;
        }
        return Some((valid.element, valid.start, valid.end));
    }
    None
}

/// Every sense the dictionaries record under this exact headword, with the source that gave them.
///
/// **Exact headword only.** A hit on a longer term that merely contains the expression is a
/// different word, and offering its senses under のに would be a worse answer than none.
fn entry_for(
    expression: &str,
    dictionaries: &[Dictionary],
) -> Result<Option<(Vec<Sense>, String)>, crate::dict::DictError> {
    for dictionary in dictionaries {
        // The error is propagated rather than folded into "no entry": a dictionary that could not
        // be read is a different situation from one that has nothing, and the interface has to be
        // able to tell them apart.
        let hits = dictionary.lookup(expression)?;
        let senses: Vec<Sense> = hits
            .iter()
            .filter(|hit| hit.entry.term == expression)
            .flat_map(|hit| {
                hit.entry.definitions.iter().map(|definition| Sense {
                    tags: definition.tags.clone(),
                    glosses: definition.glosses.clone(),
                    sequence: Some(hit.entry.sequence).filter(|sequence| *sequence != 0),
                })
            })
            .filter(|sense| !sense.glosses.is_empty())
            .collect();
        if !senses.is_empty() {
            return Ok(Some((senses, dictionary.meta().title.clone())));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::Match;

    fn token(surface: &str, dict_form: &str, start: usize) -> Token {
        Token {
            surface: surface.to_string(),
            dict_form: dict_form.to_string(),
            start: Some(start),
            end: Some(start + surface.len()),
            ..Token::default()
        }
    }

    fn noni_at(first: usize, end: usize, start: usize, stop: usize) -> Match {
        Match {
            pattern_id: "noni".into(),
            first_token: first,
            last_token_end: end,
            start: Some(start),
            end: Some(stop),
            elements: Vec::new(),
        }
    }

    /// 可愛い | の | に — the の and に are contiguous and spell のに.
    fn concessive() -> Vec<Token> {
        vec![
            token("可愛い", "可愛い", 0),
            token("の", "の", 9),
            token("に", "に", 12),
        ]
    }

    #[test]
    fn a_contiguous_run_spelling_the_expression_is_accepted() {
        let tokens = concessive();
        assert_eq!(exact_span(&tokens, 1, 3, "のに"), Some((9, 15)));
    }

    /// **A gap between the pieces is refused.** The offer is anchored to the span, and a span the
    /// tokens do not tile is one the highlight would draw over the wrong text.
    #[test]
    fn a_gap_between_the_matched_tokens_is_refused() {
        let mut tokens = concessive();
        tokens[2] = token("に", "に", 13);
        assert_eq!(exact_span(&tokens, 1, 3, "のに"), None);
    }

    /// A run that spells something else is refused even though the tokens are contiguous.
    #[test]
    fn a_run_that_spells_something_else_is_refused() {
        let tokens = vec![token("の", "の", 0), token("は", "は", 3)];
        assert_eq!(exact_span(&tokens, 0, 2, "のに"), None);
    }

    /// An ungrounded token cannot anchor an offer.
    #[test]
    fn an_ungrounded_token_is_refused() {
        let mut tokens = concessive();
        tokens[1].start = None;
        assert_eq!(exact_span(&tokens, 1, 3, "のに"), None);
    }

    /// **The coincidence, reproduced.** `みたい な の に` — 洗面器みたいなのに入れて — has the same
    /// shape as はずなのに and ことなのに, so all three are refused. See the module docs.
    #[test]
    fn the_na_no_ni_shape_is_recognised() {
        let coincidence = vec![
            token("みたい", "みたい", 0),
            token("な", "だ", 9),
            token("の", "の", 12),
            token("に", "に", 15),
        ];
        assert!(
            is_the_na_no_ni_shape(&coincidence, 2),
            "みたい な の に is the shape the offer must decline"
        );

        // And the same shape after a formal noun, which is a genuine concessive — refused too, and
        // the module docs record that this is the cost rather than an oversight.
        let genuine = vec![
            token("はず", "はず", 0),
            token("な", "だ", 6),
            token("の", "の", 9),
            token("に", "に", 12),
        ];
        assert!(is_the_na_no_ni_shape(&genuine, 2));

        // A verb before の is not the shape, and keeps its offer.
        assert!(!is_the_na_no_ni_shape(&concessive(), 1));
    }

    /// **The relationship label follows the span, not the spelling.**
    ///
    /// Selecting `の` inside `の`+`に` is *part of* the expression; a single token that happened to
    /// be the whole `のに` would *be* it. Both read the same characters, so only the validated span
    /// separates them — and the reader is told two different things.
    #[test]
    fn how_the_selection_stands_to_the_expression_comes_from_its_span() {
        // Two tokens spanning 9..15 between them: each is a part of the expression.
        let split = concessive();
        assert_eq!(extent(&split, 1, 9, 15), SelectionExtent::ContainsSelection);
        assert_eq!(extent(&split, 2, 9, 15), SelectionExtent::ContainsSelection);

        // One token over the same span *is* it. Same characters, different extent, different
        // label.
        let joined = vec![token("可愛い", "可愛い", 0), token("のに", "のに", 9)];
        assert_eq!(extent(&joined, 1, 9, 15), SelectionExtent::Same);

        // An ungrounded token claims no span, so nothing can be said about it.
        let mut ungrounded = joined.clone();
        ungrounded[1].start = None;
        assert_eq!(extent(&ungrounded, 1, 9, 15), SelectionExtent::Unknown);

        // And a token that spells the expression while covering different source is not it either.
        let elsewhere = vec![token("可愛い", "可愛い", 0), token("のに", "のに", 30)];
        assert_eq!(
            extent(&elsewhere, 1, 9, 15),
            SelectionExtent::Unknown,
            "spelling the expression is not covering it"
        );
    }

    /// A match that does not cover the selected token offers nothing for it.
    #[test]
    fn a_match_elsewhere_in_the_sentence_is_not_offered() {
        let tokens = concessive();
        let matches = vec![noni_at(1, 3, 9, 15)];
        // Token 0 is 可愛い, outside the match.
        assert_eq!(
            offered(&tokens, 0, &matches, &[]).expect("no dictionaries"),
            None
        );
    }

    /// 映画 と か 買い物 と か, laid out as contiguous source, and the match a real rule finds.
    fn enumerated() -> (Vec<Token>, Match) {
        let surfaces = ["映画", "と", "か", "買い物", "と", "か"];
        let mut at = 0;
        let tokens: Vec<Token> = surfaces
            .iter()
            .map(|surface| {
                let placed = token(surface, surface, at);
                at += surface.len();
                placed
            })
            .collect();
        // **Not hand-built.** The element spans are exactly what the matcher produces for the
        // imported enumerative rule, so a change to either ends up here.
        let found = crate::grammar::matcher::find(&tokens, pack::hanabira())
            .into_iter()
            .find(|found| found.pattern_id == "hanabira-n4-a-b")
            .expect("the enumerative rule fires on AとかBとか");
        (tokens, found)
    }

    /// Which rule position an occurrence offer would use, without needing a dictionary.
    fn placed_occurrence(
        tokens: &[Token],
        index: usize,
        found: &Match,
    ) -> Option<(usize, usize, usize)> {
        let (_, positions, expression) = OFFERED_WITHIN
            .iter()
            .find(|(id, _, _)| *id == found.pattern_id.as_str())?;
        occurrence_span(tokens, index, found, positions, expression)
    }

    /// **Both endpoints are reachable, each as its own occurrence.**
    #[test]
    fn each_toka_endpoint_is_its_own_occurrence() {
        let (tokens, found) = enumerated();
        assert_eq!((found.first_token, found.last_token_end), (1, 6));

        // と and か of the first とか both reach element 0, and it spans only that とか.
        for index in [1, 2] {
            assert_eq!(
                placed_occurrence(&tokens, index, &found),
                Some((0, 6, 12)),
                "token {index} belongs to the first endpoint"
            );
        }
        // …and both of the second reach element 2.
        for index in [4, 5] {
            assert_eq!(
                placed_occurrence(&tokens, index, &found),
                Some((2, 21, 27)),
                "token {index} belongs to the second endpoint"
            );
        }

        // Neither claims the construction's own span, which is what the parent explanation is
        // attached to.
        assert_eq!((found.start, found.end), (Some(6), Some(27)));
    }

    /// **The listed item between the endpoints is not an occurrence.**
    ///
    /// 買い物 is inside the construction and has its own entry already. Offering it とか would be
    /// the false positive this design exists to avoid — and a character scan over the matched run
    /// has no way to refuse it.
    #[test]
    fn the_item_between_the_endpoints_is_excluded() {
        let (tokens, found) = enumerated();
        assert!(
            found.first_token <= 3 && 3 < found.last_token_end,
            "買い物 is inside the match"
        );
        assert_eq!(placed_occurrence(&tokens, 3, &found), None);
    }

    /// A selection outside the construction gets nothing, as for the whole-span path.
    #[test]
    fn a_selection_outside_the_construction_is_not_an_occurrence() {
        let (tokens, found) = enumerated();
        assert_eq!(
            placed_occurrence(&tokens, 0, &found),
            None,
            "映画 is before the match"
        );
    }

    /// **A fused token is consumed whole and the occurrence is still only the とか.**
    #[test]
    fn a_fused_token_yields_the_ending_and_not_the_whole_word() {
        let surfaces = ["映画とか", "買い物", "とか"];
        let mut at = 0;
        let tokens: Vec<Token> = surfaces
            .iter()
            .map(|surface| {
                let placed = token(surface, surface, at);
                at += surface.len();
                placed
            })
            .collect();
        let found = crate::grammar::matcher::find(&tokens, pack::hanabira())
            .into_iter()
            .find(|found| found.pattern_id == "hanabira-n4-a-b")
            .expect("the enumerative rule fires on the fused shape");

        // The reader selected 映画とか. The offer is for the とか inside it, six bytes in.
        let (element, start, end) =
            placed_occurrence(&tokens, 0, &found).expect("the fused token holds an occurrence");
        assert_eq!((element, start, end), (0, 6, 12));
        assert_eq!(
            extent(&tokens, 0, start, end),
            SelectionExtent::WithinSelection,
            "the token is 映画とか: it holds the expression rather than being it, and that is the              opposite claim to の inside のに"
        );
    }

    /// **A match with no element spans offers nothing** rather than falling back to a scan.
    #[test]
    fn a_match_without_element_spans_offers_no_occurrence() {
        let (tokens, mut found) = enumerated();
        found.elements.clear();
        for index in 0..tokens.len() {
            assert_eq!(placed_occurrence(&tokens, index, &found), None);
        }
    }

    /// An element whose recorded span disagrees with its tokens is refused, exactly as a match
    /// whose span drifted from its tokens is.
    #[test]
    fn an_element_span_that_drifted_from_its_tokens_is_refused() {
        let (tokens, mut found) = enumerated();
        let first = found
            .elements
            .iter_mut()
            .find(|span| span.element == 0)
            .expect("element 0");
        first.start = Some(first.start.unwrap() + 1);
        assert_eq!(placed_occurrence(&tokens, 1, &found), None);
    }

    /// An ending outside the parent's own span is a contradiction, not an offer.
    #[test]
    fn an_occurrence_outside_its_parent_is_refused() {
        let (tokens, found) = enumerated();
        // Claim a parent that stops before the first endpoint ends. The parent's recorded
        // bounds must agree with its tokens, so shrinking them is itself refused — which is the
        // same guarantee, reached from the other side.
        let mut shrunk = found.clone();
        shrunk.end = Some(9);
        assert_eq!(placed_occurrence(&tokens, 1, &shrunk), None);
    }

    /// **The span says where; the tokens say what.**
    ///
    /// An element span that points at a run which does not end with the expression is refused,
    /// however well-formed the span is. Without this the offer would be anchored to source the
    /// reader can see does not spell とか.
    #[test]
    fn an_element_pointing_at_the_wrong_text_is_refused() {
        let (tokens, mut found) = enumerated();
        // Move element 0 onto 買い物 — **inside the construction**, internally consistent, and
        // exactly as long as とか. Every other check passes; only the spelling refuses it. Put
        // outside the parent instead, containment would refuse it first and this would prove
        // nothing about the spelling.
        let first = found
            .elements
            .iter_mut()
            .find(|span| span.element == 0)
            .expect("element 0");
        first.first_token = 3;
        first.last_token_end = 4;
        first.start = Some(12);
        first.end = Some(21);
        first.suffix_start = Some(15);
        first.suffix_end = Some(21);
        assert_eq!(tokens[3].surface, "買い物");
        assert!(
            found.start.unwrap() <= 15 && 21 <= found.end.unwrap(),
            "the fabricated occurrence is inside the parent, so containment cannot be what refuses it"
        );
        assert_eq!(placed_occurrence(&tokens, 3, &found), None);
    }

    /// **An occurrence must be at the end of the run it was found in.**
    ///
    /// The control for the check that was missing: move the first endpoint's ending onto the
    /// *second* とか, keeping everything else valid — six bytes long, inside the parent, an
    /// element whose consumed run really does end with とか. Every other test here passes it.
    /// Without the position check, selecting the first と is handed an offer anchored to text
    /// further down the sentence.
    #[test]
    fn an_ending_that_is_not_at_the_end_of_its_run_is_refused() {
        let (tokens, mut found) = enumerated();
        // What the real matcher produced, so the move is visibly a move.
        assert_eq!(placed_occurrence(&tokens, 1, &found), Some((0, 6, 12)));
        let second = found
            .element(2)
            .expect("element 2")
            .suffix()
            .expect("the second ending");
        assert_eq!(second, (21, 27));

        let first = found
            .elements
            .iter_mut()
            .find(|span| span.element == 0)
            .expect("element 0");
        first.suffix_start = Some(second.0);
        first.suffix_end = Some(second.1);

        // Still six bytes, still inside the parent, still an element whose run ends with とか.
        assert_eq!(second.1 - second.0, "とか".len());
        assert!(found.start.unwrap() <= second.0 && second.1 <= found.end.unwrap());
        assert_eq!(
            placed_occurrence(&tokens, 1, &found),
            None,
            "the first endpoint was anchored to the second one's text"
        );
    }

    /// An element claiming tokens its own match does not cover is refused.
    ///
    /// Everything else is left valid: the widened run still tiles its span, still ends with とか,
    /// and the ending is still at the end of it and inside the parent's bytes. Only the token
    /// range gives it away — the match begins at と, and this element reaches back past it.
    #[test]
    fn an_element_outside_its_match_is_refused() {
        let (tokens, mut found) = enumerated();
        assert_eq!(found.first_token, 1, "the match starts at と, not at 映画");
        let first = found
            .elements
            .iter_mut()
            .find(|span| span.element == 0)
            .expect("element 0");
        first.first_token = 0;
        first.start = Some(0);
        // The ending is unchanged and still correct for the widened run: 映画とか ends with とか.
        assert_eq!((first.last_token_end, first.end), (3, Some(12)));
        assert_eq!(first.suffix(), Some((6, 12)));
        assert_eq!(placed_occurrence(&tokens, 1, &found), None);
    }

    /// A parent whose recorded bounds disagree with its tokens cannot judge containment.
    ///
    /// The drift is at the far end, and the occurrence under test is at the near one — so the
    /// occurrence is still comfortably inside the parent's *claimed* bytes, and containment
    /// cannot be what refuses it. What refuses it is that the claim is not what the tokens say.
    #[test]
    fn a_parent_whose_bounds_drifted_offers_no_occurrence() {
        let (tokens, mut found) = enumerated();
        assert_eq!(placed_occurrence(&tokens, 1, &found), Some((0, 6, 12)));

        found.end = Some(found.end.unwrap() - 1);
        assert!(
            found.start.unwrap() <= 6 && 12 <= found.end.unwrap(),
            "the first endpoint is still inside the parent's claimed bytes"
        );
        assert_eq!(placed_occurrence(&tokens, 1, &found), None);
    }

    /// **The fourth relationship, which two booleans cannot say.**
    ///
    /// A synthetic tokenization where the ending begins inside a token that also holds text
    /// before it and finishes in the next: 映画と | か. The expression is とか; the selection is
    /// 映画と. Neither span contains the other, and the reader must not be told either does.
    #[test]
    fn an_ending_spanning_a_token_boundary_overlaps_the_selection() {
        let surfaces = ["映画と", "か"];
        let mut at = 0;
        let tokens: Vec<Token> = surfaces
            .iter()
            .map(|surface| {
                let placed = token(surface, surface, at);
                at += surface.len();
                placed
            })
            .collect();
        // 映画と is 0..9, か is 9..12, and とか is 6..12.
        assert_eq!((tokens[0].start, tokens[0].end), (Some(0), Some(9)));
        assert_eq!(
            extent(&tokens, 0, 6, 12),
            SelectionExtent::Overlapping,
            "neither span contains the other"
        );
        // The second token *is* inside the ending, and says so.
        assert_eq!(
            extent(&tokens, 1, 6, 12),
            SelectionExtent::ContainsSelection
        );
    }

    /// Every state of the relationship, from spans alone.
    #[test]
    fn the_four_relationships_are_distinguished() {
        let tokens = concessive();
        // 可愛い 0..9, の 9..12, に 12..15.
        assert_eq!(extent(&tokens, 1, 9, 12), SelectionExtent::Same);
        assert_eq!(
            extent(&tokens, 1, 9, 15),
            SelectionExtent::ContainsSelection
        );
        assert_eq!(extent(&tokens, 0, 3, 6), SelectionExtent::WithinSelection);
        assert_eq!(extent(&tokens, 0, 6, 12), SelectionExtent::Overlapping);
        // Not touching the selection at all, and an ungrounded selection: nothing to say.
        assert_eq!(extent(&tokens, 0, 12, 15), SelectionExtent::Unknown);
        let mut floating = tokens.clone();
        floating[1].start = None;
        assert_eq!(extent(&floating, 1, 9, 12), SelectionExtent::Unknown);
        assert_eq!(extent(&tokens, 99, 9, 12), SelectionExtent::Unknown);
    }

    /// **A run whose tokens tile a longer span than they spell is refused.**
    ///
    /// Contiguity is not enough. Here と claims four bytes for a three-byte surface and か picks
    /// up where it left off, so the run tiles 6..13 while spelling six bytes — and every span in
    /// sight is internally consistent with every other. An offset into text like that means
    /// nothing, so the run's source length is compared with what it spells.
    #[test]
    fn a_run_that_tiles_more_source_than_it_spells_is_refused() {
        let (mut tokens, mut found) = enumerated();
        assert_eq!(placed_occurrence(&tokens, 1, &found), Some((0, 6, 12)));

        tokens[1].end = Some(10);
        tokens[2].start = Some(10);
        tokens[2].end = Some(13);
        let first = found
            .elements
            .iter_mut()
            .find(|span| span.element == 0)
            .expect("element 0");
        // Everything else brought into agreement with the fabricated layout.
        first.end = Some(13);
        first.suffix_start = Some(7);
        first.suffix_end = Some(13);
        found.end = Some(found.end.unwrap().max(13));

        assert_eq!(
            placed_occurrence(&tokens, 1, &found),
            None,
            "the run spells six bytes and covers seven; nothing here can be anchored"
        );
    }

    /// **のに and には are unchanged**: they are whole-span offers and gain no occurrence.
    #[test]
    fn the_existing_expressions_are_not_occurrences() {
        for (id, _) in OFFERED {
            assert!(
                !OFFERED_WITHIN.iter().any(|(within, _, _)| within == id),
                "{id} is offered whole; adding it here would change what it claims"
            );
        }
        let tokens = concessive();
        let found = noni_at(1, 3, 9, 15);
        assert_eq!(placed_occurrence(&tokens, 1, &found), None);
    }

    /// **An unknown pattern is never offered**, however well it matches.
    #[test]
    fn only_the_listed_patterns_are_offered() {
        let tokens = concessive();
        let matches = vec![Match {
            pattern_id: "particle-no".into(),
            first_token: 1,
            last_token_end: 3,
            start: Some(9),
            end: Some(15),
            elements: Vec::new(),
        }];
        assert_eq!(
            offered(&tokens, 1, &matches, &[]).expect("no dictionaries"),
            None
        );
    }

    /// A match whose recorded span disagrees with the tokens it named is refused.
    #[test]
    fn a_match_whose_span_drifted_from_its_tokens_is_refused() {
        let tokens = concessive();
        let matches = vec![noni_at(1, 3, 0, 15)];
        assert_eq!(
            offered(&tokens, 1, &matches, &[]).expect("no dictionaries"),
            None
        );
    }

    /// With no dictionaries there is no entry, and that is an absence rather than an error.
    #[test]
    fn no_dictionaries_means_no_offer_not_a_failure() {
        let tokens = concessive();
        let matches = vec![noni_at(1, 3, 9, 15)];
        // `DictError` is not `PartialEq`, so the two outcomes are distinguished by shape — which
        // is the distinction that matters here anyway: an absence is not a failure.
        match offered(&tokens, 1, &matches, &[]) {
            Ok(None) => {}
            Ok(Some(offer)) => panic!("nothing can be offered without a dictionary: {offer:?}"),
            Err(error) => panic!("an empty dictionary list is not an error: {error:?}"),
        }
    }
}
