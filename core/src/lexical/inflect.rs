//! Recovering a dictionary form from an inflected one, and refusing to guess.
//!
//! # Why this is not `grammar::formation`
//!
//! That module explains *how a known word builds a known form*, works on the kana reading, and
//! "never fabricates a spelling" (`project notes` §5). It answers a different question in a different
//! direction. Repurposing it as a deinflector would mean asking it to invent okurigana, which is
//! the thing it exists to refuse.
//!
//! # Spelling and reading are separate questions
//!
//! The source says `かっこよかった`. The dictionary headword is **格好いい**, reached by reading.
//! A transformation checked by string equality against the dictionary form would reject the very
//! case this exists to fix, because you cannot reach kanji from kana by trimming suffixes.
//!
//! So two checks, kept apart:
//!
//! 1. **Spelling** — the exact source is preserved and never rewritten. What is produced is a
//!    *proposal about the reading*, plus the entry that reading reached.
//! 2. **Morphology** — the transformation is validated against the **reading**, or against a
//!    spelling relationship the dictionary itself records. Reading agreement licenses a claim
//!    about the reading and nothing about how the word is written.
//!
//! # Three classes have to agree, not two
//!
//! The transformation asserts a class of its own: everything here reverses **i-adjective**
//! inflection. So a proposal needs consistency among
//!
//! - the **transformation's** class ([`TRANSFORMATION_CLASS`]),
//! - the **analyzer's** class for the token, where it established one, and
//! - the **entry's** class, where the record asserts one.
//!
//! Comparing only the last two would let two agreeing *verb* classes validate an adjective-shaped
//! suffix operation: 働く ends in `く` and `はたら` + `い` reaches nothing, but 鳴く, 泣く and
//! 亡く share a reading, and `v5k` agreeing with `v5k` says nothing about whether `なく` is the
//! adverbial of an adjective. Contradiction rejects; unknown leaves the candidate unresolved.
//!
//! **A matching reading is never evidence of a class.** Where neither the analyzer nor the record
//! asserts one, the outcome is [`Recovered::Unresolved`] — the interpretations are kept for
//! assistance, and nothing may select them automatically.
//!
//! # `いい` is not a regular i-adjective
//!
//! It inflects through the suppletive `よい` stem: かっこよかった, かっこよくない, かっこよく. A
//! generic "drop い, append the suffix" produces かっこいかった, which is not a word — so the
//! relationship is a typed case rather than a rule applied to every final `いい`.
//!
//! And it must not be applied to every final `いい`: **かわいい inflects regularly**, giving
//! かわいかった. 可愛い carries `adj-i` explicitly, so it is the control that cannot hide behind
//! the absent-rules path. `かわよい` is a real entry of its own (Jitendex 2845536, `adj-i`), so
//! the control is **"かわよかった does not reach かわいい"** — not "かわよかった is not a word".
//! The suppletive headword is never inferred from the shape of the stem; it is a variant the
//! dictionary records for the same word, and the record that establishes it is kept.
//!
//! **What a shared sequence settles, and what it does not.** It settles that `いい` and `よい`
//! here are one word written two ways, so proposing the `いい` headword is not choosing between
//! two words. It settles nothing about which of that word's *senses* applies in a sentence —
//! that is a contextual question, and nothing in this module asks it.

use std::collections::BTreeMap;

use super::candidate::{classes_agree, ClassAgreement, EntryEvidence};
use crate::dict::index::Dictionary;
use crate::kana::to_hiragana;

/// The class the transformations in this module assert. Everything here reverses i-adjective
/// inflection, so a token the analyzer calls a godan verb is contradicting the operation itself.
pub const TRANSFORMATION_CLASS: &str = "adj-i";

/// An i-adjective form, named by what it does rather than by its ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjectiveForm {
    /// かっこよかった, かわいかった
    Past,
    /// かっこよくない, かわいくない
    Negative,
    /// かっこよくなかった, かわいくなかった
    PastNegative,
    /// かっこよく, かわいく
    Adverbial,
    /// かっこよければ, かわいければ
    Conditional,
}

impl AdjectiveForm {
    pub fn name(self) -> &'static str {
        match self {
            AdjectiveForm::Past => "past",
            AdjectiveForm::Negative => "negative",
            AdjectiveForm::PastNegative => "past negative",
            AdjectiveForm::Adverbial => "adverbial",
            AdjectiveForm::Conditional => "conditional",
        }
    }

    /// The reading suffix this form replaces the stem's final `い` with.
    fn suffix(self) -> &'static str {
        match self {
            AdjectiveForm::Past => "かった",
            AdjectiveForm::Negative => "くない",
            AdjectiveForm::PastNegative => "くなかった",
            AdjectiveForm::Adverbial => "く",
            AdjectiveForm::Conditional => "ければ",
        }
    }

    /// Longest first, so かっこよくなかった is read as a past negative and not as a negative with
    /// something after it.
    fn all() -> [AdjectiveForm; 5] {
        [
            AdjectiveForm::PastNegative,
            AdjectiveForm::Negative,
            AdjectiveForm::Conditional,
            AdjectiveForm::Past,
            AdjectiveForm::Adverbial,
        ]
    }
}

/// A dictionary that could not be consulted.
///
/// **Not an absence.** A dictionary that failed to answer has not said there is no such word, and
/// treating the two alike would let a corrupted index look like a clean negative — or make a
/// surviving dictionary's answer look like the only one there is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupFailure {
    /// The installed directory name, which is the dictionary's identity on disk.
    pub dictionary: String,
    /// The reading that was being looked up when it failed.
    pub reading: String,
    pub error: String,
}

/// What a reading lookup produced, keeping failure distinct from absence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadingHits {
    pub entries: Vec<EntryEvidence>,
    pub failures: Vec<LookupFailure>,
}

impl ReadingHits {
    /// Everything answered, nothing failed — the ordinary case, and the only one in unit tests
    /// that supply their own records.
    pub fn found(entries: Vec<EntryEvidence>) -> ReadingHits {
        ReadingHits {
            entries,
            failures: Vec::new(),
        }
    }
}

/// The result of a recovery attempt, with the dictionaries that could not be reached.
///
/// `failures` is carried beside every outcome rather than only the negative ones: a proposal
/// reached while one dictionary was unreadable is an **incomplete** answer even though it is a
/// real one, and a caller that compares candidate sets has to know that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    pub outcome: Recovered,
    pub failures: Vec<LookupFailure>,
}

impl Recovery {
    /// Whether every dictionary answered. A `false` here qualifies whatever `outcome` says.
    pub fn complete(&self) -> bool {
        self.failures.is_empty()
    }
}

/// What reversing an inflection produced, or why it produced nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovered {
    /// A dictionary form was proposed, entries support it, and something other than the matching
    /// reading establishes that this is an i-adjective.
    ///
    /// **Every supporting interpretation is here**, not the first one found. One lemma reading can
    /// be several words, and which word it is belongs to selection — so recovery hands them all
    /// over rather than letting dictionary order choose.
    Proposal(LemmaProposals),
    /// Entries support the proposal, but **no independent grammatical evidence does**: the
    /// analyzer established no class and the records assert none, so all that is known is that a
    /// word with that reading exists. Kept as alternatives; never selected automatically.
    Unresolved(LemmaProposals),
    /// A form was recognised but more than one **dictionary form** is consistent with it, each
    /// with its own supporting records.
    Ambiguous(Vec<LemmaProposals>),
    /// The analyzer's own class contradicts the transformation: the surface has the shape, and
    /// the analysis says it is not an adjective. A rejection, not an open question.
    Contradicted {
        form: AdjectiveForm,
        analyzer_class: String,
        transformation_class: &'static str,
    },
    /// The surface is not a form this module supports. **Not a failure** — most words are not
    /// inflected i-adjectives, and saying so is the ordinary answer.
    Unsupported,
    /// A form was recognised and no entry supports the proposal it implies, **and every
    /// dictionary answered**.
    Unvalidated { proposed_reading: String },
    /// A form was recognised, nothing supported it, and at least one dictionary could not be
    /// read. Distinct from [`Recovered::Unvalidated`] because nothing was established.
    Unavailable { proposed_reading: String },
}

/// Every interpretation supporting one dictionary-form reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LemmaProposals {
    /// The dictionary-form **reading** these share. Never a spelling.
    pub lemma_reading: String,
    /// The supporting interpretations, in a deterministic order that does not depend on the order
    /// the dictionaries were consulted in. Never empty.
    pub interpretations: Vec<Proposal>,
}

impl LemmaProposals {
    /// Whether any interpretation rests on evidence other than the matching reading.
    pub fn independently_grounded(&self) -> bool {
        self.interpretations
            .iter()
            .any(Proposal::independently_grounded)
    }

    /// The identities of the supporting records, for comparing two runs' interpretation sets.
    pub fn identities(&self) -> Vec<(String, Option<i64>, String)> {
        self.interpretations
            .iter()
            .map(|proposal| proposal.entry.id.clone())
            .map(|id| (id.dictionary.installed_as, id.sequence, id.locator))
            .collect()
    }
}

/// One validated dictionary-form proposal, and the record it rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The form recognised in the source.
    pub form: AdjectiveForm,
    /// The reading the source has, unchanged.
    pub surface_reading: String,
    /// The dictionary-form **reading** this proposes. Never a spelling.
    pub lemma_reading: String,
    /// Whether the suppletive `いい`/`よい` headword was used. Equivalent to `variant.is_some()`,
    /// and kept because it is what the case is called.
    pub suppletive: bool,
    /// Each step, for explanation only. Never the evidence a selector reads.
    pub steps: Vec<String>,
    /// How the supporting entry's asserted class compared with the **analyzer's**.
    pub agreement: ClassAgreement,
    /// How the **analyzer's** class compared with the class the transformation asserts. This is
    /// what carries the acceptance family, whose records assert nothing.
    pub analyzer_supports_transformation: ClassAgreement,
    /// How the **entry's** class compared with the class the transformation asserts. This is what
    /// carries a token the analyzer could say nothing about.
    pub entry_supports_transformation: ClassAgreement,
    /// The entry that supports the proposal.
    pub entry: EntryEvidence,
    /// The sibling record establishing the suppletive headword reading, where one did, together
    /// with what ties the two records to one word. **Both records are kept**: the justification
    /// for changing the lemma reading is evidence, not commentary.
    pub variant: Option<VariantEvidence>,
}

impl Proposal {
    /// Whether something other than the matching reading establishes the i-adjective class.
    ///
    /// A reading match establishes a possible word. It does not establish what kind of word, and
    /// a proposal resting on it alone is an alternative rather than an answer.
    pub fn independently_grounded(&self) -> bool {
        self.analyzer_supports_transformation == ClassAgreement::Compatible
            || self.entry_supports_transformation == ClassAgreement::Compatible
    }
}

/// The record that supplies the suppletive headword reading, and how it is the same word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantEvidence {
    /// The sibling record — 格好いい to the 格好良い that the inflected surface reached.
    pub entry: EntryEvidence,
    pub relationship: SameWordRelationship,
}

/// What establishes that two records are one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameWordRelationship {
    /// Both records are filed under one dictionary sequence.
    Sequence(i64),
    /// One record points at the other, and cross-reference resolution established the target.
    CrossReference(i64),
}

impl SameWordRelationship {
    pub fn name(self) -> &'static str {
        match self {
            SameWordRelationship::Sequence(_) => "same sequence",
            SameWordRelationship::CrossReference(_) => "resolved cross-reference",
        }
    }

    pub fn target(self) -> i64 {
        match self {
            SameWordRelationship::Sequence(sequence)
            | SameWordRelationship::CrossReference(sequence) => sequence,
        }
    }
}

/// The evidence a caller must supply about the token itself.
///
/// **Independent of the dictionary.** Where an entry asserts no class — 格好いい asserts none —
/// this is what has to carry the grammatical claim instead, and a reading match alone will not do.
#[derive(Debug, Clone)]
pub struct TokenMorphology<'a> {
    /// The surface reading the analyzer established.
    pub reading: &'a str,
    /// The class the analyzer established, in the shared code space (`adj-i`, `v5r`, …).
    pub class: Option<&'a str>,
}

impl<'a> TokenMorphology<'a> {
    /// Read the morphology the analyzer **actually supplied** for a token.
    ///
    /// Nothing is filled in and nothing is inferred from the surface. Where the analyzer
    /// established no class this reports none, and recovery then has no independent grammatical
    /// evidence to rest on — which is the honest outcome, not a gap to paper over.
    ///
    /// The class comes from the base transformation's `pos`, which is where the analyzer records
    /// the normalized conjugation class it deduced for the lemma.
    pub fn of_token(token: &'a crate::model::Token) -> TokenMorphology<'a> {
        let class = token
            .conjugation
            .as_ref()
            .and_then(|conjugation| {
                conjugation
                    .props
                    .iter()
                    .find(|prop| prop.is_base)
                    .or_else(|| conjugation.props.last())
            })
            .map(|prop| prop.pos.trim())
            .filter(|pos| !pos.is_empty());
        TokenMorphology {
            reading: token.reading.as_str(),
            class,
        }
    }
}

/// Propose a dictionary form for an inflected i-adjective reading, and validate it.
///
/// `lookup` is how a proposed reading is checked against the installed dictionaries; it is passed
/// in so this stays testable without a disk, and it reports **failures separately from absence**.
pub fn recover_adjective(
    morphology: &TokenMorphology<'_>,
    lookup: &mut dyn FnMut(&str) -> ReadingHits,
) -> Recovery {
    let reading = to_hiragana(morphology.reading);
    let analyzer_class = morphology
        .class
        .map(str::trim)
        .filter(|class| !class.is_empty());
    // The transformation's own claim, compared with the analyzer's — before any dictionary is
    // consulted, because no record can reconcile a verb analysis with an adjective operation.
    let analyzer_supports_transformation = match analyzer_class {
        Some(class) => classes_agree(class, TRANSFORMATION_CLASS),
        None => ClassAgreement::Absent,
    };
    let mut failures: Vec<LookupFailure> = Vec::new();

    for form in AdjectiveForm::all() {
        let Some(stem) = reading.strip_suffix(form.suffix()) else {
            continue;
        };
        if stem.is_empty() {
            continue;
        }

        // The shape matches; now the analysis has to permit an adjective here at all. Checked
        // after the shape so that 食べる, which has no adjective shape, is reported as
        // unsupported rather than as a contradiction.
        if analyzer_supports_transformation == ClassAgreement::Incompatible {
            return Recovery {
                outcome: Recovered::Contradicted {
                    form,
                    analyzer_class: analyzer_class.unwrap_or_default().to_string(),
                    transformation_class: TRANSFORMATION_CLASS,
                },
                failures,
            };
        }

        // **One proposal, from the regular rule.** An earlier version offered a second,
        // suppletive reading whenever the stem ended in よ — and that manufactured かわいい out of
        // かわよかった, which belongs to かわよい. The suppletive headword is not something to
        // guess at from the surface; it is a variant the dictionary either records for this word
        // or does not.
        let regular = format!("{stem}い");
        let hits = lookup(&regular);
        failures.extend(hits.failures);
        let mut validated: Vec<Proposal> = Vec::new();

        for entry in hits.entries {
            if to_hiragana(&entry.reading) != regular {
                continue;
            }
            // A name, a redirect, or an answer about a component cannot support a word here.
            if !entry.supports_lexical_unit() {
                continue;
            }
            let entry_supports_transformation = entry.agrees_with_class(TRANSFORMATION_CLASS);
            if entry_supports_transformation == ClassAgreement::Incompatible {
                continue;
            }
            let agreement = match analyzer_class {
                Some(class) => entry.agrees_with_class(class),
                None => ClassAgreement::Absent,
            };
            if agreement == ClassAgreement::Incompatible {
                continue;
            }

            let mut lemma_reading = regular.clone();
            let mut steps = vec![
                format!(
                    "{reading} = stem {stem} + {} ({})",
                    form.suffix(),
                    form.name()
                ),
                format!("i-adjective: {stem} + い"),
            ];
            let variant = suppletive_variant(
                stem,
                &entry,
                analyzer_class,
                lookup,
                &mut failures,
                &mut steps,
            );
            if let Some(found) = &variant {
                lemma_reading = to_hiragana(&found.entry.reading);
            }

            validated.push(Proposal {
                form,
                surface_reading: reading.clone(),
                lemma_reading,
                suppletive: variant.is_some(),
                steps,
                agreement,
                analyzer_supports_transformation,
                entry_supports_transformation,
                entry,
                variant,
            });
        }

        if validated.is_empty() {
            // **A dictionary that failed is not a dictionary that said no.**
            let outcome = if failures.is_empty() {
                Recovered::Unvalidated {
                    proposed_reading: regular,
                }
            } else {
                Recovered::Unavailable {
                    proposed_reading: regular,
                }
            };
            return Recovery { outcome, failures };
        }

        let mut groups = group_by_lemma(validated);
        let outcome = if groups.len() > 1 {
            // **More than one dictionary form is consistent with this surface.** Choosing would
            // be guessing, so every reading is returned with the records behind it.
            Recovered::Ambiguous(groups)
        } else {
            let sole = groups.remove(0);
            if sole.independently_grounded() {
                Recovered::Proposal(sole)
            } else {
                Recovered::Unresolved(sole)
            }
        };
        return Recovery { outcome, failures };
    }

    Recovery {
        outcome: Recovered::Unsupported,
        failures,
    }
}

/// Find the suppletive headword the dictionary records **for this same word**, if any.
///
/// The sibling is held to the same standards as the entry that reached it: it must be an ordinary
/// whole-word definition, and its class must contradict neither the transformation nor the
/// analyzer. A variant that fails those is not evidence about the headword reading.
fn suppletive_variant(
    stem: &str,
    entry: &EntryEvidence,
    analyzer_class: Option<&str>,
    lookup: &mut dyn FnMut(&str) -> ReadingHits,
    failures: &mut Vec<LookupFailure>,
    steps: &mut Vec<String>,
) -> Option<VariantEvidence> {
    // 格好良い is filed at 1577490 with reading かっこよい, and 格好いい at the same sequence with
    // かっこいい — so かっこよかった's headword reading is かっこいい, read off the data rather than
    // assumed from the shape of the stem. かわよい's own record has no いい sibling, so かわよかった
    // recovers かわよい and stops there.
    let root = stem.strip_suffix('よ')?;
    let candidate_reading = format!("{root}いい");
    let hits = lookup(&candidate_reading);
    failures.extend(hits.failures);

    let sibling = hits.entries.into_iter().find(|sibling| {
        to_hiragana(&sibling.reading) == candidate_reading
            && sibling.id.same_word_key() == entry.id.same_word_key()
            && sibling.supports_lexical_unit()
            && sibling.agrees_with_class(TRANSFORMATION_CLASS) != ClassAgreement::Incompatible
            && analyzer_class
                .map(|class| sibling.agrees_with_class(class))
                .unwrap_or(ClassAgreement::Absent)
                != ClassAgreement::Incompatible
    })?;

    // Why they are one word, kept as a value rather than left implicit in having matched.
    let relationship = match (entry.id.same_word_as, sibling.id.same_word_as) {
        (Some(target), _) | (_, Some(target)) => SameWordRelationship::CrossReference(target),
        (None, None) => SameWordRelationship::Sequence(entry.id.sequence?),
    };
    steps.push(format!(
        "the same word ({}) is also written {} and read {candidate_reading}",
        relationship.name(),
        sibling.term
    ));
    Some(VariantEvidence {
        entry: sibling,
        relationship,
    })
}

/// Group proposals by the reading they propose, in an order that does not depend on the order the
/// dictionaries answered in.
fn group_by_lemma(validated: Vec<Proposal>) -> Vec<LemmaProposals> {
    let mut grouped: BTreeMap<String, Vec<Proposal>> = BTreeMap::new();
    for proposal in validated {
        grouped
            .entry(proposal.lemma_reading.clone())
            .or_default()
            .push(proposal);
    }
    grouped
        .into_iter()
        .map(|(lemma_reading, mut interpretations)| {
            interpretations.sort_by(|left, right| {
                let key = |proposal: &Proposal| {
                    (
                        proposal.entry.id.dictionary.installed_as.clone(),
                        proposal.entry.id.sequence,
                        proposal.entry.id.locator.clone(),
                    )
                };
                key(left).cmp(&key(right))
            });
            LemmaProposals {
                lemma_reading,
                interpretations,
            }
        })
        .collect()
}

/// Look a reading up across installed dictionaries, for callers that have them.
///
/// A dictionary that fails is **recorded**, never skipped: silently dropping it would turn a
/// broken index into "no such word", and would make whatever the remaining dictionaries said look
/// like the whole answer.
pub fn reading_lookup<'a>(dictionaries: &'a [Dictionary]) -> impl FnMut(&str) -> ReadingHits + 'a {
    reading_lookup_in(crate::dict::evidence::Evidence::direct(dictionaries))
}

/// [`reading_lookup`], reading through one request's evidence.
pub fn reading_lookup_in<'a>(
    dictionaries: crate::dict::evidence::Evidence<'a>,
) -> impl FnMut(&str) -> ReadingHits + 'a {
    move |reading: &str| {
        let mut hits = ReadingHits::default();
        for source in dictionaries.sources() {
            let dictionary = source.dictionary;
            match source.whole_word(reading) {
                Ok(records) => {
                    for record in &records {
                        hits.entries.push(EntryEvidence::read(
                            dictionary,
                            record,
                            crate::dict::merge::Retrieved::Whole {
                                query: reading.to_string(),
                            },
                        ));
                    }
                }
                Err(error) => hits.failures.push(LookupFailure {
                    dictionary: dictionary.installed_as(),
                    reading: reading.to_string(),
                    error: error.to_string(),
                }),
            }
        }
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::index::MatchKind;
    use crate::dict::merge::Retrieved;
    use crate::lexical::candidate::{AnswerKind, DictionaryId, EntryId};

    fn record(term: &str, reading: &str, rules: &[&str]) -> EntryEvidence {
        record_at(term, reading, rules, 1)
    }

    fn record_at(term: &str, reading: &str, rules: &[&str], sequence: i64) -> EntryEvidence {
        record_in("fixture", term, reading, rules, sequence)
    }

    fn record_in(
        dictionary: &str,
        term: &str,
        reading: &str,
        rules: &[&str],
        sequence: i64,
    ) -> EntryEvidence {
        EntryEvidence {
            id: EntryId {
                dictionary: DictionaryId {
                    title: "Fixture".into(),
                    installed_as: dictionary.into(),
                    generation: "g1".into(),
                },
                sequence: Some(sequence),
                locator: format!("{term}/{reading}"),
                same_word_as: None,
            },
            term: term.into(),
            reading: reading.into(),
            kind: AnswerKind::OrdinaryDefinition,
            matched: MatchKind::Reading,
            retrieved: Retrieved::Whole {
                query: reading.into(),
            },
            rules: rules.iter().map(|rule| (*rule).to_string()).collect(),
            tags: Vec::new(),
            sense_tags: Vec::new(),
        }
    }

    /// A dictionary holding exactly the records named, answered by reading.
    fn dictionary(records: Vec<EntryEvidence>) -> impl FnMut(&str) -> ReadingHits {
        move |reading: &str| {
            ReadingHits::found(
                records
                    .iter()
                    .filter(|record| to_hiragana(&record.reading) == reading)
                    .cloned()
                    .collect(),
            )
        }
    }

    /// A dictionary that answers for some readings and fails for others.
    fn dictionary_failing_on(
        records: Vec<EntryEvidence>,
        broken: &'static str,
    ) -> impl FnMut(&str) -> ReadingHits {
        move |reading: &str| {
            if reading == broken {
                return ReadingHits {
                    entries: Vec::new(),
                    failures: vec![LookupFailure {
                        dictionary: "broken".into(),
                        reading: reading.into(),
                        error: "index is corrupt".into(),
                    }],
                };
            }
            ReadingHits::found(
                records
                    .iter()
                    .filter(|record| to_hiragana(&record.reading) == reading)
                    .cloned()
                    .collect(),
            )
        }
    }

    fn sole(outcome: &Recovered) -> &Proposal {
        let Recovered::Proposal(lemma) = outcome else {
            panic!("expected a proposal, got {outcome:?}");
        };
        assert_eq!(
            lemma.interpretations.len(),
            1,
            "expected exactly one interpretation: {lemma:?}"
        );
        &lemma.interpretations[0]
    }

    /// **The acceptance family.** 格好いい asserts no class, so the analyzer's must carry it.
    #[test]
    fn the_suppletive_stem_recovers_the_acceptance_family() {
        for (surface, form) in [
            ("かっこよかった", AdjectiveForm::Past),
            ("かっこよくない", AdjectiveForm::Negative),
            ("かっこよくなかった", AdjectiveForm::PastNegative),
            ("かっこよく", AdjectiveForm::Adverbial),
            ("かっこよければ", AdjectiveForm::Conditional),
        ] {
            // As Jitendex files it: 格好いい and 格好良い under one sequence, with readings
            // かっこいい and かっこよい. The よい reading is what the inflected surface reaches;
            // the いい reading is the headword, and the two are one word.
            let mut look = dictionary(vec![
                record_at("格好良い", "かっこよい", &[], 1577490),
                record_at("格好いい", "かっこいい", &[], 1577490),
            ]);
            let recovery = recover_adjective(
                &TokenMorphology {
                    reading: surface,
                    class: Some("adj-i"),
                },
                &mut look,
            );
            assert!(recovery.complete());
            let proposal = sole(&recovery.outcome);
            assert_eq!(proposal.form, form);
            assert_eq!(proposal.lemma_reading, "かっこいい");
            assert!(
                proposal.suppletive,
                "{surface} reaches かっこよい, and the same word's headword reading is かっこいい"
            );
            // The entry asserts no class, so this is the absent path — not agreement.
            assert_eq!(proposal.agreement, ClassAgreement::Absent);
            assert_eq!(
                proposal.entry_supports_transformation,
                ClassAgreement::Absent
            );
            // …and the analyzer is what grounds it.
            assert_eq!(
                proposal.analyzer_supports_transformation,
                ClassAgreement::Compatible
            );
            assert!(proposal.independently_grounded());
            // The source spelling is never touched; what is proposed is a reading.
            assert_eq!(proposal.surface_reading, surface);

            // **Both records are kept**, with what ties them to one word.
            let variant = proposal
                .variant
                .as_ref()
                .expect("the sibling record justifies the changed lemma reading");
            assert_eq!(proposal.entry.term, "格好良い");
            assert_eq!(variant.entry.term, "格好いい");
            assert_eq!(
                variant.relationship,
                SameWordRelationship::Sequence(1577490)
            );
        }
    }

    /// **A reading match is not grammatical evidence.** Neither side asserting a class leaves the
    /// interpretation unresolved, however well the reading fits.
    #[test]
    fn a_matching_reading_alone_does_not_ground_a_proposal() {
        let mut look = dictionary(vec![
            record_at("格好良い", "かっこよい", &[], 1577490),
            record_at("格好いい", "かっこいい", &[], 1577490),
        ]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "かっこよかった",
                // The analyzer established nothing — which is what happens when the surface is
                // split and no single token carries the adjective's morphology.
                class: None,
            },
            &mut look,
        );
        let Recovered::Unresolved(lemma) = &recovery.outcome else {
            panic!("expected an unresolved outcome, got {:?}", recovery.outcome);
        };
        assert_eq!(lemma.lemma_reading, "かっこいい");
        assert!(
            !lemma.independently_grounded(),
            "nothing but the reading supports this, and the reading is not evidence of a class"
        );
    }

    /// An entry that asserts the class grounds the interpretation without the analyzer.
    #[test]
    fn an_entry_that_asserts_the_class_grounds_it_on_its_own() {
        let mut look = dictionary(vec![record("高い", "たかい", &["adj-i"])]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "たかかった",
                class: None,
            },
            &mut look,
        );
        let proposal = sole(&recovery.outcome);
        assert_eq!(
            proposal.entry_supports_transformation,
            ClassAgreement::Compatible
        );
        assert_eq!(
            proposal.analyzer_supports_transformation,
            ClassAgreement::Absent
        );
        assert!(proposal.independently_grounded());
    }

    /// **The counterexample.** かわいい is regular: かわいかった, and the suppletive stem is never
    /// applied to it.
    #[test]
    fn a_regular_adjective_is_never_given_the_suppletive_stem() {
        let mut look = dictionary(vec![record("可愛い", "かわいい", &["adj-i"])]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "かわいかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let proposal = sole(&recovery.outcome);
        assert_eq!(proposal.lemma_reading, "かわいい");
        assert!(
            !proposal.suppletive,
            "かわいい inflects regularly and must not go through the よい stem"
        );
        assert!(proposal.variant.is_none());
        assert_eq!(
            proposal.agreement,
            ClassAgreement::Compatible,
            "可愛い asserts adj-i, so this cannot hide behind the absent-rules path"
        );

        // **And かわよかった does not reach かわいい.**
        //
        // The control is the direction, not the existence: かわよい is a real entry (Jitendex
        // 2845536, `adj-i`), so かわよかった is a form of its own word. What must never happen is
        // the suppletive stem carrying it to かわいい, which is what a rule keyed on "the stem
        // ends in よ" did. Here the dictionary holds both words, with no relationship between
        // them, and the recovery must stay with the one the surface belongs to.
        let mut look = dictionary(vec![
            record_at("可愛い", "かわいい", &["adj-i"], 1577200),
            record_at("かわよい", "かわよい", &["adj-i"], 2845536),
        ]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "かわよかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let proposal = sole(&recovery.outcome);
        assert_eq!(
            proposal.lemma_reading, "かわよい",
            "かわよかった is a form of かわよい"
        );
        assert_ne!(proposal.lemma_reading, "かわいい");
        assert!(
            !proposal.suppletive,
            "the two words share no sequence, so nothing records a suppletive relationship"
        );
    }

    /// The same-word test is the **key**, not the reading: a かわいい record filed under a
    /// different word cannot become かわよい's headword however well its reading fits.
    #[test]
    fn a_sibling_of_a_different_word_is_not_a_suppletive_variant() {
        let mut look = dictionary(vec![
            record_at("かわよい", "かわよい", &["adj-i"], 2845536),
            // Same reading as the suppletive form would have, different word entirely.
            record_at("可愛い", "かわいい", &["adj-i"], 1577200),
        ]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "かわよかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let proposal = sole(&recovery.outcome);
        assert_eq!(proposal.lemma_reading, "かわよい");
        assert!(proposal.variant.is_none());
    }

    /// **The sibling is evidence, so it is held to the same standard as the entry.**
    ///
    /// A name, a redirect, or a record whose class contradicts the transformation cannot establish
    /// what the headword reading is, however well its own reading fits and however firmly it is
    /// filed under the same sequence.
    #[test]
    fn the_suppletive_sibling_is_held_to_the_same_standard() {
        // A name filed under the same sequence.
        let mut naming = record_at("格好いい", "かっこいい", &[], 1577490);
        naming.kind = AnswerKind::NameOnly;
        // A record under the same sequence whose class says it is a verb.
        let contradicting = record_at("格好いい", "かっこいい", &["v5r"], 1577490);

        for sibling in [naming, contradicting] {
            let described = format!("{:?}/{:?}", sibling.kind, sibling.rules);
            let mut look = dictionary(vec![
                record_at("格好良い", "かっこよい", &["adj-i"], 1577490),
                sibling,
            ]);
            let recovery = recover_adjective(
                &TokenMorphology {
                    reading: "かっこよかった",
                    class: Some("adj-i"),
                },
                &mut look,
            );
            let proposal = sole(&recovery.outcome);
            assert_eq!(
                proposal.lemma_reading, "かっこよい",
                "{described}: this record cannot establish a headword reading"
            );
            assert!(!proposal.suppletive, "{described}");
            assert!(proposal.variant.is_none(), "{described}");
        }
    }

    /// An ordinary i-adjective, as an independent control on the mechanism.
    #[test]
    fn ordinary_adjectives_go_through_the_same_mechanism() {
        for (surface, lemma) in [
            ("たかかった", "たかい"),
            ("おいしくない", "おいしい"),
            ("さむければ", "さむい"),
        ] {
            let mut look = dictionary(vec![
                record("高い", "たかい", &["adj-i"]),
                record("美味しい", "おいしい", &["adj-i"]),
                record("寒い", "さむい", &["adj-i"]),
            ]);
            let recovery = recover_adjective(
                &TokenMorphology {
                    reading: surface,
                    class: Some("adj-i"),
                },
                &mut look,
            );
            let proposal = sole(&recovery.outcome);
            assert_eq!(proposal.lemma_reading, lemma);
            assert!(!proposal.suppletive);
            assert_eq!(proposal.agreement, ClassAgreement::Compatible);
        }
    }

    /// **A class that disagrees rejects the entry**, rather than being pooled with one that agrees.
    #[test]
    fn an_incompatible_class_cannot_validate_a_proposal() {
        let mut look = dictionary(vec![record("高い", "たかい", &["v5r"])]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "たかかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        assert!(
            matches!(recovery.outcome, Recovered::Unvalidated { .. }),
            "a v5r record cannot validate an i-adjective proposal: {:?}",
            recovery.outcome
        );
    }

    /// **Two agreeing verb classes do not validate an adjective transformation.**
    ///
    /// This is the case class-against-class comparison missed: the analyzer says `v5k`, the record
    /// says `v5k`, they agree perfectly — about a verb. The operation being reversed is an
    /// i-adjective's, and the analysis contradicts it.
    #[test]
    fn agreeing_verb_classes_do_not_license_an_adjective_suffix_operation() {
        let mut look = dictionary(vec![record("鳴く", "なく", &["v5k"])]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "なく",
                class: Some("v5k"),
            },
            &mut look,
        );
        let Recovered::Contradicted {
            form,
            analyzer_class,
            transformation_class,
        } = &recovery.outcome
        else {
            panic!(
                "a godan verb must not validate an adverbial adjective: {:?}",
                recovery.outcome
            );
        };
        assert_eq!(*form, AdjectiveForm::Adverbial);
        assert_eq!(analyzer_class, "v5k");
        assert_eq!(*transformation_class, TRANSFORMATION_CLASS);
    }

    /// An entry whose class contradicts the **transformation** is rejected even when the analyzer
    /// said nothing to contradict it with.
    #[test]
    fn an_entry_contradicting_the_transformation_is_rejected_without_an_analyzer_class() {
        let mut look = dictionary(vec![record("無い", "ない", &["v5r"])]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "なかった",
                class: None,
            },
            &mut look,
        );
        assert!(
            matches!(recovery.outcome, Recovered::Unvalidated { .. }),
            "a v5r record cannot support an i-adjective proposal: {:?}",
            recovery.outcome
        );
    }

    /// A name, a redirect or a component answer cannot support a word.
    #[test]
    fn only_an_ordinary_whole_word_definition_supports_a_proposal() {
        for kind in [
            AnswerKind::NameOnly,
            AnswerKind::UnresolvedPointer,
            AnswerKind::NoDefinition,
        ] {
            let mut entry = record("高い", "たかい", &["adj-i"]);
            entry.kind = kind;
            let mut look = dictionary(vec![entry]);
            let recovery = recover_adjective(
                &TokenMorphology {
                    reading: "たかかった",
                    class: Some("adj-i"),
                },
                &mut look,
            );
            assert!(
                matches!(recovery.outcome, Recovered::Unvalidated { .. }),
                "{kind:?} cannot support a lexical unit: {:?}",
                recovery.outcome
            );
        }

        // And an ordinary definition retrieved for a *component* is about the component.
        let mut entry = record("高い", "たかい", &["adj-i"]);
        entry.retrieved = Retrieved::Component {
            query: "たか".into(),
        };
        let mut look = dictionary(vec![entry]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "たかかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        assert!(matches!(recovery.outcome, Recovered::Unvalidated { .. }));
    }

    /// **Two different lemma readings for one surface is ambiguity, not a choice** — and each
    /// keeps the records behind it.
    #[test]
    fn two_lemma_readings_for_one_surface_are_reported_as_ambiguous() {
        let mut look = dictionary(vec![
            // One word written two ways, so its headword reading is the いい form.
            record_at("格好良い", "かっこよい", &["adj-i"], 1577490),
            record_at("格好いい", "かっこいい", &["adj-i"], 1577490),
            // A different word sharing the よい reading, with no いい variant of its own.
            record_at("恰好良い", "かっこよい", &["adj-i"], 9999),
        ]);
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "かっこよかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let Recovered::Ambiguous(readings) = &recovery.outcome else {
            panic!("expected ambiguity, got {:?}", recovery.outcome);
        };
        assert_eq!(
            readings
                .iter()
                .map(|group| group.lemma_reading.as_str())
                .collect::<Vec<_>>(),
            vec!["かっこいい", "かっこよい"],
            "two headword readings, and choosing between them would be guessing"
        );
        // **The evidence survives**, rather than only the reading strings.
        assert_eq!(readings[0].interpretations[0].entry.term, "格好良い");
        assert_eq!(
            readings[0].interpretations[0]
                .variant
                .as_ref()
                .expect("the いい sibling")
                .entry
                .term,
            "格好いい"
        );
        assert_eq!(readings[1].interpretations[0].entry.term, "恰好良い");
        assert!(readings[1].interpretations[0].variant.is_none());
    }

    /// **Homographs are several interpretations of one answer, and all of them survive.**
    ///
    /// Three words sharing one lemma reading give one answer to "what is the dictionary form's
    /// reading"; which word it is belongs to selection — so selection has to be given all three.
    #[test]
    fn several_words_sharing_a_lemma_reading_all_survive() {
        let records = vec![
            record_in("alpha", "甲い", "こうい", &["adj-i"], 10),
            record_in("beta", "乙い", "こうい", &["adj-i"], 20),
            record_in("gamma", "丙い", "こうい", &["adj-i"], 30),
        ];
        let mut look = dictionary(records.clone());
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "こうかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let Recovered::Proposal(lemma) = &recovery.outcome else {
            panic!("expected one lemma reading, got {:?}", recovery.outcome);
        };
        assert_eq!(lemma.lemma_reading, "こうい");
        assert_eq!(
            lemma
                .interpretations
                .iter()
                .map(|proposal| proposal.entry.term.as_str())
                .collect::<Vec<_>>(),
            vec!["甲い", "乙い", "丙い"],
            "every supporting word survives; dictionary order does not choose one"
        );

        // **Reversing the order the dictionaries answer in changes nothing.** Before, recovery
        // returned `validated.remove(0)` and whichever dictionary happened to answer first became
        // the answer.
        let mut reversed = records;
        reversed.reverse();
        let mut look = dictionary(reversed);
        let other = recover_adjective(
            &TokenMorphology {
                reading: "こうかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let Recovered::Proposal(mirrored) = &other.outcome else {
            panic!("expected one lemma reading, got {:?}", other.outcome);
        };
        assert_eq!(
            lemma.identities(),
            mirrored.identities(),
            "the interpretation set must not depend on lookup order"
        );
    }

    /// Most words are not inflected i-adjectives, and saying so is the ordinary answer.
    #[test]
    fn an_unsupported_form_is_not_a_failure() {
        let mut look = dictionary(Vec::new());
        assert_eq!(
            recover_adjective(
                &TokenMorphology {
                    reading: "たべる",
                    class: Some("v1"),
                },
                &mut look
            )
            .outcome,
            Recovered::Unsupported
        );
    }

    /// A reading that reaches no entry is unvalidated, and no lemma is manufactured.
    #[test]
    fn nothing_is_proposed_without_an_entry_behind_it() {
        let mut look = dictionary(Vec::new());
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "ぬるかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let Recovered::Unvalidated { proposed_reading } = &recovery.outcome else {
            panic!(
                "expected an unvalidated outcome, got {:?}",
                recovery.outcome
            );
        };
        assert!(proposed_reading.contains("ぬるい"));
    }

    /// **A dictionary that failed has not said there is no such word.**
    #[test]
    fn a_failed_lookup_is_not_an_absent_entry() {
        let mut look = dictionary_failing_on(Vec::new(), "ぬるい");
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "ぬるかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let Recovered::Unavailable { proposed_reading } = &recovery.outcome else {
            panic!(
                "a corrupt index must not read as an absent entry: {:?}",
                recovery.outcome
            );
        };
        assert_eq!(proposed_reading, "ぬるい");
        assert!(!recovery.complete());
        assert_eq!(recovery.failures.len(), 1);
        assert_eq!(recovery.failures[0].reading, "ぬるい");
    }

    /// A failure while looking for the **sibling** is carried too: the proposal is real, and the
    /// answer is incomplete.
    #[test]
    fn a_failure_while_seeking_the_variant_is_reported_with_the_proposal() {
        let mut look = dictionary_failing_on(
            vec![record_at("格好良い", "かっこよい", &["adj-i"], 1577490)],
            "かっこいい",
        );
        let recovery = recover_adjective(
            &TokenMorphology {
                reading: "かっこよかった",
                class: Some("adj-i"),
            },
            &mut look,
        );
        let proposal = sole(&recovery.outcome);
        // Without the sibling the headword reading stays the regular one — and the caller is told
        // the search for a variant did not complete, so this is not evidence there is none.
        assert_eq!(proposal.lemma_reading, "かっこよい");
        assert!(!proposal.suppletive);
        assert!(
            !recovery.complete(),
            "the variant lookup failed, so the absence of a variant is not established"
        );
        assert_eq!(recovery.failures[0].reading, "かっこいい");
    }
}
