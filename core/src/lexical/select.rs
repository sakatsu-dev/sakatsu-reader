//! Deciding which proposed unit, if any, replaces the baseline over a region.
//!
//! # What replaces "the longest span the dictionary can answer for"
//!
//! `project notes` §4a stated that rule, and it is the one this module exists to replace. Length is
//! not evidence. A dictionary large enough to answer for almost any run of kana will answer for
//! plenty of runs that are not words, and choosing the longest of them is choosing the one most
//! likely to be wrong. What decides a unit here is **what supports it**, and where the support
//! does not settle the question the baseline stands.
//!
//! # Two routes, because one of them would exclude the case that motivated this
//!
//! - **Lexical** — a dictionary entry for the whole unit, with reading and grammatical evidence
//!   that independently support it.
//! - **Construction** — a recognised construction whose validated occurrence span covers the unit
//!   exactly. `とか` is a particle construction; no lexical route reaches it, and a layer that
//!   only had the first would deliver assistance for it and never the grouping.
//!
//! A construction-backed unit takes its extent from the occurrence the matcher already validated
//! ([`crate::grammar::ElementSpan`]), never from searching the region again for the characters it
//! expects to find. Recomputing a location that has already been decided is how two layers come
//! to disagree about the same text.
//!
//! # Abstention is an outcome, not a failure
//!
//! Where the requirements do not establish a choice, the baseline stands and the validated
//! alternatives are offered as assistance. That is the honest answer to a genuinely open
//! question, and it is what the reader asked for: ambiguous cases left alone.
//!
//! # What a conflict is, stated so the baseline cannot veto every improvement
//!
//! Only interpretations that **independently satisfy a route** are compared. The baseline's
//! existence is not a conflict: every grouping differs from the baseline in extent, so a rule
//! that treated a differing span as a conflict would let `かっこ | いい` veto `かっこいい` for
//! ever, and no grouping could happen at all. The baseline is excluded by the ordinary
//! requirements rather than by a special case — it carries no dictionary evidence for the unit,
//! so it never satisfies a route.
//!
//! Before comparing, demonstrated spelling variants and resolved redirects are canonicalized:
//! 格好いい and 格好良い under one sequence are **one** interpretation, not two competing ones.
//! What forces abstention is a materially different, independently supported reading of the same
//! text — or the same word claimed over two different extents, which is a disagreement about
//! where the word is.

use std::collections::BTreeMap;

use crate::grammar::Match;
use crate::model::Token;

use super::candidate::{Candidate, ClassAgreement, Derivation, Eligibility, EntryEvidence, Origin};
use super::generate::Generated;

/// How a unit earned its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// A dictionary entry for the whole unit, with independent grammatical evidence.
    Lexical,
    /// A validated occurrence of a recognised construction.
    Construction,
}

impl Route {
    pub fn name(self) -> &'static str {
        match self {
            Route::Lexical => "lexical",
            Route::Construction => "construction",
        }
    }
}

/// What was decided about one region of the passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub start: usize,
    pub end: usize,
    pub outcome: Outcome,
}

/// The decision itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// This candidate replaces the baseline over the region.
    Grouped {
        /// Index into [`Generated::candidates`].
        candidate: usize,
        route: Route,
    },
    /// The baseline stands. **Not a failure** — the alternatives are real and are offered.
    Abstained {
        reason: &'static str,
        /// Indices into [`Generated::candidates`], in span then index order.
        alternatives: Vec<usize>,
    },
}

impl Outcome {
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Grouped { .. } => "grouped",
            Outcome::Abstained { .. } => "abstained",
        }
    }
}

/// The shapes a lexical grouping may take, to start.
///
/// **A whitelist, and deliberately a short one.** A blocklist of things not to swallow — a
/// connector, an auxiliary chain, punctuation — is wrong wherever nobody thought of the case;
/// naming what *is* supported is wrong only where it is too conservative, and too conservative
/// means the baseline stands, which is where the reader already is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Structure {
    /// A non-inflecting word and an i-adjective head: かっこ + いい, かっこ + よかった.
    NounAdjective,
    /// A non-inflecting word and a suffix: 子供 + 向け.
    NounSuffix,
}

impl Structure {
    pub fn name(self) -> &'static str {
        match self {
            Structure::NounAdjective => "noun+adjective",
            Structure::NounSuffix => "noun+suffix",
        }
    }
}

/// Whether a token inflects — which is what separates a word that can head an adjective compound
/// from the noun in front of it.
fn inflects(token: &Token) -> bool {
    token.conjugation.is_some()
}

/// The normalized class the analyzer established for a token, where it established one.
fn class_of(token: &Token) -> Option<&str> {
    super::inflect::TokenMorphology::of_token(token).class
}

/// Which supported shape a run has, if any.
///
/// Two tokens to start. Longer compounds are a real thing and are not refused because they cannot
/// exist — they are refused because nothing here has yet shown which of them are safe to join
/// automatically, and the cost of guessing is a wrong unit in the reader's text.
pub fn structure(run: &[Token]) -> Option<Structure> {
    let [first, head] = run else {
        return None;
    };
    // **The left constituent must be established, not merely un-refuted.** UniDic says 名詞 or
    // it does not, and anything else — an adverb, a connector, a word it could not classify —
    // is left where the analyzer put it. Reading silence as a noun was the bug: it made every
    // unclassifiable token a candidate for joining.
    if first.lexical_role != Some(crate::model::LexicalRole::Noun) || inflects(first) {
        return None;
    }
    if head.lexical_role == Some(crate::model::LexicalRole::Suffix) {
        return Some(Structure::NounSuffix);
    }
    // **An i-adjective head, established by the analyzer.** Both acceptance shapes have one:
    // Lindera classifies the bare `いい` of かっこいい as `adj-i` just as it does the inflected
    // よかった of かっこよかった, so the requirement costs the motivating case nothing.
    //
    // A head the analyzer did not classify is refused, and that refusal is doing real work. と
    // and か are classified as nothing, so an enumerative とか cannot be grouped by this route —
    // which is correct, because `AとかBとか` is a construction and *which* とか it is belongs to
    // the matcher. A rule that accepted "two words the analyzer says nothing about" would group
    // every と + か on the page, including the hearsay one.
    let class = class_of(head)?;
    (crate::lexical::classes_agree(class, super::inflect::TRANSFORMATION_CLASS)
        == super::candidate::ClassAgreement::Compatible)
        .then_some(Structure::NounAdjective)
}

/// Why a candidate cannot be chosen automatically, or that it can.
///
/// **Per candidate, and route by route.** Each interpretation is assessed on its own evidence;
/// none of them inherits another's support, and a span with three interpretations has three
/// answers rather than one.
pub fn assess(
    candidate: &Candidate,
    tokens: &[Token],
    found: &Generated,
    occurrences: &[Occurrence],
) -> Eligibility {
    if candidate.joins() < 2 {
        // Not a grouping decision at all. The baseline already covers this span, so there is
        // nothing to replace and nothing to weigh.
        return Eligibility::Rejected("covers a single token, so it is not a grouping");
    }
    // **An incomplete search is not a complete answer.** Both of these are about the region, not
    // the candidate, and both mean the same thing: something that might have mattered was not
    // looked at.
    if found.lookup_failed_over(candidate.start, candidate.end) {
        return Eligibility::Rejected("a dictionary failed over this region");
    }
    if found.incomplete_over(candidate.start, candidate.end) {
        return Eligibility::Rejected(
            "an admissible interpretation over this region was unexamined",
        );
    }

    match candidate.origin {
        Origin::Construction => construction_route(candidate, occurrences),
        _ => lexical_route(candidate, tokens),
    }
}

/// A validated construction occurrence, as §3's matcher established it.
///
/// Carried in rather than recomputed: the matcher decided where the occurrence is, and a second
/// search of the same text for the same characters is how two layers come to disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    pub start: usize,
    pub end: usize,
    /// The token range the occurrence's element consumed.
    pub first_token: usize,
    pub last_token_end: usize,
}

/// The constructions whose occurrences grouping may use, and the rule positions that hold one.
///
/// **The same shape as the expression offer's list, and deliberately as short.** A construction
/// reaches this list by having been read against real contexts, not by having a rule that fires.
/// `hanabira-n4-a-b` is `[{surface_ends とか}, {0,6}, {surface_ends とか}]`, so positions 0 and 2
/// are its endpoints and position 1 is the listed item between them.
pub const GROUPED_CONSTRUCTIONS: &[(&str, &[usize], &str)] =
    &[("hanabira-n4-a-b", &[0, 2], "とか")];

/// Every validated occurrence a set of matches carries, for the patterns grouping may use.
///
/// **The same validator the expression offer uses** ([`crate::grammar::occurrence`]), so the two
/// cannot disagree about where an occurrence is or whether it is one. This reads the result; it
/// derives nothing.
pub fn occurrences(
    tokens: &[Token],
    matches: &[Match],
    offered: &[(&str, &[usize], &str)],
) -> Vec<Occurrence> {
    let mut found = Vec::new();
    for hit in matches {
        let Some((_, positions, expression)) = offered
            .iter()
            .find(|(id, _, _)| *id == hit.pattern_id.as_str())
        else {
            continue;
        };
        for valid in crate::grammar::occurrence::all(tokens, hit, positions, expression) {
            found.push(Occurrence {
                start: valid.start,
                end: valid.end,
                first_token: valid.first_token,
                last_token_end: valid.last_token_end,
            });
        }
    }
    found
}

fn lexical_route(candidate: &Candidate, tokens: &[Token]) -> Eligibility {
    // A complete dictionary entry for the unit, and nothing that only looks like one: a name is
    // still a name, and an answer about a component is still about the component.
    if !candidate.supported_as_lexical_unit() {
        return Eligibility::Rejected("no record supports treating this span as a word");
    }
    let Some(lemma_reading) = candidate.lemma_reading.established() else {
        return Eligibility::Rejected("the unit's reading was not established");
    };
    // **Grammatical evidence, independently.** For a recovered form that means the class came
    // from somewhere other than a matching reading; a reading match establishes a possible word
    // and never what kind of word it is.
    if let Derivation::Inflection { ground, .. } = &candidate.derivation {
        if !ground.is_grounded() {
            return Eligibility::Rejected("nothing but a matching reading supports the unit");
        }
    }
    let Some(run) = candidate
        .tokens
        .first()
        .zip(candidate.tokens.last())
        .and_then(|(&from, &to)| tokens.get(from..=to))
    else {
        return Eligibility::Rejected("the unit's tokens are not a run of the analysis");
    };
    if run.len() != candidate.tokens.len() {
        return Eligibility::Rejected("the unit's tokens are not contiguous");
    }
    let Some(shape) = structure(run) else {
        return Eligibility::Rejected("the structure is not one grouping supports");
    };

    // **What the whole entry says about the whole unit**, which the token structure does not.
    //
    // Without this a candidate passed because its *tail* was adjectival while the entry it rested
    // on described a different word: a different reading, or a different class. The structure
    // says these tokens could be a compound of this shape; the record has to say that this
    // compound is that word.
    let support: Vec<&EntryEvidence> = candidate
        .evidence
        .iter()
        .filter(|record| record.supports_lexical_unit())
        .collect();

    // The reading proposed for the unit has to be a reading some supporting record records. An
    // established lemma reading only means one was written down.
    let proposed = crate::kana::to_hiragana(lemma_reading);
    if !support
        .iter()
        .any(|record| crate::kana::to_hiragana(&record.reading) == proposed)
    {
        return Eligibility::Rejected("no record reads the way the unit is proposed to read");
    }

    // **And the analyzer's own reading of the span has to agree**, which is a different question.
    //
    // The lemma reading was *copied from* a record in generation, so comparing it with the
    // records compares dictionary evidence with itself and can only fail if the evidence is
    // internally inconsistent. The independent evidence is what the analyzer read over this
    // source. 上手 read じょうず by the analyzer is not the 上手 read うわて a whole-query hit may
    // return, and only this comparison notices.
    let Some(observed) = candidate.surface_reading.established() else {
        return Eligibility::Rejected("the analyzer established no reading for the span");
    };
    if matches!(candidate.derivation, Derivation::AsWritten) {
        // Nothing was transformed, so the analyzer's reading and the record's are the same claim
        // about the same characters and must match.
        let observed = crate::kana::to_hiragana(observed);
        if !support
            .iter()
            .any(|record| crate::kana::to_hiragana(&record.reading) == observed)
        {
            return Eligibility::Rejected("no record reads the way the analyzer read the span");
        }
    }
    // For a recovered unit the two readings *should* differ — かっこよかった is not かっこいい —
    // and what connects them is the transformation, which `inflect` validated against a record
    // and which is recorded in the derivation. Requiring them to match here would reject every
    // inflected form.

    // The class the structure claims for the unit. `noun+adjective` claims an i-adjective;
    // `noun+suffix` claims nothing in particular, so the requirement there is only that the
    // record commits to *some* class rather than leaving the unit unclassified.
    match shape {
        Structure::NounAdjective => {
            let agreements: Vec<ClassAgreement> = support
                .iter()
                .map(|record| record.asserted_class(super::inflect::TRANSFORMATION_CLASS))
                .collect();
            if agreements.contains(&ClassAgreement::Compatible) {
                return Eligibility::Eligible;
            }
            if agreements.contains(&ClassAgreement::Incompatible) {
                return Eligibility::Rejected(
                    "the record's class contradicts the unit's structure",
                );
            }
            // **Absent is not agreement**, and this is the plan's rule for a record that asserts
            // nothing: the candidate is kept as an alternative rather than chosen.
            Eligibility::Rejected("no record asserts a class for the unit")
        }
        Structure::NounSuffix => {
            if support.iter().any(|record| record.asserts_any_class()) {
                Eligibility::Eligible
            } else {
                Eligibility::Rejected("no record asserts a class for the unit")
            }
        }
    }
}

fn construction_route(candidate: &Candidate, occurrences: &[Occurrence]) -> Eligibility {
    // **Exactly**, in both dimensions. A unit that merely sits inside an occurrence is not the
    // occurrence, and one that spills past it is claiming text the matcher did not validate.
    let exact = occurrences.iter().any(|occurrence| {
        (occurrence.start, occurrence.end) == (candidate.start, candidate.end)
            && candidate.tokens.first() == Some(&occurrence.first_token)
            && candidate.tokens.last() == Some(&(occurrence.last_token_end - 1))
    });
    if !exact {
        return Eligibility::Rejected("no validated occurrence covers this unit exactly");
    }
    if !candidate.supported_as_lexical_unit() {
        return Eligibility::Rejected("the occurrence has no dictionary assistance behind it");
    }
    Eligibility::Eligible
}

/// The identity of an **interpretation**, canonicalized: which installed dictionary, which
/// sequence (or locator, for a record filed under none), the reading proposed for the unit, and
/// the class evidence the record offers.
///
/// **Reading and class belong in it.** Canonicalizing on sequence alone collapsed 上手 read
/// じょうず with 上手 read うわて, and collapsed two records that share a sequence while asserting
/// different classes — undoing in the selector exactly the partitioning `interpretation_key`
/// performs in generation. Spelling variants and resolved redirects still canonicalize together,
/// because those share a sequence *and* a reading *and* a class; what does not collapse is two
/// different analyses.
type WordKey = (String, Option<i64>, String, String, Vec<String>);

/// One claim over one region: an interpretation, and the extent it is claimed over. **Both**,
/// because the same word proposed over two stretches of text is a disagreement about where the
/// word is.
type Claim = (
    String,
    Option<i64>,
    String,
    String,
    Vec<String>,
    usize,
    usize,
);

/// What makes two interpretations the same one.
///
/// **Canonicalized before comparing.** 格好いい and 格好良い are filed under one sequence, and a
/// redirect and its target are one word — so they are one interpretation, and treating them as
/// two competitors would abstain on a case where nothing actually disagrees.
fn canonical(candidate: &Candidate) -> Option<WordKey> {
    candidate
        .evidence
        .iter()
        .find(|record| record.supports_lexical_unit())
        .map(|record| {
            let (dictionary, sequence, locator) = record.id.same_word_key();
            (
                dictionary.installed_as,
                sequence,
                locator,
                crate::kana::to_hiragana(
                    candidate
                        .lemma_reading
                        .established()
                        .unwrap_or(&candidate.surface),
                ),
                record.class_evidence(),
            )
        })
}

/// Decide every region, and say why where nothing was decided.
///
/// `occurrences` are the validated construction occurrences the construction route may use, and
/// `assessed` is filled with each candidate's eligibility so a caller can report the whole
/// picture rather than only what was chosen.
pub fn decide(
    tokens: &[Token],
    found: &Generated,
    occurrences: &[Occurrence],
) -> (Vec<Decision>, Vec<Eligibility>) {
    let assessed: Vec<Eligibility> = found
        .candidates
        .iter()
        .map(|candidate| assess(candidate, tokens, found, occurrences))
        .collect();

    let eligible: Vec<usize> = (0..found.candidates.len())
        .filter(|&index| assessed[index].is_eligible())
        .collect();

    // Regions are maximal runs of overlapping eligible candidates: a decision about one of them
    // is a decision about all of them, because they are claims about the same text.
    let mut regions: Vec<Vec<usize>> = Vec::new();
    for &index in &eligible {
        let candidate = &found.candidates[index];
        let joined: Vec<usize> = regions
            .iter()
            .enumerate()
            .filter(|(_, region)| {
                region.iter().any(|&other| {
                    let against = &found.candidates[other];
                    against.start < candidate.end && candidate.start < against.end
                })
            })
            .map(|(at, _)| at)
            .collect();
        match joined.split_first() {
            None => regions.push(vec![index]),
            Some((&first, rest)) => {
                regions[first].push(index);
                // Merge any other regions this candidate bridges, highest first so the indices
                // stay valid as they are removed.
                for &other in rest.iter().rev() {
                    let merged = regions.remove(other);
                    regions[first].extend(merged);
                }
            }
        }
    }

    let mut decisions: Vec<Decision> = Vec::new();
    for mut region in regions {
        region.sort_unstable();
        let start = region
            .iter()
            .map(|&index| found.candidates[index].start)
            .min()
            .unwrap_or_default();
        let end = region
            .iter()
            .map(|&index| found.candidates[index].end)
            .max()
            .unwrap_or_default();

        // One interpretation, over one extent, or nothing is settled.
        let mut claims: BTreeMap<Claim, Vec<usize>> = BTreeMap::new();
        let mut unidentified = false;
        for &index in &region {
            let candidate = &found.candidates[index];
            match canonical(candidate) {
                Some((dictionary, sequence, locator, reading, class)) => claims
                    .entry((
                        dictionary,
                        sequence,
                        locator,
                        reading,
                        class,
                        candidate.start,
                        candidate.end,
                    ))
                    .or_default()
                    .push(index),
                None => unidentified = true,
            }
        }

        let outcome = if unidentified || claims.len() != 1 {
            Decision {
                start,
                end,
                outcome: Outcome::Abstained {
                    reason: "two supported interpretations disagree over this region",
                    alternatives: region.clone(),
                },
            }
        } else {
            let (_, indices) = claims.into_iter().next().expect("exactly one claim");
            let chosen = indices[0];
            let route = match found.candidates[chosen].origin {
                Origin::Construction => Route::Construction,
                _ => Route::Lexical,
            };
            Decision {
                start,
                end,
                outcome: Outcome::Grouped {
                    candidate: chosen,
                    route,
                },
            }
        };
        decisions.push(outcome);
    }

    decisions.sort_by_key(|decision| (decision.start, decision.end));
    (decisions, assessed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::index::MatchKind;
    use crate::dict::merge::Retrieved;
    use crate::lexical::candidate::{
        AnswerKind, DictionaryId, EntryEvidence, EntryId, InflectionGround, Reading,
    };
    use crate::lexical::generate::{Failure, Unsearched};
    use crate::model::{Conjugation, LexicalRole, Prop};

    /// A token UniDic classified as 名詞 — the only thing that may stand left of a head.
    fn noun(surface: &str, at: usize) -> Token {
        Token {
            lexical_role: Some(LexicalRole::Noun),
            ..token(surface, at)
        }
    }

    /// A token written in kanji, so its reading is not its spelling. **The ordinary case**, and
    /// the one where "what the analyzer read" and "what the record says" are separable claims.
    fn read(surface: &str, reading: &str, at: usize) -> Token {
        Token {
            reading: reading.into(),
            lemma_reading: reading.into(),
            ..token(surface, at)
        }
    }

    /// A token the analyzer classified as nothing: an adverb, a connector, or a word UniDic
    /// could not place. **The model does not distinguish them**, and the selector does not need
    /// it to — none of them is established as a noun, and that is the whole requirement.
    fn token(surface: &str, at: usize) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            reading: surface.into(),
            lemma_reading: surface.into(),
            start: Some(at),
            end: Some(at + surface.len()),
            ..Default::default()
        }
    }

    /// An adjective as Lindera actually reports the bare form: classified, not inflected away.
    fn adjective(surface: &str, at: usize) -> Token {
        classified(surface, at, "adj-i", "Non-past")
    }

    /// A token the analyzer gave a conjugation, and therefore a class.
    fn inflected(surface: &str, at: usize, class: &str) -> Token {
        classified(surface, at, class, "Past (~ta)")
    }

    fn classified(surface: &str, at: usize, class: &str, form: &str) -> Token {
        Token {
            conjugation: Some(Conjugation {
                lemma: surface.into(),
                reading: surface.into(),
                props: vec![Prop {
                    pos: class.into(),
                    form: form.into(),
                    is_base: true,
                    ..Default::default()
                }],
            }),
            ..token(surface, at)
        }
    }

    fn laid_out(surfaces: &[&str]) -> Vec<Token> {
        let mut at = 0;
        surfaces
            .iter()
            .map(|surface| {
                let placed = token(surface, at);
                at += surface.len();
                placed
            })
            .collect()
    }

    /// A record whose senses say `adjective`, which is what Jitendex actually writes for the
    /// acceptance case — its `rules` are empty and its definition tags are register markers.
    fn record(term: &str, reading: &str, sequence: i64, dictionary: &str) -> EntryEvidence {
        classed(term, reading, sequence, dictionary, &["adj-i"])
    }

    fn classed(
        term: &str,
        reading: &str,
        sequence: i64,
        dictionary: &str,
        classes: &[&str],
    ) -> EntryEvidence {
        EntryEvidence {
            id: EntryId {
                dictionary: DictionaryId {
                    title: "Fixture".into(),
                    installed_as: dictionary.into(),
                    generation: "g1".into(),
                },
                sequence: Some(sequence),
                locator: format!("{term}/{sequence}"),
                same_word_as: None,
            },
            term: term.into(),
            reading: reading.into(),
            kind: AnswerKind::OrdinaryDefinition,
            matched: MatchKind::Spelling,
            retrieved: Retrieved::Whole { query: term.into() },
            rules: Vec::new(),
            tags: Vec::new(),
            sense_tags: classes.iter().map(|class| (*class).to_string()).collect(),
        }
    }

    /// A grouping candidate over `tokens[from..=to]`, supported by `evidence`.
    fn grouping(
        tokens: &[Token],
        from: usize,
        to: usize,
        evidence: Vec<EntryEvidence>,
    ) -> Candidate {
        let surface: String = tokens[from..=to]
            .iter()
            .map(|token| token.surface.as_str())
            .collect();
        Candidate {
            start: tokens[from].start.unwrap(),
            end: tokens[to].end.unwrap(),
            tokens: (from..=to).collect(),
            dict_form: evidence
                .first()
                .map(|record| record.term.clone())
                .unwrap_or_else(|| surface.clone()),
            // From the tokens, as `run_reading` does in production — never from the spelling,
            // or the surface-reading check would be comparing a record with itself again.
            surface_reading: Reading::Established(
                tokens[from..=to]
                    .iter()
                    .map(|token| token.reading.as_str())
                    .collect(),
            ),
            lemma_reading: Reading::Established(
                evidence
                    .first()
                    .map(|record| record.reading.clone())
                    .unwrap_or_else(|| surface.clone()),
            ),
            surface,
            origin: Origin::LexicalLookup,
            derivation: Derivation::AsWritten,
            evidence,
            eligibility: Eligibility::Unassessed,
        }
    }

    fn generated(candidates: Vec<Candidate>) -> Generated {
        Generated {
            candidates,
            ..Default::default()
        }
    }

    /// かっこ + いい, the acceptance shape: a plain noun and an uninflecting kana adjective.
    fn acceptance() -> (Vec<Token>, Generated) {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
        )]);
        (tokens, found)
    }

    fn only_decision(decisions: &[Decision]) -> &Decision {
        assert_eq!(decisions.len(), 1, "expected one region: {decisions:?}");
        &decisions[0]
    }

    /// **The baseline's existence is not a conflict.**
    ///
    /// Every grouping differs from the baseline in extent. A rule that read a differing span as a
    /// disagreement would let かっこ | いい veto かっこいい for ever — and no automatic grouping
    /// could happen at all, which is the failure mode this definition exists to avoid.
    #[test]
    fn a_supported_grouping_replaces_the_baseline() {
        let (tokens, mut found) = acceptance();
        // The baseline, exactly as generation emits it: one candidate per token, no evidence.
        for index in 0..tokens.len() {
            let mut baseline = grouping(&tokens, index, index, Vec::new());
            baseline.origin = Origin::Baseline;
            found.candidates.push(baseline);
        }

        let (decisions, _) = decide(&tokens, &found, &[]);
        let decision = only_decision(&decisions);
        assert_eq!(
            decision.outcome,
            Outcome::Grouped {
                candidate: 0,
                route: Route::Lexical
            }
        );
        assert_eq!((decision.start, decision.end), (0, 15));
    }

    /// The baseline is excluded by the ordinary requirements, not by a special case.
    #[test]
    fn the_baseline_is_refused_for_carrying_no_evidence_of_a_unit() {
        let (tokens, _) = acceptance();
        let mut baseline = grouping(&tokens, 0, 0, Vec::new());
        baseline.origin = Origin::Baseline;
        let found = generated(vec![baseline]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("covers a single token, so it is not a grouping")
        );
    }

    /// **Two genuinely supported interpretations force abstention.**
    #[test]
    fn two_supported_interpretations_over_one_region_abstain() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let found = generated(vec![
            grouping(
                &tokens,
                0,
                1,
                vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
            ),
            // A different word entirely, over the same text.
            grouping(
                &tokens,
                0,
                1,
                vec![record("恰好好い", "かっこいい", 9999, "jitendex")],
            ),
        ]);
        let (decisions, _) = decide(&tokens, &found, &[]);
        let Outcome::Abstained {
            reason,
            alternatives,
        } = &only_decision(&decisions).outcome
        else {
            panic!("expected abstention: {decisions:?}");
        };
        assert!(reason.contains("disagree"));
        assert_eq!(
            alternatives,
            &vec![0, 1],
            "both are offered, neither is chosen"
        );
    }

    /// **Spelling variants of one word are one interpretation**, not two competitors.
    #[test]
    fn variants_under_one_sequence_do_not_force_abstention() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let found = generated(vec![
            grouping(
                &tokens,
                0,
                1,
                vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
            ),
            grouping(
                &tokens,
                0,
                1,
                vec![record("格好良い", "かっこいい", 1577490, "jitendex")],
            ),
        ]);
        let (decisions, _) = decide(&tokens, &found, &[]);
        assert!(matches!(
            only_decision(&decisions).outcome,
            Outcome::Grouped { .. }
        ));
    }

    /// A resolved redirect and its target are the same word too.
    #[test]
    fn a_redirect_and_its_target_are_one_interpretation() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let mut redirect = record("かっこ良い", "かっこいい", -1577490, "jitendex");
        redirect.id.sequence = Some(-1577490);
        redirect.id.same_word_as = Some(1577490);
        let found = generated(vec![
            grouping(
                &tokens,
                0,
                1,
                vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
            ),
            grouping(&tokens, 0, 1, vec![redirect]),
        ]);
        let (decisions, _) = decide(&tokens, &found, &[]);
        assert!(matches!(
            only_decision(&decisions).outcome,
            Outcome::Grouped { .. }
        ));
    }

    /// **An unsupported fragment does not veto a grouping.**
    #[test]
    fn a_fragment_with_no_support_does_not_force_abstention() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let mut fragment = grouping(&tokens, 0, 1, Vec::new());
        fragment.dict_form = "かっこい".into();
        let found = generated(vec![
            grouping(
                &tokens,
                0,
                1,
                vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
            ),
            fragment,
        ]);
        let (decisions, _) = decide(&tokens, &found, &[]);
        assert!(matches!(
            only_decision(&decisions).outcome,
            Outcome::Grouped { .. }
        ));
    }

    /// **The same word claimed over two extents is a disagreement about where the word is.**
    ///
    /// # Why this is reached through the construction route
    ///
    /// The lexical whitelist cannot produce this case, and that is worth stating rather than
    /// leaving as an accident: a supported shape is a non-inflecting word followed by a
    /// classified adjective head, so two such spans can only overlap by sharing a token that is
    /// the head of one and the left constituent of the other — which the same rules forbid. The
    /// guard is still real, because it is what the rule would have to survive if the whitelist
    /// ever widened, so it is exercised where the extents *can* be varied.
    ///
    /// The overlapping occurrences are built here rather than matched. The matcher does not emit
    /// overlapping occurrences today; this is a test of `decide`'s conflict rule, not of the
    /// matcher, and pretending otherwise would be testing two things and checking neither.
    #[test]
    fn one_word_over_two_extents_abstains() {
        let tokens = vec![
            noun("映画", 0),
            token("と", 6),
            token("か", 9),
            token("です", 12),
        ];
        let places = [
            Occurrence {
                start: 6,
                end: 12,
                first_token: 1,
                last_token_end: 3,
            },
            Occurrence {
                start: 6,
                end: 18,
                first_token: 1,
                last_token_end: 4,
            },
        ];
        let mut near = grouping(
            &tokens,
            1,
            2,
            vec![record("とか", "とか", 1008490, "jitendex")],
        );
        near.origin = Origin::Construction;
        let mut far = grouping(
            &tokens,
            1,
            3,
            vec![record("とか", "とか", 1008490, "jitendex")],
        );
        far.origin = Origin::Construction;
        let found = generated(vec![near, far]);

        // Both are eligible on their own: each is exactly an occurrence, with assistance behind it.
        for index in 0..2 {
            assert_eq!(
                assess(&found.candidates[index], &tokens, &found, &places),
                Eligibility::Eligible,
                "candidate {index}"
            );
        }
        // Together they are one word over two stretches of text, and that is not settled.
        let (decisions, _) = decide(&tokens, &found, &places);
        let Outcome::Abstained { alternatives, .. } = &only_decision(&decisions).outcome else {
            panic!("expected abstention over two extents");
        };
        assert_eq!(alternatives, &vec![0, 1]);
    }

    /// **A reading match is not grammatical evidence**, so nothing rests on one.
    #[test]
    fn a_reading_only_recovery_is_never_chosen() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let mut candidate = grouping(
            &tokens,
            0,
            1,
            vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
        );
        candidate.origin = Origin::InflectionRecovery;
        candidate.derivation = Derivation::Inflection {
            class: "adj-i",
            ground: InflectionGround::ReadingOnly,
            steps: Vec::new(),
        };
        let found = generated(vec![candidate]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("nothing but a matching reading supports the unit")
        );
        assert!(decide(&tokens, &found, &[]).0.is_empty());
    }

    /// Composed morphology is evidence, and is not refused for being composed — it is weighed
    /// as what it is, and the other requirements still apply.
    #[test]
    fn composed_morphology_grounds_a_recovered_unit() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let mut candidate = grouping(
            &tokens,
            0,
            1,
            vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
        );
        candidate.origin = Origin::InflectionRecovery;
        candidate.derivation = Derivation::Inflection {
            class: "adj-i",
            ground: InflectionGround::ComposedMorphology {
                head: 1,
                class: "adj-i".into(),
            },
            steps: Vec::new(),
        };
        let found = generated(vec![candidate]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Eligible
        );
    }

    /// **An incomplete lookup prevents automatic selection.**
    #[test]
    fn a_failed_dictionary_blocks_the_region() {
        let (tokens, mut found) = acceptance();
        found.failures.push(Failure {
            dictionary: "jitendex".into(),
            span: (0, 15),
            query: "かっこいい".into(),
            error: "index is corrupt".into(),
        });
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("a dictionary failed over this region")
        );
        assert!(decide(&tokens, &found, &[]).0.is_empty());
    }

    /// An interpretation nobody looked at cannot be known not to disagree.
    #[test]
    fn an_unexamined_extension_blocks_the_region() {
        let (tokens, mut found) = acceptance();
        found.unsearched.push(Unsearched {
            from: 0,
            first_unsearched: (9, 15),
            reason: "stopped at the token bound",
        });
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("an admissible interpretation over this region was unexamined")
        );
    }

    /// A name, a redirect, or an answer about a component cannot make a unit.
    #[test]
    fn only_an_ordinary_whole_word_answer_can_group() {
        let (tokens, _) = acceptance();
        for kind in [
            AnswerKind::NameOnly,
            AnswerKind::UnresolvedPointer,
            AnswerKind::NoDefinition,
        ] {
            let mut evidence = record("格好いい", "かっこいい", 1577490, "jitendex");
            evidence.kind = kind;
            let found = generated(vec![grouping(&tokens, 0, 1, vec![evidence])]);
            assert_eq!(
                assess(&found.candidates[0], &tokens, &found, &[]),
                Eligibility::Rejected("no record supports treating this span as a word"),
                "{kind:?}"
            );
        }
        let mut component = record("格好いい", "かっこいい", 1577490, "jitendex");
        component.retrieved = Retrieved::Component {
            query: "かっこ".into(),
        };
        let found = generated(vec![grouping(&tokens, 0, 1, vec![component])]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("no record supports treating this span as a word")
        );
    }

    /// **The whitelist refuses everything it does not name**, which is where the "no swallowed
    /// connector, no swallowed auxiliary chain" rules come from: they are consequences of naming
    /// two shapes, not a list of things to remember to exclude.
    #[test]
    fn the_structure_whitelist_refuses_what_it_does_not_name() {
        // A connector standing on its own.
        let connector = vec![noun("本", 0), token("を", 3)];
        assert_eq!(structure(&connector), None);
        let leading = vec![token("を", 0), token("いい", 3)];
        assert_eq!(structure(&leading), None);

        // An auxiliary chain: the left constituent inflects, so this is not a noun compound.
        let chain = vec![inflected("読んで", 0, "v5m"), token("いる", 9)];
        assert_eq!(structure(&chain), None);

        // A verb head is not an adjective head, however the left side looks.
        let verb = vec![noun("勉強", 0), inflected("した", 6, "vs-i")];
        assert_eq!(structure(&verb), None);

        // Three tokens: longer compounds are real, and nothing here has shown which are safe.
        let three = vec![noun("かっこ", 0), adjective("いい", 9), token("ね", 15)];
        assert_eq!(structure(&three), None);

        // Two non-inflecting words the analyzer classified as nothing: と + か, which is the
        // enumerative construction's endpoint and not a lexical compound. Refusing it here is
        // what keeps the hearsay とか — and every other と followed by か — out of grouping.
        assert_eq!(structure(&laid_out(&["と", "か"])), None);

        // And the shapes that are supported. Lindera classifies the bare いい as `adj-i`, so the
        // acceptance case is not paying for this requirement.
        assert_eq!(
            structure(&[noun("かっこ", 0), adjective("いい", 9)]),
            Some(Structure::NounAdjective)
        );
        let inflected_head = vec![noun("かっこ", 0), inflected("よかった", 9, "adj-i")];
        assert_eq!(structure(&inflected_head), Some(Structure::NounAdjective));

        let mut suffix = token("向け", 6);
        suffix.lexical_role = Some(LexicalRole::Suffix);
        assert_eq!(
            structure(&[noun("子供", 0), suffix]),
            Some(Structure::NounSuffix)
        );
    }

    /// **A lexical candidate whose shape the whitelist does not name is refused**, however good
    /// its dictionary evidence is. Evidence that a span is a word is not evidence that joining
    /// these particular tokens is safe.
    #[test]
    fn a_lexical_unit_with_an_unsupported_shape_is_refused() {
        // 読んで + いる: a real auxiliary chain, and 読んでいる is a real thing to look up. The
        // left constituent inflects, so this is not a compound to join.
        let tokens = vec![inflected("読んで", 0, "v5m"), token("いる", 9)];
        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![record("読んでいる", "よんでいる", 1234567, "jitendex")],
        )]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("the structure is not one grouping supports")
        );
        assert!(decide(&tokens, &found, &[]).0.is_empty());
    }

    /// **The left constituent must be established as a noun, not merely unrefuted.**
    ///
    /// Three ways a token can fail to be one, and the model does not distinguish them: an adverb,
    /// a connector, and a word UniDic could not classify all arrive with no role. None of them is
    /// a noun, and none of them may be joined to the adjective that follows it. The head is the
    /// same in every case, so the head requirement is not what refuses them.
    #[test]
    fn only_an_established_noun_may_stand_left_of_a_head() {
        let head = adjective("いい", 9);
        assert_eq!(
            structure(&[noun("かっこ", 0), head.clone()]),
            Some(Structure::NounAdjective),
            "the established case, so the others fail for the reason claimed"
        );

        // とても — an adverb. UniDic files it 副詞, which is not a role this model carries.
        assert_eq!(structure(&[token("とても", 0), head.clone()]), None);
        // でも — a connector, standing where the sentence needs it.
        assert_eq!(structure(&[token("でも", 0), head.clone()]), None);
        // A word the analyzer placed nowhere at all.
        assert_eq!(structure(&[token("ｘｙｚ", 0), head.clone()]), None);
        // A pronoun is classified, and is still not a noun that heads compounds.
        let mut pronoun = token("それ", 0);
        pronoun.lexical_role = Some(LexicalRole::Pronoun);
        assert_eq!(structure(&[pronoun, head]), None);
    }

    /// **A whole-entry class that contradicts the structure refuses the unit**, however well the
    /// tokens line up. The shape says these two could be a compound adjective; the record says
    /// the word it found is a verb.
    #[test]
    fn an_entry_whose_class_contradicts_the_structure_is_refused() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![classed(
                "格好いい",
                "かっこいい",
                1577490,
                "jitendex",
                &["v5r"],
            )],
        )]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("the record's class contradicts the unit's structure")
        );
    }

    /// **An entry that asserts no class anywhere is an alternative, not an answer.**
    ///
    /// This is the plan's rule for absent morphology, and it is the reason the *sense* tags are
    /// read at all: 格好いい's `rules` are empty and its definition tags are `★`, `priority`,
    /// `form`. Had its senses not said `adjective`, the acceptance case would sit here.
    #[test]
    fn an_entry_that_asserts_no_class_is_not_selected() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![classed("格好いい", "かっこいい", 1577490, "jitendex", &[])],
        )]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("no record asserts a class for the unit")
        );
    }

    /// **A record that reads differently is a different word.**
    ///
    /// An established lemma reading only means one was written down. The entry a whole-query hit
    /// returned has to actually read the way the unit is proposed to read, or the unit is resting
    /// on a record about something else that happens to be spelled the same.
    #[test]
    fn an_entry_with_a_conflicting_reading_is_refused() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let mut candidate = grouping(
            &tokens,
            0,
            1,
            vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
        );
        // The unit is proposed as かっこいい; the record says かっこういい, a different reading of
        // the same spelling.
        candidate.evidence[0].reading = "かっこういい".into();
        let found = generated(vec![candidate]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("no record reads the way the unit is proposed to read")
        );
    }

    /// **Two readings of one sequence are two interpretations.**
    ///
    /// Canonicalizing on the sequence alone collapsed them into a single claim and let the first
    /// candidate win — undoing in the selector the partitioning generation performs.
    ///
    /// # Why this is a recovered case
    ///
    /// An as-written unit cannot reach it, and that is a property of the surface-reading check
    /// rather than an accident: both candidates would have to read the way the analyzer read the
    /// span, so both would have the same reading. Recovery is where two readings survive — this
    /// is §2's `かっこよかった` ambiguity, where one record has a suppletive sibling recorded for
    /// the same word and another does not, so one proposes かっこいい and the other かっこよい.
    #[test]
    fn one_sequence_with_two_readings_is_two_claims() {
        let tokens = vec![noun("かっこ", 0), inflected("よかった", 9, "adj-i")];
        let recovered = |lemma: &str, locator: &str| {
            let mut record = record("格好良い", lemma, 1577490, "jitendex");
            record.id.locator = locator.into();
            let mut candidate = grouping(&tokens, 0, 1, vec![record]);
            candidate.origin = Origin::InflectionRecovery;
            candidate.derivation = Derivation::Inflection {
                class: "adj-i",
                ground: InflectionGround::ComposedMorphology {
                    head: 1,
                    class: "adj-i".into(),
                },
                steps: Vec::new(),
            };
            candidate
        };
        let found = generated(vec![
            recovered("かっこいい", "with-variant"),
            recovered("かっこよい", "without-variant"),
        ]);
        // Both qualify on their own: the transformation is what connects each lemma reading to
        // the surface, and the surfaces are identical.
        for index in 0..2 {
            assert_eq!(
                assess(&found.candidates[index], &tokens, &found, &[]),
                Eligibility::Eligible,
                "candidate {index}"
            );
        }
        assert!(
            matches!(
                only_decision(&decide(&tokens, &found, &[]).0).outcome,
                Outcome::Abstained { .. }
            ),
            "two readings of one sequence are a disagreement"
        );
    }

    /// **The analyzer's reading is independent evidence, and it is checked.**
    ///
    /// A whole-query hit can return a record for the same spelling read a different way — 上手 is
    /// read じょうず and うわて — and the lemma reading cannot notice, because generation copied
    /// it from that very record. What notices is what the analyzer read over this source.
    #[test]
    fn a_record_that_reads_differently_from_the_analyzer_is_refused() {
        let tokens = vec![
            Token {
                lexical_role: Some(LexicalRole::Noun),
                ..read("上", "じょう", 0)
            },
            {
                let mut head = read("手", "ず", 3);
                head.conjugation = Some(Conjugation {
                    lemma: "手".into(),
                    reading: "ず".into(),
                    props: vec![Prop {
                        pos: "adj-i".into(),
                        form: "Non-past".into(),
                        is_base: true,
                        ..Default::default()
                    }],
                });
                head
            },
        ];
        // The analyzer read じょうず; the record is the うわて entry.
        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![record("上手", "うわて", 1352320, "jitendex")],
        )]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("no record reads the way the analyzer read the span")
        );

        // The same record for the reading the analyzer actually established is eligible, so the
        // refusal is about the reading and not the shape.
        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![record("上手", "じょうず", 1352320, "jitendex")],
        )]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Eligible
        );
    }

    /// **A span the analyzer could not read is not grouped.**
    #[test]
    fn a_span_with_no_established_reading_is_refused() {
        let mut tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
        )]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Eligible
        );

        // One constituent with no reading leaves the run's reading incomplete, and an incomplete
        // reading is not a reading that happens to be empty.
        tokens[1].reading = String::new();
        let mut blank = found.candidates[0].clone();
        blank.surface_reading = Reading::Incomplete;
        let found = generated(vec![blank]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("the analyzer established no reading for the span")
        );
    }

    /// **One sequence and one reading, two incompatible classes, two claims.**
    #[test]
    fn one_sequence_with_incompatible_classes_is_assessed_separately() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let adjectival = grouping(
            &tokens,
            0,
            1,
            vec![classed(
                "格好いい",
                "かっこいい",
                1577490,
                "jitendex",
                &["adj-i"],
            )],
        );
        let nominal = grouping(
            &tokens,
            0,
            1,
            vec![classed(
                "格好いい",
                "かっこいい",
                1577490,
                "jitendex",
                &["n"],
            )],
        );
        // The two records are separated by class, so they do not pool into one claim. Only the
        // adjectival one satisfies the structure, so the nominal one is not a competing *claim* —
        // it is refused outright, which is the stronger of the two answers.
        let found = generated(vec![adjectival, nominal]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Eligible
        );
        assert_eq!(
            assess(&found.candidates[1], &tokens, &found, &[]),
            Eligibility::Rejected("the record's class contradicts the unit's structure")
        );
    }

    /// **Two incompatible classes over one span are two claims.**
    ///
    /// Reached through the suffix shape, where the requirement is that the record commit to some
    /// class rather than to a particular one — so two records can both qualify while describing
    /// different kinds of word, and choosing between them is not this layer's to make.
    #[test]
    fn two_incompatible_classes_over_one_span_abstain() {
        let mut suffix = read("向け", "むけ", 6);
        suffix.lexical_role = Some(LexicalRole::Suffix);
        let tokens = vec![
            Token {
                lexical_role: Some(LexicalRole::Noun),
                ..read("子供", "こども", 0)
            },
            suffix,
        ];

        let found = generated(vec![
            grouping(
                &tokens,
                0,
                1,
                vec![classed("子供向け", "こどもむけ", 111, "jitendex", &["n"])],
            ),
            grouping(
                &tokens,
                0,
                1,
                vec![classed("子供向け", "こどもむけ", 111, "jitendex", &["v5r"])],
            ),
        ]);
        for index in 0..2 {
            assert_eq!(
                assess(&found.candidates[index], &tokens, &found, &[]),
                Eligibility::Eligible,
                "candidate {index}"
            );
        }
        assert!(matches!(
            only_decision(&decide(&tokens, &found, &[]).0).outcome,
            Outcome::Abstained { .. }
        ));
    }

    /// **A suffix unit whose record commits to no class is refused.**
    ///
    /// The `noun+suffix` shape does not claim a particular class, so what is required is that the
    /// record commit to *one*. Two records that each commit — to different classes — are a
    /// disagreement; a record that commits to nothing is not evidence at all, and the two are
    /// different failures needing different controls.
    #[test]
    fn a_suffix_unit_with_no_asserted_class_is_refused() {
        let mut suffix = read("向け", "むけ", 6);
        suffix.lexical_role = Some(LexicalRole::Suffix);
        let tokens = vec![
            Token {
                lexical_role: Some(LexicalRole::Noun),
                ..read("子供", "こども", 0)
            },
            suffix,
        ];

        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![classed("子供向け", "こどもむけ", 111, "jitendex", &[])],
        )]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("no record asserts a class for the unit")
        );

        // …and one that does commit is eligible, so the refusal is about the class and not the
        // shape.
        let found = generated(vec![grouping(
            &tokens,
            0,
            1,
            vec![classed("子供向け", "こどもむけ", 111, "jitendex", &["n"])],
        )]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Eligible
        );
    }

    /// …but one class written two ways is **one** claim, not a disagreement.
    #[test]
    fn one_class_written_two_ways_is_one_claim() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let found = generated(vec![
            grouping(
                &tokens,
                0,
                1,
                vec![classed(
                    "格好いい",
                    "かっこいい",
                    1577490,
                    "jitendex",
                    &["adj-i"],
                )],
            ),
            grouping(
                &tokens,
                0,
                1,
                vec![classed(
                    "格好良い",
                    "かっこいい",
                    1577490,
                    "jitendex",
                    &["adj-ix"],
                )],
            ),
        ]);
        assert!(
            matches!(
                only_decision(&decide(&tokens, &found, &[]).0).outcome,
                Outcome::Grouped { .. }
            ),
            "adj-i and adj-ix are the same class, and abstaining between them would be abstaining \
             on nothing"
        );
    }

    /// **Reversing the order the candidates arrive in does not change the decision.**
    #[test]
    fn the_decision_does_not_depend_on_candidate_order() {
        let tokens = vec![noun("かっこ", 0), adjective("いい", 9)];
        let forward = vec![
            grouping(
                &tokens,
                0,
                1,
                vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
            ),
            grouping(
                &tokens,
                0,
                1,
                vec![record("格好良い", "かっこいい", 1577490, "jitendex")],
            ),
        ];
        let mut reversed = forward.clone();
        reversed.reverse();

        let decide_for = |candidates: Vec<Candidate>| {
            let found = generated(candidates);
            let (decisions, _) = decide(&tokens, &found, &[]);
            decisions
                .iter()
                .map(|decision| match &decision.outcome {
                    Outcome::Grouped { candidate, route } => (
                        decision.start,
                        decision.end,
                        found.candidates[*candidate].dict_form.clone(),
                        Some(*route),
                        found.candidates[*candidate]
                            .lemma_reading
                            .established()
                            .unwrap_or_default()
                            .to_string(),
                    ),
                    Outcome::Abstained { .. } => (
                        decision.start,
                        decision.end,
                        String::new(),
                        None,
                        String::new(),
                    ),
                })
                .collect::<Vec<_>>()
        };

        let first = decide_for(forward);
        let second = decide_for(reversed);
        // One claim either way — the two spellings are one interpretation — and the substantive
        // decision is the same span, the same reading and the same route. Which of two identical
        // records is named is not a substantive difference, so the spelling is not compared.
        assert_eq!(first.len(), 1);
        assert_eq!(
            first
                .iter()
                .map(|(start, end, _, route, reading)| (*start, *end, *route, reading.clone()))
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|(start, end, _, route, reading)| (*start, *end, *route, reading.clone()))
                .collect::<Vec<_>>()
        );
    }

    /// An inflecting head the analyzer could say nothing about is left alone.
    #[test]
    fn an_inflecting_head_with_no_class_is_not_a_supported_structure() {
        let mut head = token("よかった", 9);
        head.conjugation = Some(Conjugation {
            lemma: "よい".into(),
            reading: "よい".into(),
            props: vec![Prop::default()],
        });
        assert_eq!(structure(&[noun("かっこ", 0), head]), None);
    }

    // ---------------------------------------------------------------- the construction route

    fn occurrence_match(tokens: &[Token]) -> Match {
        // The real matcher over the real rule, so the element spans are not invented here.
        crate::grammar::matcher::find(tokens, crate::grammar::pack::hanabira())
            .into_iter()
            .find(|found| found.pattern_id == "hanabira-n4-a-b")
            .expect("the enumerative rule fires")
    }

    /// The production list, so a test cannot quietly diverge from what ships.
    const OFFERED: &[(&str, &[usize], &str)] = GROUPED_CONSTRUCTIONS;

    /// **Occurrences are read from the element spans**, never re-derived from the text.
    #[test]
    fn occurrences_come_from_the_validated_element_spans() {
        let tokens = vec![
            noun("映画", 0),
            token("と", 6),
            token("か", 9),
            noun("買い物", 12),
            token("と", 21),
            token("か", 24),
        ];
        let found = occurrence_match(&tokens);
        let places = occurrences(&tokens, std::slice::from_ref(&found), OFFERED);
        assert_eq!(
            places
                .iter()
                .map(|place| (place.start, place.end))
                .collect::<Vec<_>>(),
            vec![(6, 12), (21, 27)]
        );

        // A match without element spans — an older payload — yields none rather than a scan.
        let mut older = found.clone();
        older.elements.clear();
        assert!(occurrences(&tokens, std::slice::from_ref(&older), OFFERED).is_empty());

        // **And an element whose ending could not be placed yields none either**, rather than
        // falling back to the match's own span. The match covers 映画とか買い物とか; an
        // occurrence that claimed all of it would be a different and much larger claim.
        let mut unplaced = found.clone();
        for element in &mut unplaced.elements {
            element.suffix_start = None;
            element.suffix_end = None;
        }
        assert!(unplaced.start.is_some() && unplaced.end.is_some());
        assert!(
            occurrences(&tokens, std::slice::from_ref(&unplaced), OFFERED).is_empty(),
            "an unplaceable ending fell back to the parent's span"
        );

        // **A shifted endpoint is refused here too**, which only the shared validator can see.
        // Moving the first endpoint's ending onto the second one leaves a span that is six bytes
        // long, inside the parent, and belongs to an element whose run really does end with とか
        // — everything a reader of the suffix fields alone would check. What refuses it is that
        // the ending is no longer at the end of the run it was found in.
        let mut shifted = found.clone();
        let second = shifted
            .element(2)
            .expect("element 2")
            .suffix()
            .expect("ending");
        let first = shifted
            .elements
            .iter_mut()
            .find(|span| span.element == 0)
            .expect("element 0");
        first.suffix_start = Some(second.0);
        first.suffix_end = Some(second.1);
        let places = occurrences(&tokens, std::slice::from_ref(&shifted), OFFERED);
        assert_eq!(
            places
                .iter()
                .map(|place| (place.start, place.end))
                .collect::<Vec<_>>(),
            vec![(21, 27)],
            "the shifted endpoint was reported as if the matcher had placed it"
        );
    }

    /// A construction-backed unit must be exactly a validated occurrence.
    #[test]
    fn a_construction_unit_must_be_exactly_an_occurrence() {
        let tokens = vec![
            noun("映画", 0),
            token("と", 6),
            token("か", 9),
            noun("買い物", 12),
            token("と", 21),
            token("か", 24),
        ];
        let places = occurrences(
            &tokens,
            std::slice::from_ref(&occurrence_match(&tokens)),
            OFFERED,
        );

        let mut unit = grouping(
            &tokens,
            1,
            2,
            vec![record("とか", "とか", 1008490, "jitendex")],
        );
        unit.origin = Origin::Construction;
        let found = generated(vec![unit]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &places),
            Eligibility::Eligible
        );

        // One token further, and it is no longer the occurrence the matcher validated.
        let mut spilled = grouping(
            &tokens,
            1,
            3,
            vec![record("とか", "とか", 1008490, "jitendex")],
        );
        spilled.origin = Origin::Construction;
        let found = generated(vec![spilled]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &places),
            Eligibility::Rejected("no validated occurrence covers this unit exactly")
        );

        // And with no occurrences at all — an older payload — nothing is eligible.
        let mut unit = grouping(
            &tokens,
            1,
            2,
            vec![record("とか", "とか", 1008490, "jitendex")],
        );
        unit.origin = Origin::Construction;
        let found = generated(vec![unit]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &[]),
            Eligibility::Rejected("no validated occurrence covers this unit exactly")
        );
    }

    /// A construction unit still needs dictionary assistance behind it.
    #[test]
    fn a_construction_unit_without_assistance_is_refused() {
        let tokens = vec![
            noun("映画", 0),
            token("と", 6),
            token("か", 9),
            noun("買い物", 12),
            token("と", 21),
            token("か", 24),
        ];
        let places = occurrences(
            &tokens,
            std::slice::from_ref(&occurrence_match(&tokens)),
            OFFERED,
        );
        let mut unit = grouping(&tokens, 1, 2, Vec::new());
        unit.origin = Origin::Construction;
        let found = generated(vec![unit]);
        assert_eq!(
            assess(&found.candidates[0], &tokens, &found, &places),
            Eligibility::Rejected("the occurrence has no dictionary assistance behind it")
        );
    }

    /// **The construction route does not go through the lexical structure whitelist.**
    ///
    /// と + か is not a noun and an adjective, and it never will be. That is the whole reason
    /// there are two routes: a lexical-only layer would deliver assistance for とか and never the
    /// grouping the reader asked for.
    #[test]
    fn a_construction_unit_is_not_held_to_the_lexical_structure() {
        let tokens = vec![
            noun("映画", 0),
            token("と", 6),
            token("か", 9),
            noun("買い物", 12),
            token("と", 21),
            token("か", 24),
        ];
        assert_eq!(structure(&tokens[1..=2]), None, "と+か is no lexical shape");

        let places = occurrences(
            &tokens,
            std::slice::from_ref(&occurrence_match(&tokens)),
            OFFERED,
        );
        let mut unit = grouping(
            &tokens,
            1,
            2,
            vec![record("とか", "とか", 1008490, "jitendex")],
        );
        unit.origin = Origin::Construction;
        let found = generated(vec![unit]);
        let (decisions, _) = decide(&tokens, &found, &places);
        assert_eq!(
            only_decision(&decisions).outcome,
            Outcome::Grouped {
                candidate: 0,
                route: Route::Construction
            }
        );
    }

    /// Two regions far apart are decided separately: one abstention does not silence the other.
    #[test]
    fn regions_are_decided_independently() {
        let tokens = vec![
            noun("かっこ", 0),
            adjective("いい", 9),
            token("、", 15),
            noun("かっこ", 18),
            adjective("いい", 27),
        ];
        let found = generated(vec![
            grouping(
                &tokens,
                0,
                1,
                vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
            ),
            grouping(
                &tokens,
                3,
                4,
                vec![record("格好いい", "かっこいい", 1577490, "jitendex")],
            ),
            // A competitor over the second region only.
            grouping(
                &tokens,
                3,
                4,
                vec![record("恰好好い", "かっこいい", 9999, "jitendex")],
            ),
        ]);
        let (decisions, _) = decide(&tokens, &found, &[]);
        assert_eq!(decisions.len(), 2, "{decisions:?}");
        assert!(matches!(decisions[0].outcome, Outcome::Grouped { .. }));
        assert!(matches!(decisions[1].outcome, Outcome::Abstained { .. }));
    }
}
