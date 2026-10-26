//! Proposing lexical units over a finished analysis, without touching it.
//!
//! # What it produces
//!
//! One [`Candidate`] per **interpretation**, not per span. Records that describe the same word are
//! merged with their evidence kept; records that describe different words over the same bytes stay
//! apart, because that disagreement is exactly what a selector must abstain on — and an earlier
//! version of this file destroyed it by putting every hit into one candidate.
//!
//! # What it refuses
//!
//! **Merging across anything the source does not join.** A run spanning whitespace or punctuation
//! is not a word however well the concatenation looks up: `深夜徘徊 する` joined would produce a
//! token missing the byte its own span still covers.
//!
//! **A valid baseline is a precondition, not an assumption.** An ungrounded or overlapping token
//! is an error here, not a candidate quietly skipped — proposing units over an analysis that fails
//! its own span contract would build on something already known to be wrong.
//!
//! # Limits, and what a limit means
//!
//! [`MOST_TOKENS`] and [`MOST_CHARS`] are resource bounds. A bound is only *reached* when an
//! admissible extension existed and went unexamined. Punctuation, whitespace, the end of the
//! passage and an invalid token are **answers** — reporting them as unsearched made hundreds of
//! complete searches look incomplete. Incompleteness is recorded against the region it affects, so
//! a limit in one clause cannot suppress an improvement in another.

use std::collections::{BTreeMap, BTreeSet};

use super::candidate::{
    Candidate, ClassAgreement, Derivation, DictionaryId, Eligibility, EntryEvidence, EntryId,
    InflectionGround, Origin, Reading,
};
use super::select;
use crate::analyzer::SpanFault;
use crate::dict::evidence::Evidence;
use crate::dict::index::Dictionary;
use crate::dict::merge::Retrieved;
use crate::model::{Analysis, Token};

/// The longest run of baseline tokens a single lexical unit may join.
pub const MOST_TOKENS: usize = 8;
/// The longest source span, in characters, a single lexical unit may cover.
pub const MOST_CHARS: usize = 64;

/// A search stopped with an admissible extension unexamined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsearched {
    /// Byte offset where the interrupted search started.
    pub from: usize,
    /// The first extension it did not examine.
    pub first_unsearched: (usize, usize),
    pub reason: &'static str,
}

/// A span the source itself rules out. A complete answer, not a gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub start: usize,
    pub end: usize,
    pub reason: &'static str,
}

/// A dictionary that failed, with what was asked and where. Not an absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub dictionary: String,
    pub span: (usize, usize),
    pub query: String,
    pub error: String,
}

/// What generation found, and what it could not reach.
#[derive(Debug, Default, Clone)]
pub struct Generated {
    pub candidates: Vec<Candidate>,
    /// Searches interrupted by a bound with an admissible extension left unexamined.
    pub unsearched: Vec<Unsearched>,
    /// Spans the source rules out.
    pub refused: Vec<Refused>,
    /// Dictionary failures, carrying what was asked and where.
    pub failures: Vec<Failure>,
    /// How many times a dictionary was asked something, across every dictionary and every span.
    ///
    /// **The cost, counted rather than estimated.** Recovery runs on every enumerated span
    /// whether or not a whole-form entry exists, which is what lets an inflected form be reached
    /// at all — and it is also work. A claim about what that costs needs this number, and a claim
    /// made without it was withdrawn once already.
    pub queries: usize,
}

impl Generated {
    pub fn groupings(&self) -> impl Iterator<Item = &Candidate> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.joins() > 1)
    }

    /// Whether an unexamined interpretation could have covered this region.
    ///
    /// **Measured against the span the unexamined interpretation *would* have**, from where the
    /// interrupted search began through the end of the extension it never tried — not against the
    /// extension alone. A nine-token reading left unexplored competes with a grouping over its
    /// first two tokens, and checking only the ninth token's bytes reported that grouping complete.
    ///
    /// Still scoped: a bound reached in one clause does not suppress a decision in another.
    pub fn incomplete_over(&self, start: usize, end: usize) -> bool {
        self.unsearched.iter().any(|gap| {
            let possible_end = gap.first_unsearched.1;
            gap.from < end && possible_end > start
        })
    }

    /// Whether a dictionary failed while this region was being searched.
    ///
    /// **An incomplete search is not a complete answer**, so a selector consults this before
    /// choosing anything over the region: the interpretation it would have preferred may be the
    /// one the unreadable dictionary held. Distinct from [`Generated::incomplete_over`], which is
    /// about a search this layer chose to stop rather than one that broke.
    pub fn lookup_failed_over(&self, start: usize, end: usize) -> bool {
        self.failures
            .iter()
            .any(|failure| failure.span.0 < end && failure.span.1 > start)
    }
}

/// Characters a dictionary genuinely uses inside a word.
const WORD_INTERNAL: [char; 4] = ['\u{30fb}', '\u{30fc}', '-', '\''];

/// Why a proposal cannot even be attempted over a span.
///
/// **One policy, both scripts.** The middle dot in a transliterated name and the prolonged sound
/// mark are word-internal; sentence punctuation, brackets and any whitespace are barriers.
fn joinable(source: &str) -> Result<(), &'static str> {
    if source.chars().any(char::is_whitespace) {
        return Err("the span contains whitespace");
    }
    let barrier = source.chars().any(|character| {
        if WORD_INTERNAL.contains(&character) {
            return false;
        }
        character.is_ascii_punctuation()
            || matches!(
                character,
                '\u{3001}'
                    | '\u{3002}'
                    | '\u{ff01}'
                    | '\u{ff1f}'
                    | '\u{ff0c}'
                    | '\u{ff0e}'
                    | '\u{300c}'
                    | '\u{300d}'
                    | '\u{300e}'
                    | '\u{300f}'
                    | '\u{ff08}'
                    | '\u{ff09}'
                    | '\u{2026}'
                    | '\u{ff5e}'
                    | '\u{301c}'
            )
    });
    if barrier {
        return Err("the span contains punctuation");
    }
    Ok(())
}

/// Whether a token could extend a span — used to tell a bound from an answer.
fn admissible(text: &str, token: &Token) -> bool {
    let (Some(start), Some(end)) = (token.start, token.end) else {
        return false;
    };
    text.get(start..end)
        .is_some_and(|source| joinable(source).is_ok())
}

/// The span a contiguous run covers, or `None` if the tokens do not form one.
fn contiguous_span(text: &str, tokens: &[Token], from: usize, to: usize) -> Option<(usize, usize)> {
    let (start, end) = (tokens.get(from)?.start?, tokens.get(to)?.end?);
    if start >= end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return None;
    }
    let mut cursor = start;
    for token in &tokens[from..=to] {
        let (token_start, token_end) = (token.start?, token.end?);
        if token_start != cursor {
            return None;
        }
        cursor = token_end;
    }
    (cursor == end).then_some((start, end))
}

/// The reading the baseline established for a run, or the honest absence of one.
fn run_reading(tokens: &[Token]) -> Reading {
    if tokens.iter().any(|token| token.reading.trim().is_empty()) {
        return Reading::Incomplete;
    }
    Reading::Established(tokens.iter().map(|token| token.reading.as_str()).collect())
}

/// What makes two records one interpretation.
type InterpretationKey = (DictionaryId, Option<i64>, String, String, Vec<String>);

/// The key a record is grouped under.
///
/// **Reading and grammatical class are both part of it.** One sequence can carry several readings
/// — お父さん is filed at 1002590 with both おとうさん and おとっさん — and 上手 read じょうず is not
/// 上手 read うわて. Records can equally share a sequence and a reading while asserting different
/// classes, and pooling those lets one lend the other rules it never claimed.
fn interpretation_key(evidence: &EntryEvidence) -> InterpretationKey {
    let (dictionary, sequence, locator) = evidence.id.same_word_key();
    (
        dictionary,
        sequence,
        locator,
        crate::kana::to_hiragana(&evidence.reading),
        // **Class too, from whichever field records one.** Two records can share a sequence and
        // a reading and assert different grammatical classes; pooled, one would lend the other its
        // rules and its part of speech, and inflection validation would then be checking a
        // proposal against evidence belonging to a different word.
        //
        // `class_evidence` rather than `grammar_class`, because selection canonicalizes claims the
        // same way. Partitioning on rules alone while keying claims on rules-or-senses left the
        // two layers disagreeing about what one interpretation is: two records with empty rules
        // and different sense classes pooled into one candidate here, and the claim selection then
        // made depended on which of them the dictionary happened to return first.
        evidence.class_evidence(),
    )
}

/// Ask every dictionary about one span, grouping records by the word they describe.
fn interpretations(
    span: (usize, usize),
    query: &str,
    dictionaries: Evidence<'_>,
    failures: &mut Vec<Failure>,
    queries: &mut usize,
) -> Vec<Vec<EntryEvidence>> {
    type Key = InterpretationKey;
    let mut grouped: BTreeMap<Key, Vec<EntryEvidence>> = BTreeMap::new();
    let mut order: Vec<Key> = Vec::new();

    *queries += dictionaries.len();
    for source in dictionaries.sources() {
        let dictionary = source.dictionary;
        match source.whole_word(query) {
            Ok(found) => {
                for record in &found {
                    let evidence = EntryEvidence::read(
                        dictionary,
                        record,
                        Retrieved::Whole {
                            query: query.to_string(),
                        },
                    );
                    let key = interpretation_key(&evidence);
                    if !grouped.contains_key(&key) {
                        order.push(key.clone());
                    }
                    grouped.entry(key).or_default().push(evidence);
                }
            }
            Err(error) => failures.push(Failure {
                dictionary: dictionary.installed_as(),
                span,
                query: query.to_string(),
                error: error.to_string(),
            }),
        }
    }
    order
        .into_iter()
        .filter_map(|key| grouped.remove(&key))
        .collect()
}

/// The class the analyzer established for a span, and what asserting it about this span means.
///
/// A single token was described by the analyzer directly. A run of tokens was not — the analyzer
/// described its parts — so the head constituent's class is offered as a **hypothesis about
/// structure**, labelled as one. See [`InflectionGround`].
fn span_morphology(tokens: &[Token], from: usize, to: usize) -> (Option<String>, bool) {
    let head = tokens.last();
    let class = head
        .map(crate::lexical::inflect::TokenMorphology::of_token)
        .and_then(|morphology| morphology.class.map(str::to_string));
    (class, from == to)
}

/// Propose dictionary forms for a span by reversing an inflection, and record what grounds them.
///
/// **Every outcome is reported as itself.** A recovered proposal is a candidate; an interpretation
/// resting on nothing but a matching reading is a candidate too, labelled
/// [`InflectionGround::ReadingOnly`] so a selector can refuse it without re-deriving why; an
/// ambiguity yields every reading it could not choose between, because the disagreement is the
/// finding. A dictionary that failed is recorded against this span, and nothing over it may then
/// be chosen automatically.
#[allow(clippy::too_many_arguments)]
fn recovered(
    span: (usize, usize),
    source: &str,
    run: &[Token],
    from: usize,
    to: usize,
    dictionaries: Evidence<'_>,
    found: &mut Generated,
) {
    use crate::lexical::inflect::{self, Recovered, TokenMorphology};

    let Reading::Established(surface_reading) = run_reading(run) else {
        // Without a reading for the whole run there is nothing to reverse. Not a failure: the
        // analyzer simply did not supply one.
        return;
    };
    let (class, whole_span) = span_morphology(run, from, to);
    let mut look = inflect::reading_lookup_in(dictionaries);
    // Counted where it happens: `recover_adjective` decides how many readings it asks about — one
    // for the regular form, a second only where a suppletive sibling is possible — and guessing
    // from outside would be guessing.
    let mut asked = 0usize;
    let mut counting = |reading: &str| {
        asked += 1;
        look(reading)
    };
    let recovery = inflect::recover_adjective(
        &TokenMorphology {
            reading: &surface_reading,
            class: class.as_deref(),
        },
        &mut counting,
    );
    found.queries += asked * dictionaries.len();

    for failure in &recovery.failures {
        found.failures.push(Failure {
            dictionary: failure.dictionary.clone(),
            span,
            query: failure.reading.clone(),
            error: failure.error.clone(),
        });
    }

    let groups = match &recovery.outcome {
        Recovered::Proposal(lemma) => std::slice::from_ref(lemma),
        Recovered::Unresolved(lemma) => std::slice::from_ref(lemma),
        Recovered::Ambiguous(groups) => groups.as_slice(),
        // Unsupported, contradicted, unvalidated and unavailable all propose nothing. The first
        // is the ordinary answer for most spans; the others are already recorded — a
        // contradiction by being refused, a failed dictionary by the loop above.
        _ => return,
    };

    for lemma in groups {
        for proposal in &lemma.interpretations {
            let ground = if proposal.analyzer_supports_transformation == ClassAgreement::Compatible
            {
                let class = class.clone().unwrap_or_default();
                if whole_span {
                    InflectionGround::TokenMorphology { token: from, class }
                } else {
                    InflectionGround::ComposedMorphology { head: to, class }
                }
            } else if proposal.entry_supports_transformation == ClassAgreement::Compatible {
                InflectionGround::RecordClass
            } else {
                InflectionGround::ReadingOnly
            };

            // The record whose reading **is** the headword's is the one whose spelling to show:
            // the surface reached 格好良い, and 格好いい is the same word's headword.
            let headword = proposal
                .variant
                .as_ref()
                .map_or(&proposal.entry, |variant| &variant.entry);
            let mut evidence = vec![proposal.entry.clone()];
            if let Some(variant) = &proposal.variant {
                evidence.push(variant.entry.clone());
            }

            found.candidates.push(Candidate {
                start: span.0,
                end: span.1,
                tokens: (from..=to).collect(),
                // The source is never rewritten. What was recovered is a reading and the entry it
                // reached.
                surface: source.to_string(),
                dict_form: headword.term.clone(),
                surface_reading: Reading::Established(proposal.surface_reading.clone()),
                lemma_reading: Reading::Established(proposal.lemma_reading.clone()),
                origin: Origin::InflectionRecovery,
                derivation: Derivation::Inflection {
                    class: inflect::TRANSFORMATION_CLASS,
                    ground,
                    steps: proposal.steps.clone(),
                },
                evidence,
                eligibility: Eligibility::Unassessed,
            });
        }
    }
}

/// Propose units for the validated occurrences of recognised constructions.
///
/// **The locations are read, never re-derived.** The matcher already decided where each
/// occurrence is and recorded it as a rule-element span; searching the region again for the
/// characters it expects to find is how two layers come to disagree about the same text. What is
/// added here is the dictionary question — does an entry exist for this occurrence — and nothing
/// about where it is.
///
/// An occurrence that is not a whole run of tokens produces nothing. Ichiran fuses `映画とか` into
/// one token, and the `とか` inside it is a **sub-token** span: a real occurrence, offered as
/// assistance by `expression::offered`, but not something grouping can act on, because there is
/// no seam there to join.
fn from_constructions(analysis: &Analysis, dictionaries: Evidence<'_>, found: &mut Generated) {
    let matches = crate::grammar::patterns(analysis);
    let places = select::occurrences(&analysis.tokens, &matches, select::GROUPED_CONSTRUCTIONS);
    let tokens = &analysis.tokens;

    for place in places {
        let Some(last) = place.last_token_end.checked_sub(1) else {
            continue;
        };
        // Two tokens at least: one token is already a unit, and joining nothing is not grouping.
        if last <= place.first_token {
            continue;
        }
        // The occurrence has to *be* the token run, not sit inside it.
        let Some((start, end)) = contiguous_span(&analysis.text, tokens, place.first_token, last)
        else {
            continue;
        };
        if (start, end) != (place.start, place.end) {
            // **A real occurrence with no seam to join.** The `とか` inside a fused 映画とか is
            // there and is offered as assistance; grouping cannot act on it, because acting
            // would mean splitting a token the analyzer produced whole. Recorded, because that
            // is an answer about this span rather than a gap in the search.
            found.refused.push(Refused {
                start: place.start,
                end: place.end,
                reason: "the occurrence is not a whole run of tokens",
            });
            continue;
        }
        let source = &analysis.text[start..end];
        if joinable(source).is_err() {
            continue;
        }

        for evidence in interpretations(
            (start, end),
            source,
            dictionaries,
            &mut found.failures,
            &mut found.queries,
        ) {
            let lemma_reading = evidence
                .first()
                .map(|record| record.reading.clone())
                .filter(|reading| !reading.trim().is_empty())
                .map_or(Reading::Incomplete, Reading::Established);
            found.candidates.push(Candidate {
                start,
                end,
                tokens: (place.first_token..=last).collect(),
                surface: source.to_string(),
                dict_form: evidence
                    .first()
                    .map(|record| record.term.clone())
                    .unwrap_or_else(|| source.to_string()),
                surface_reading: run_reading(&tokens[place.first_token..=last]),
                lemma_reading,
                origin: Origin::Construction,
                derivation: Derivation::AsWritten,
                evidence,
                eligibility: Eligibility::Unassessed,
            });
        }
    }
}

/// Propose lexical units over a finished analysis.
///
/// Fails when the baseline does not satisfy its own span contract.
pub fn candidates(
    analysis: &Analysis,
    dictionaries: &[Dictionary],
) -> Result<Generated, Vec<SpanFault>> {
    candidates_in(analysis, Evidence::direct(dictionaries))
}

/// [`candidates`], reading through one request's evidence.
pub fn candidates_in(
    analysis: &Analysis,
    dictionaries: Evidence<'_>,
) -> Result<Generated, Vec<SpanFault>> {
    let faults = crate::analyzer::validate_analysis(analysis);
    if !faults.is_empty() {
        return Err(faults);
    }

    let mut found = Generated::default();
    let text = analysis.text.as_str();
    let tokens = &analysis.tokens;

    for (index, token) in tokens.iter().enumerate() {
        let (Some(start), Some(end)) = (token.start, token.end) else {
            continue;
        };
        // **Two different questions.** つたわって is read つたわって; its *lemma* is read つたわる.
        // Assigning the surface reading to both made every inflected baseline token claim a lemma
        // reading it does not have — the evidence inflection recovery is about to validate against.
        let surface_reading = run_reading(std::slice::from_ref(token));
        let lemma_reading = if token.lemma_reading.trim().is_empty() {
            Reading::Incomplete
        } else {
            Reading::Established(token.lemma_reading.clone())
        };
        found.candidates.push(Candidate {
            start,
            end,
            tokens: vec![index],
            surface: token.surface.clone(),
            dict_form: if token.dict_form.is_empty() {
                token.surface.clone()
            } else {
                token.dict_form.clone()
            },
            surface_reading,
            lemma_reading,
            origin: Origin::Baseline,
            derivation: Derivation::AsWritten,
            evidence: Vec::new(),
            eligibility: Eligibility::Unassessed,
        });
    }

    for from in 0..tokens.len() {
        // Why the search from this point stopped. `None` while it is still running, which is the
        // only state in which the token bound can have been the reason.
        let mut stopped: Option<&'static str> = None;
        let mut reached = from;

        for length in 2..=MOST_TOKENS {
            let to = from + length - 1;
            if to >= tokens.len() {
                stopped = Some("the passage ends");
                break;
            }
            let Some((start, end)) = contiguous_span(text, tokens, from, to) else {
                // **A gap between two tokens is source the analyzer dropped** — whitespace, in
                // practice, since the baseline has already been validated and cannot be
                // ungrounded here. That is an answer about this span, so it is recorded as a
                // refusal rather than vanishing into a bare `break`.
                if let (Some(start), Some(end)) = (tokens[from].start, tokens[to].end) {
                    found.refused.push(Refused {
                        start,
                        end,
                        reason: "the tokens are not contiguous in the source",
                    });
                }
                stopped = Some("the tokens are not contiguous");
                break;
            };
            let source = &text[start..end];
            // **Source admissibility before the character bound.** A span both too long and
            // punctuated is refused, not truncated: the punctuation is the answer.
            if let Err(why) = joinable(source) {
                found.refused.push(Refused {
                    start,
                    end,
                    reason: why,
                });
                stopped = Some(why);
                break;
            }
            if source.chars().count() > MOST_CHARS {
                stopped = Some("longer than the character bound");
                found.unsearched.push(Unsearched {
                    from: start,
                    first_unsearched: (
                        tokens[to].start.unwrap_or(start),
                        tokens[to].end.unwrap_or(end),
                    ),
                    reason: "longer than the character bound",
                });
                break;
            }
            reached = to;

            // **Recovery runs whether or not the span has a whole-form entry.** かっこよかった
            // is not a headword in any dictionary and never will be — inflected forms are not
            // filed — so requiring a whole-form hit first would refuse the case this layer exists
            // for before it was ever asked. What a whole-form hit and a recovered lemma have in
            // common is that both are proposals about one span; which of them a reader is shown
            // is selection's question, not generation's.
            recovered(
                (start, end),
                source,
                &tokens[from..=to],
                from,
                to,
                dictionaries,
                &mut found,
            );

            for evidence in interpretations(
                (start, end),
                source,
                dictionaries,
                &mut found.failures,
                &mut found.queries,
            ) {
                // The surface reading is what the analyzer established for these tokens; the lemma
                // reading is the entry's own. For かっこいい they coincide; for an inflected run
                // they do not, and conflating them would hand §2 mislabelled evidence.
                let surface_reading = run_reading(&tokens[from..=to]);
                let lemma_reading = evidence
                    .first()
                    .map(|record| record.reading.clone())
                    .filter(|reading| !reading.trim().is_empty())
                    .map_or(Reading::Incomplete, Reading::Established);
                found.candidates.push(Candidate {
                    start,
                    end,
                    tokens: (from..=to).collect(),
                    surface: source.to_string(),
                    dict_form: evidence
                        .first()
                        .map(|record| record.term.clone())
                        .unwrap_or_else(|| source.to_string()),
                    surface_reading,
                    lemma_reading,
                    origin: Origin::LexicalLookup,
                    derivation: Derivation::AsWritten,
                    evidence,
                    eligibility: Eligibility::Unassessed,
                });
            }
        }

        // **A bound is reached only if something admissible went unexamined.** The token after the
        // last one searched decides: punctuation, an invalid token, or the passage ending are all
        // answers, and only an admissible token makes this an unanswered question.
        if stopped.is_none() {
            let next = reached + 1;
            // **Contiguity as well as characters.** Eight tokens, a space, then another word is a
            // source refusal: the ninth token cannot extend this span however word-like it looks,
            // so reporting budget exhaustion would invent an unanswered question.
            let extendable = tokens
                .get(next)
                .filter(|token| admissible(text, token))
                .filter(|_| contiguous_span(text, tokens, from, next).is_some());
            match (extendable, tokens.get(next)) {
                (Some(token), _) => {
                    if let (Some(start), Some(end)) = (token.start, token.end) {
                        found.unsearched.push(Unsearched {
                            from: tokens[from].start.unwrap_or(start),
                            first_unsearched: (start, end),
                            reason: "stopped at the token bound",
                        });
                    }
                }
                (None, Some(token)) => {
                    // There is a next token and it cannot extend this span. An answer.
                    if let (Some(start), Some(end)) = (tokens[from].start, token.end) {
                        found.refused.push(Refused {
                            start,
                            end,
                            reason: "the next token cannot extend this span",
                        });
                    }
                }
                (None, None) => {}
            }
        }
    }

    dedupe(&mut found.candidates);
    // Construction-backed units, from the occurrences the matcher validated. Run after the span
    // sweep so that a span reached both ways carries both interpretations and selection sees the
    // agreement — or the disagreement — rather than whichever ran first.
    from_constructions(analysis, dictionaries, &mut found);

    Ok(found)
}

/// Collapse proposals that are the same interpretation of the same span.
fn dedupe(candidates: &mut Vec<Candidate>) {
    let mut seen: BTreeSet<(usize, usize, String, String, Vec<EntryId>)> = BTreeSet::new();
    let mut keep = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        let key = (
            candidate.start,
            candidate.end,
            candidate.dict_form.clone(),
            candidate
                .lemma_reading
                .established()
                .unwrap_or("")
                .to_string(),
            candidate
                .evidence
                .iter()
                .map(|record| record.id.clone())
                .collect::<Vec<_>>(),
        );
        if seen.insert(key) {
            keep.push(candidate);
        }
    }
    keep.sort_by(|a, b| {
        (a.start, a.end, a.origin)
            .cmp(&(b.start, b.end, b.origin))
            .then_with(|| a.dict_form.cmp(&b.dict_form))
    });
    *candidates = keep;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(surface: &str, reading: &str, start: usize) -> Token {
        Token {
            surface: surface.to_string(),
            dict_form: surface.to_string(),
            reading: reading.to_string(),
            start: Some(start),
            end: Some(start + surface.len()),
            ..Default::default()
        }
    }

    fn analysis(text: &str, tokens: Vec<Token>) -> Analysis {
        Analysis {
            execution: None,
            text: text.to_string(),
            tokens,
        }
    }

    /// **A construction occurrence that is not a whole run of tokens produces no unit.**
    ///
    /// The ending is allowed to begin *inside* a token — that is what `surface_ends` means, and
    /// §3 records where it begins. Here `映画と` holds the と, so the element consumes 映画と + か
    /// while the occurrence is only the last six bytes of that run. There is no seam between 映画
    /// and と for grouping to act on, so the answer is a refusal rather than a unit over source
    /// the analyzer did not divide there.
    ///
    /// No dictionaries are needed: what is under test is whether the span is *considered*, and
    /// the refusal is recorded before any lookup.
    #[test]
    fn an_occurrence_inside_a_token_is_refused_rather_than_grouped() {
        let text = "映画とか買い物とか";
        let analysed = analysis(
            text,
            vec![
                token("映画と", "エイガト", 0),
                token("か", "カ", 9),
                token("買い物と", "カイモノト", 12),
                token("か", "カ", 24),
            ],
        );
        let mut found = Generated::default();
        from_constructions(&analysed, Evidence::direct(&[]), &mut found);

        let refused: Vec<(usize, usize, &str)> = found
            .refused
            .iter()
            .map(|refusal| (refusal.start, refusal.end, refusal.reason))
            .collect();
        assert!(
            refused.iter().any(|(start, end, reason)| {
                *reason == "the occurrence is not a whole run of tokens"
                    && (*start, *end) == (6, 12)
            }),
            "the occurrence beginning inside a token was not recorded as refused: {refused:?}"
        );
        assert!(
            found
                .candidates
                .iter()
                .all(|candidate| candidate.origin != Origin::Construction),
            "a unit was proposed over source the analyzer did not divide there"
        );
    }

    #[test]
    fn a_span_is_never_joined_across_a_space() {
        let text = "徘徊 する";
        let tokens = vec![token("徘徊", "ハイカイ", 0), token("する", "スル", 7)];
        assert!(contiguous_span(text, &tokens, 0, 1).is_none());
    }

    /// One policy over both scripts, with word-internal marks kept.
    #[test]
    fn the_punctuation_policy_covers_both_scripts() {
        assert!(joinable("かっこいい").is_ok());
        assert!(
            joinable("パク・チウォン").is_ok(),
            "a name separator is word-internal"
        );
        assert!(
            joinable("メーカー").is_ok(),
            "the prolonged sound mark is not punctuation"
        );
        assert_eq!(joinable("徘徊 する"), Err("the span contains whitespace"));
        assert_eq!(joinable("これ、それ"), Err("the span contains punctuation"));
        assert_eq!(joinable("a.b"), Err("the span contains punctuation"));
        assert_eq!(joinable("「音」"), Err("the span contains punctuation"));
    }

    /// **The limit-accounting case.** Eight tokens then punctuation is an answer, not a bound.
    #[test]
    fn punctuation_after_the_bound_is_not_an_unsearched_extension() {
        let mut text = String::new();
        let mut tokens = Vec::new();
        for _ in 0..MOST_TOKENS {
            tokens.push(token("あ", "ア", text.len()));
            text.push('あ');
        }
        tokens.push(token("。", "", text.len()));
        text.push('。');
        let found = candidates(&analysis(&text, tokens), &[]).expect("valid baseline");
        assert!(
            found.unsearched.is_empty(),
            "the ninth token is punctuation, so nothing admissible went unexamined: {:?}",
            found.unsearched
        );
    }

    /// …and an admissible ninth token *is* an unanswered question, scoped to where it applies.
    #[test]
    fn an_admissible_extension_past_the_bound_is_recorded() {
        let mut text = String::new();
        let mut tokens = Vec::new();
        for _ in 0..=MOST_TOKENS {
            tokens.push(token("あ", "ア", text.len()));
            text.push('あ');
        }
        let found = candidates(&analysis(&text, tokens), &[]).expect("valid baseline");
        assert!(
            !found.unsearched.is_empty(),
            "a ninth joinable token was never examined"
        );
        let gap = &found.unsearched[0];
        assert_eq!(gap.reason, "stopped at the token bound");
        assert!(found.incomplete_over(gap.from, gap.first_unsearched.1));
        assert!(
            !found.incomplete_over(10_000, 10_100),
            "incompleteness does not suppress decisions elsewhere"
        );
    }

    /// An invalid baseline is an error, not a quietly smaller search.
    #[test]
    fn an_invalid_baseline_is_refused() {
        let mut loose = analysis("ああ", vec![token("あ", "ア", 0), token("あ", "ア", 3)]);
        loose.tokens[1].start = None;
        loose.tokens[1].end = None;
        let faults = candidates(&loose, &[]).expect_err("an ungrounded token fails the contract");
        assert!(
            faults
                .iter()
                .any(|fault| matches!(fault, SpanFault::Ungrounded { .. })),
            "{faults:?}"
        );
    }

    /// A run with a readingless constituent has no established reading.
    #[test]
    fn a_missing_constituent_reading_is_not_concatenated_away() {
        let with = [token("かっこ", "カッコ", 0), token("いい", "イイ", 9)];
        assert_eq!(
            run_reading(&with),
            Reading::Established("カッコイイ".into())
        );
        let without = [token("かっこ", "カッコ", 0), token("いい", "", 9)];
        assert_eq!(run_reading(&without), Reading::Incomplete);
    }

    /// **The prefix case.** An unexplored nine-token reading competes with a grouping over its
    /// first two tokens, so that grouping is not complete — checking only the ninth token's bytes
    /// said it was.
    #[test]
    fn an_unexplored_interpretation_covers_the_prefix_it_would_span() {
        let mut text = String::new();
        let mut tokens = Vec::new();
        for _ in 0..=MOST_TOKENS {
            tokens.push(token("あ", "ア", text.len()));
            text.push('あ');
        }
        let found = candidates(&analysis(&text, tokens), &[]).expect("valid baseline");
        let gap = found
            .unsearched
            .first()
            .expect("a ninth joinable token was never examined");

        // The first two tokens: a grouping there competes with the unexplored longer reading.
        assert!(
            found.incomplete_over(0, 6),
            "the prefix of the unexplored span is not complete: {gap:?}"
        );
        // And the extension itself, obviously.
        assert!(found.incomplete_over(gap.first_unsearched.0, gap.first_unsearched.1));
        // An unrelated clause is unaffected.
        assert!(
            !found.incomplete_over(10_000, 10_100),
            "incompleteness stays scoped to the region it affects"
        );
    }

    /// Eight tokens, a space, then a word: the ninth cannot extend the span, so this is a refusal.
    #[test]
    fn a_ninth_token_past_a_space_is_a_refusal_not_a_budget_failure() {
        let mut text = String::new();
        let mut tokens = Vec::new();
        for _ in 0..MOST_TOKENS {
            tokens.push(token("あ", "ア", text.len()));
            text.push('あ');
        }
        text.push(' ');
        tokens.push(token("い", "イ", text.len()));
        text.push('い');

        let found = candidates(&analysis(&text, tokens), &[]).expect("valid baseline");
        assert!(
            found.unsearched.is_empty(),
            "the ninth token is past a space and cannot extend the span: {:?}",
            found.unsearched
        );
        assert!(
            found
                .refused
                .iter()
                .any(|refusal| refusal.reason.contains("cannot extend")),
            "and that is recorded as an answer: {:?}",
            found.refused
        );
    }

    /// **One sequence, two readings, two interpretations.**
    ///
    /// お父さん is filed at 1002590 with both おとうさん and おとっさん. Grouping on entry identity
    /// alone put them in one candidate, which is a disagreement destroyed rather than reported.
    #[test]
    fn one_sequence_with_two_readings_is_two_interpretations() {
        use crate::dict::index::MatchKind;
        use crate::dict::merge::Retrieved;
        use crate::lexical::candidate::{AnswerKind, EntryId};

        let record = |reading: &str, locator: &str| EntryEvidence {
            id: EntryId {
                dictionary: DictionaryId {
                    title: "Jitendex".into(),
                    installed_as: "jitendex".into(),
                    generation: "g1".into(),
                },
                sequence: Some(1002590),
                locator: locator.into(),
                same_word_as: None,
            },
            term: "お父さん".into(),
            reading: reading.into(),
            kind: AnswerKind::OrdinaryDefinition,
            matched: MatchKind::Reading,
            retrieved: Retrieved::Whole {
                query: "お父さん".into(),
            },
            rules: Vec::new(),
            tags: Vec::new(),
            sense_tags: Vec::new(),
        };

        assert_ne!(
            interpretation_key(&record("おとうさん", "d1")),
            interpretation_key(&record("おとっさん", "d2")),
            "two readings under one sequence are two interpretations"
        );
        assert_eq!(
            interpretation_key(&record("おとうさん", "d1")),
            interpretation_key(&record("オトウサン", "d3")),
            "and the same reading in another script is the same one"
        );
    }

    /// **The requirement §2 rests on.** Same sequence, same reading, incompatible classes.
    ///
    /// **Sense classes partition too, and not by accident of order.**
    ///
    /// 格好いい's `rules` are empty, so rules alone cannot separate it from a record that shares
    /// its sequence and reading while its senses call it something else. Selection keys claims on
    /// `class_evidence`; generation has to pool on the same thing, or the pooled candidate's class
    /// is whichever record the dictionary returned first.
    #[test]
    fn records_with_different_sense_classes_are_different_interpretations() {
        use crate::lexical::candidate::{AnswerKind, EntryId};

        // One sequence, one reading, empty rules — separated only by what the senses say.
        let sensed = |senses: &[&str], locator: &str| EntryEvidence {
            id: EntryId {
                dictionary: DictionaryId {
                    title: "Fixture".into(),
                    installed_as: "fixture".into(),
                    generation: "g1".into(),
                },
                sequence: Some(1_577_490),
                locator: locator.into(),
                same_word_as: None,
            },
            term: "格好いい".into(),
            reading: "かっこいい".into(),
            kind: AnswerKind::OrdinaryDefinition,
            matched: crate::dict::index::MatchKind::Spelling,
            retrieved: Retrieved::Whole {
                query: "かっこいい".into(),
            },
            rules: Vec::new(),
            tags: vec!["★".into(), "priority".into(), "form".into()],
            sense_tags: senses.iter().map(|tag| (*tag).to_string()).collect(),
        };

        let adjectival = sensed(&["adj-i"], "d1");
        let nominal = sensed(&["n"], "d2");
        assert_ne!(
            interpretation_key(&adjectival),
            interpretation_key(&nominal),
            "empty rules must not pool two records their senses separate"
        );

        // **Order-independent.** The keys are a set, and reversing the order the records arrive in
        // produces the same set.
        let forward: std::collections::BTreeSet<_> = [&adjectival, &nominal]
            .iter()
            .map(|record| interpretation_key(record))
            .collect();
        let reversed: std::collections::BTreeSet<_> = [&nominal, &adjectival]
            .iter()
            .map(|record| interpretation_key(record))
            .collect();
        assert_eq!(forward, reversed);
        assert_eq!(forward.len(), 2);

        // And records whose senses agree still pool, including across the two spellings of one
        // class — abstaining between `adj-i` and `adj-ix` would be abstaining on nothing.
        assert_eq!(
            interpretation_key(&sensed(&["adj-i"], "d1")),
            interpretation_key(&sensed(&["adj-ix"], "d1"))
        );
    }

    /// Pooled, the `v5` record would lend the `adj-i` one its rules, and an inflection proposal
    /// about the adjective would be validated against a verb's evidence.
    #[test]
    fn records_with_incompatible_classes_cannot_lend_each_other_rules() {
        use crate::dict::index::MatchKind;
        use crate::dict::merge::Retrieved;
        use crate::lexical::candidate::{AnswerKind, ClassAgreement, EntryId};

        let record = |rules: &[&str], locator: &str| EntryEvidence {
            id: EntryId {
                dictionary: DictionaryId {
                    title: "Fixture".into(),
                    installed_as: "fixture".into(),
                    generation: "g1".into(),
                },
                sequence: Some(4242),
                locator: locator.into(),
                same_word_as: None,
            },
            term: "かかる".into(),
            reading: "かかる".into(),
            kind: AnswerKind::OrdinaryDefinition,
            matched: MatchKind::Spelling,
            retrieved: Retrieved::Whole {
                query: "かかる".into(),
            },
            rules: rules.iter().map(|rule| (*rule).to_string()).collect(),
            tags: Vec::new(),
            sense_tags: Vec::new(),
        };

        let verb = record(&["v5r"], "d1");
        let adjective = record(&["adj-i"], "d2");
        assert_ne!(
            interpretation_key(&verb),
            interpretation_key(&adjective),
            "one sequence, one reading, two classes — two interpretations"
        );
        // The same class in another order or case is still the same interpretation.
        assert_eq!(
            interpretation_key(&record(&["v5r"], "d1")),
            interpretation_key(&record(&["V5R"], "d1"))
        );

        // And each record answers about its own class only.
        assert_eq!(verb.agrees_with_class("v5r"), ClassAgreement::Compatible);
        assert_eq!(verb.agrees_with_class("v5"), ClassAgreement::Compatible);
        assert_eq!(
            verb.agrees_with_class("adj-i"),
            ClassAgreement::Incompatible
        );
        assert_eq!(
            adjective.agrees_with_class("adj-i"),
            ClassAgreement::Compatible
        );
        assert_eq!(
            adjective.agrees_with_class("v5r"),
            ClassAgreement::Incompatible
        );

        // A record asserting nothing says nothing — the third outcome, not a permissive second.
        assert_eq!(
            record(&[], "d3").agrees_with_class("adj-i"),
            ClassAgreement::Absent
        );
        assert!(record(&[], "d3").grammar_class().is_empty());
    }

    /// A baseline token's lemma reading is its own, not its surface.
    #[test]
    fn an_inflected_token_keeps_its_own_lemma_reading() {
        let mut inflected = token("伝わって", "ツタワッテ", 0);
        inflected.dict_form = "伝わる".into();
        inflected.lemma_reading = "ツタワル".into();
        let found = candidates(&analysis("伝わって", vec![inflected]), &[]).expect("valid");
        let candidate = &found.candidates[0];
        assert_eq!(
            candidate.surface_reading.established(),
            Some("ツタワッテ"),
            "the surface is read as it appears"
        );
        assert_eq!(
            candidate.lemma_reading.established(),
            Some("ツタワル"),
            "and the lemma has its own reading, which is a different question"
        );
    }

    #[test]
    fn generation_does_not_touch_the_baseline() {
        let source = analysis(
            "かっこいい",
            vec![token("かっこ", "カッコ", 0), token("いい", "イイ", 9)],
        );
        let before = format!("{source:?}");
        let found = candidates(&source, &[]).expect("valid baseline");
        assert_eq!(before, format!("{source:?}"));
        assert_eq!(found.candidates.len(), 2);
        assert!(found.groupings().next().is_none());
    }
}
