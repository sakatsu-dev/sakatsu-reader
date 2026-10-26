//! What a span *could* be, with evidence kept in the categories that actually differ.
//!
//! # Three things that are not one thing
//!
//! An earlier version of this file asked one question — was the query the whole token? — and
//! treated the answer as licence to group. It is not. Three properties vary independently:
//!
//! | | What it says | What it cannot say |
//! |---|---|---|
//! | **Query scope** ([`Retrieved`]) | the lookup asked about the whole token, or one component | whether the answer means anything |
//! | **Answer kind** ([`AnswerKind`]) | an ordinary definition, a name, a pointer, or nothing | what was asked |
//! | **Match route** ([`MatchKind`]) | spelling, reading, a fold, a する-verb | either of the above |
//!
//! A JMnedict hit retrieved `Whole` is a whole-query answer saying only that somebody is called
//! this. A resolved-to-nothing cross-reference retrieved `Whole` is a whole-query answer saying
//! where the meaning would be. Neither supports calling the span a word, and a check on scope alone
//! passed both.
//!
//! # Identity, and why a title is not one
//!
//! Two installed dictionaries can carry the same `index.json` title, and a sequence number means
//! different things in different dictionaries — Jitendex files a search-only spelling under the
//! *negated* sequence of its target, which is a convention of that data and not a general rule. So
//! an entry is addressed by **installation and borrowed generation**, plus the sequence where its
//! meaning is established and a term/reading locator where it is not. `abs()` is applied only where
//! the dictionary's own redirect convention has been confirmed for that dictionary.

use crate::dict::index::{Dictionary, MatchKind};
use crate::dict::merge::Retrieved;
use crate::dict::yomitan::Entry;

/// Which dictionary, which installed copy, which generation of it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DictionaryId {
    /// For showing a reader. **Not an identity** — two installations may share it.
    pub title: String,
    /// The directory the application installed it under. Stable across publications.
    pub installed_as: String,
    /// The generation this reader borrowed. Changes when an index is republished, so evidence
    /// gathered under one is not silently compared with evidence gathered under another.
    pub generation: String,
}

impl DictionaryId {
    pub fn of(dictionary: &Dictionary) -> DictionaryId {
        DictionaryId {
            title: dictionary.meta().title.clone(),
            installed_as: dictionary.installed_as(),
            generation: dictionary
                .root()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

/// A dictionary record, addressable without assuming what a sequence means.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntryId {
    pub dictionary: DictionaryId,
    /// The dictionary's own sequence, when it has one.
    pub sequence: Option<i64>,
    /// A content digest of the record itself.
    ///
    /// **`(term, reading)` is not a locator.** Two sequence-less records can share both and differ
    /// in grammatical class or in what they mean, so keying on the pair silently merged them. This
    /// is derived from everything the record says, so two records are the same record only when
    /// they *are* the same record.
    pub locator: String,
    /// The sequence resolution followed this record's senses to, where it did.
    ///
    /// Set from what `lookup::whole_word` observed. Absent means no relationship was established —
    /// **not** that none exists, and not a licence to infer one from the sign.
    pub same_word_as: Option<i64>,
}

impl EntryId {
    /// The key two records are the same word under.
    ///
    /// **Only relationships that were established.** A dictionary owning a sequence bucket says it
    /// can be queried by sequence, not that `-1577490` denotes `1577490` — so the sign is never
    /// equated on that basis. Where resolution *followed* a cross-reference, it reported the target
    /// and that is used; otherwise a record is keyed by its own sequence, or by a content digest
    /// when it has none.
    pub fn same_word_key(&self) -> (DictionaryId, Option<i64>, String) {
        // A relationship resolution actually established takes precedence: the record's senses
        // came from that sequence, so it and the target are one word here.
        if let Some(target) = self.same_word_as {
            return (self.dictionary.clone(), Some(target), String::new());
        }
        match self.sequence {
            Some(sequence) => (self.dictionary.clone(), Some(sequence), String::new()),
            // No sequence and no established relationship: the record addresses only itself.
            None => (self.dictionary.clone(), None, self.locator.clone()),
        }
    }
}

/// A content digest of one record, for addressing it when a sequence cannot.
///
/// Covers everything the record asserts — spelling, reading, grammatical class, register tags and
/// every gloss — so two records collide only when they say the same things. Two sequence-less
/// entries sharing a term and a reading but differing in morphology are two records, and were one
/// under the pair alone.
fn record_digest(entry: &Entry) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in [entry.term.as_str(), entry.reading.as_str()] {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    for rule in &entry.rules {
        hasher.update(rule.as_bytes());
        hasher.update([0x1e]);
    }
    for tag in &entry.definition_tags {
        hasher.update(tag.as_bytes());
        hasher.update([0x1d]);
    }
    for definition in &entry.definitions {
        for tag in &definition.tags {
            hasher.update(tag.as_bytes());
            hasher.update([0x1c]);
        }
        for gloss in &definition.glosses {
            hasher.update(gloss.as_bytes());
            hasher.update([0x1b]);
        }
    }
    crate::hashing::hex(hasher.finalize())
}

/// What kind of answer a record is.
///
/// Reused from the dictionary's own classification — name tags and cross-reference text — rather
/// than guessed from a title or an English gloss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnswerKind {
    /// Senses a reader can read. The only kind that supports calling a span a word.
    OrdinaryDefinition,
    /// Name tags only. Says somebody is called this, not what it means.
    NameOnly,
    /// Cross-reference text whose target was not resolved — an arrow, not a meaning.
    UnresolvedPointer,
    /// A record with no glosses at all.
    NoDefinition,
}

impl AnswerKind {
    /// Classify a record the way the dictionary layer already classifies it.
    pub fn of(entry: &Entry) -> AnswerKind {
        let has_glosses = entry.definitions.iter().any(|definition| {
            definition
                .glosses
                .iter()
                .any(|gloss| !gloss.trim().is_empty())
        });
        if !has_glosses {
            return AnswerKind::NoDefinition;
        }
        if crate::lookup::is_cross_reference(entry) {
            return AnswerKind::UnresolvedPointer;
        }
        if crate::dict::tags::marks_a_name(&entry.definition_tags) {
            return AnswerKind::NameOnly;
        }
        AnswerKind::OrdinaryDefinition
    }

    pub fn name(self) -> &'static str {
        match self {
            AnswerKind::OrdinaryDefinition => "ordinary-definition",
            AnswerKind::NameOnly => "name-only",
            AnswerKind::UnresolvedPointer => "unresolved-pointer",
            AnswerKind::NoDefinition => "no-definition",
        }
    }
}

/// One dictionary record supporting a proposal, with everything selection and inflection need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryEvidence {
    pub id: EntryId,
    pub term: String,
    pub reading: String,
    /// What the record is.
    pub kind: AnswerKind,
    /// How the query reached it.
    pub matched: MatchKind,
    /// What the query was: the whole token, a component, or unrecorded.
    pub retrieved: Retrieved,
    /// Deinflection rule identifiers (`v1`, `v5`, `adj-i`). **Empty is not "incompatible"** —
    /// 格好いい carries none — so absence is its own outcome, not a rejection.
    pub rules: Vec<String>,
    /// Part-of-speech and register tags, as the dictionary files them. **Register, mostly**:
    /// 格好いい's are `★`, `priority`, `form`, which say how common it is and nothing about what
    /// kind of word it is.
    pub tags: Vec<String>,
    /// The class codes the record's **senses** carry, canonicalized through
    /// [`crate::dict::tags`]: Jitendex writes `adjective`, `particle`, `1-dan`, and the codes
    /// behind those labels are what can be compared with an analyzer's.
    ///
    /// **This is where 格好いい's part of speech actually is.** Its `rules` are empty and its
    /// definition tags are register markers, but every one of its senses is tagged `adjective`.
    /// A selector that looked only at the first two would have concluded the dictionary says
    /// nothing about it, and abstained on the case this layer exists for.
    pub sense_tags: Vec<String>,
}

impl EntryEvidence {
    /// Read a hit, classifying it rather than assuming.
    pub fn read(
        dictionary: &Dictionary,
        found: &crate::lookup::WholeWord,
        retrieved: Retrieved,
    ) -> EntryEvidence {
        let hit = &found.hit;
        let entry = &hit.entry;
        EntryEvidence {
            id: EntryId {
                dictionary: DictionaryId::of(dictionary),
                sequence: (entry.sequence != 0).then_some(entry.sequence),
                locator: record_digest(entry),
                same_word_as: found.same_word_as,
            },
            term: entry.term.clone(),
            reading: entry.reading.clone(),
            kind: AnswerKind::of(entry),
            matched: hit.matched,
            retrieved,
            rules: entry.rules.clone(),
            tags: entry.definition_tags.clone(),
            sense_tags: entry
                .definitions
                .iter()
                .flat_map(|definition| definition.tags.iter())
                .filter_map(|label| crate::dict::tags::canonical_code(label))
                .map(str::to_string)
                .collect(),
        }
    }

    /// The grammatical class this record asserts, canonically ordered.
    ///
    /// **`rules` and nothing else.** That is the dictionary's own explicit class field, in the
    /// same code space the analyzer normalises UniDic into (`v1`, `v5r`, `adj-i`), so the two can
    /// be compared. `definition_tags` is deliberately excluded: it mixes grammar with register —
    /// 格好いい carries `★`, `priority`, `form` and no class at all — so partitioning on it would
    /// split one word across its spellings while telling us nothing about grammar.
    ///
    /// **A record asserting no rules has no class here**, and cannot be separated from another
    /// that also asserts none. That is why absent rules require independent analyzer morphology
    /// before anything may be concluded from them, rather than being read as agreement.
    pub fn grammar_class(&self) -> Vec<String> {
        let mut classes: Vec<String> = self
            .rules
            .iter()
            .map(|rule| rule.trim().to_ascii_lowercase())
            .filter(|rule| !rule.is_empty())
            .collect();
        classes.sort();
        classes.dedup();
        classes
    }

    /// Whether this record's class is compatible with one established elsewhere.
    ///
    /// Three outcomes, never two: the record agrees, the record disagrees, or one of the two sides
    /// asserts nothing. **An empty claim is absent, not universal** — an earlier version compared
    /// the codes by string prefix, under which `""` was a prefix of every class and so agreed with
    /// all of them, turning "the analyzer told us nothing" into "the analyzer agrees".
    pub fn agrees_with_class(&self, other_class: &str) -> ClassAgreement {
        let classes = self.grammar_class();
        if classes.is_empty() {
            return ClassAgreement::Absent;
        }
        let outcomes: Vec<ClassAgreement> = classes
            .iter()
            .map(|class| classes_agree(class, other_class))
            .collect();
        if outcomes.contains(&ClassAgreement::Compatible) {
            ClassAgreement::Compatible
        } else if outcomes.contains(&ClassAgreement::Incompatible) {
            ClassAgreement::Incompatible
        } else {
            ClassAgreement::Absent
        }
    }

    /// The class the record's senses assert, canonically ordered.
    ///
    /// Kept apart from [`EntryEvidence::grammar_class`] on purpose. `rules` is the dictionary's
    /// own deinflection field and is what `interpretation_key` partitions on; sense tags are a
    /// second, weaker source that must not be allowed to pool two records the rules separate.
    /// They answer different questions and are asked separately.
    pub fn sense_class(&self) -> Vec<String> {
        let mut classes: Vec<String> = self
            .sense_tags
            .iter()
            .map(|tag| tag.trim().to_ascii_lowercase())
            .filter(|tag| !tag.is_empty())
            .collect();
        classes.sort();
        classes.dedup();
        classes
    }

    /// What the record asserts about its class, from **whichever field records one**.
    ///
    /// `rules` first, because that is the dictionary's explicit grammatical field; its senses
    /// after, because an entry whose rules are empty may still say plainly that it is an
    /// adjective. `Absent` only when neither does — and an entry that asserts nothing anywhere
    /// is one nothing can be concluded from.
    pub fn asserted_class(&self, other_class: &str) -> ClassAgreement {
        if !self.grammar_class().is_empty() {
            return self.agrees_with_class(other_class);
        }
        let classes = self.sense_class();
        if classes.is_empty() {
            return ClassAgreement::Absent;
        }
        let outcomes: Vec<ClassAgreement> = classes
            .iter()
            .map(|class| classes_agree(class, other_class))
            .collect();
        if outcomes.contains(&ClassAgreement::Compatible) {
            ClassAgreement::Compatible
        } else if outcomes.contains(&ClassAgreement::Incompatible) {
            ClassAgreement::Incompatible
        } else {
            ClassAgreement::Absent
        }
    }

    /// The class evidence this record actually offers, from whichever field records one.
    ///
    /// Used to tell two interpretations apart. `rules` where there are rules, senses otherwise —
    /// the same order of preference [`EntryEvidence::asserted_class`] uses, so a record cannot be
    /// compared by one field and partitioned by another.
    pub fn class_evidence(&self) -> Vec<String> {
        let rules = self.grammar_class();
        let classes = if rules.is_empty() {
            self.sense_class()
        } else {
            rules
        };
        let mut canonical: Vec<String> = classes
            .iter()
            .map(|class| canonical_class(class))
            .filter(|class| !class.is_empty())
            .collect();
        canonical.sort();
        canonical.dedup();
        canonical
    }

    /// Whether the record asserts a class in any field at all.
    pub fn asserts_any_class(&self) -> bool {
        !self.grammar_class().is_empty() || !self.sense_class().is_empty()
    }

    /// Whether this record can support treating the span as a word.
    ///
    /// **Both halves are required.** A whole-query name is still a name; an ordinary definition
    /// found for a component is still about the component.
    pub fn supports_lexical_unit(&self) -> bool {
        self.kind == AnswerKind::OrdinaryDefinition
            && matches!(self.retrieved, Retrieved::Whole { .. })
    }
}

/// A grammatical class code, split into the parts that decide what it is compatible with.
///
/// **Explicit relationships, not string prefixes.** The codes are a small vocabulary shared with
/// the analyzer's normaliser (`core/src/analyzer/lindera/mod.rs`), and comparing them as raw
/// strings gets two things wrong that matter here: `""` prefixes everything, and `v5` prefixes
/// `v5r` for the right reason while `v` would prefix `v5r` and `v1` alike for no reason at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClassCode<'a> {
    /// The class proper: `adj-i`, `v5`, `vs`.
    family: &'a str,
    /// What the family leaves open — the godan row, an irregularity — or empty when the code
    /// names the family alone.
    variant: &'a str,
}

/// The verb families, longest-disambiguating first so `v5r` is not read as `v5` plus `r`… which
/// is in fact exactly what it is. The list exists so `vt` (transitive, not a conjugation class)
/// is not mistaken for a verb family.
const VERB_FAMILIES: [&str; 9] = ["v5", "v4", "v2", "v1", "vs", "vz", "vk", "vn", "vr"];

impl<'a> ClassCode<'a> {
    /// Parse a lowercased, trimmed code. `None` where there is no class to compare.
    fn parse(code: &'a str) -> Option<ClassCode<'a>> {
        if code.is_empty() {
            return None;
        }
        // **`adj-ix` is the よい/いい class of i-adjective, not a different class.** The `x` marks
        // the suppletion — which is the very case this layer exists for — so a record filed
        // `adj-ix` must agree with an analyzer that says `adj-i`, and a strict code comparison
        // would reject 格好いい's own family.
        if code == "adj-i" || code == "adj-ix" {
            return Some(ClassCode {
                family: "adj-i",
                variant: "",
            });
        }
        for family in VERB_FAMILIES {
            let Some(rest) = code.strip_prefix(family) else {
                continue;
            };
            // `v5` alone, `v5r`, `v5r-i`, `v1-s` — but not `v55`, which is not a code.
            let opens = rest.is_empty()
                || rest.starts_with('-')
                || rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
            if opens {
                return Some(ClassCode {
                    family,
                    variant: rest.trim_start_matches('-'),
                });
            }
        }
        Some(ClassCode {
            family: code,
            variant: "",
        })
    }
}

/// Whether two variants within one family can be the same word's class.
///
/// Empty means the code named only the family — the analyzer often knows the group and not the
/// row — and that is compatible with any row rather than with none. Otherwise they agree only
/// when they are the same, or when one extends the other at a hyphen: `v5r` and `v5r-i` are the
/// same row with an irregularity noted, `v5r` and `v5k` are different rows.
fn variants_agree(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() || left == right {
        return true;
    }
    let (short, long) = if left.len() < right.len() {
        (left, right)
    } else {
        (right, left)
    };
    long.strip_prefix(short)
        .is_some_and(|rest| rest.starts_with('-'))
}

/// The canonical spelling of a class code, for telling two interpretations apart.
///
/// **Comparison and identity are different operations.** [`classes_agree`] answers "could these
/// be the same class", which is what evidence needs; a map key needs "are these the same class",
/// which string equality gets wrong in the one place it matters: `adj-i` and `adj-ix` are the
/// same class written two ways, and two records asserting them are not two interpretations to
/// abstain between.
///
/// The variant is kept, so `v5r` and `v5k` stay apart — they are different rows and different
/// words. A code that names only its family stays as it is, so `v5` and `v5r` are two claims:
/// conservative, and abstention is the safe side of that line.
pub fn canonical_class(code: &str) -> String {
    let code = code.trim().to_ascii_lowercase();
    match ClassCode::parse(&code) {
        Some(parsed) if parsed.variant.is_empty() => parsed.family.to_string(),
        Some(parsed) => format!("{}-{}", parsed.family, parsed.variant),
        None => String::new(),
    }
}

/// Compare two grammatical class codes.
///
/// **Absent is returned whenever either side asserts nothing**, so a missing class can never be
/// read as agreement by a caller that only checks for `Incompatible`.
pub fn classes_agree(left: &str, right: &str) -> ClassAgreement {
    let left = left.trim().to_ascii_lowercase();
    let right = right.trim().to_ascii_lowercase();
    let (Some(left), Some(right)) = (ClassCode::parse(&left), ClassCode::parse(&right)) else {
        return ClassAgreement::Absent;
    };
    if left.family == right.family && variants_agree(left.variant, right.variant) {
        ClassAgreement::Compatible
    } else {
        ClassAgreement::Incompatible
    }
}

/// Whether a record's asserted class agrees with one established elsewhere.
///
/// **Absent is not agreement.** 格好いい asserts no class, so nothing about it can be concluded
/// from its silence — which is why it is a third outcome and not a permissive second one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassAgreement {
    Compatible,
    Incompatible,
    Absent,
}

impl ClassAgreement {
    pub fn name(self) -> &'static str {
        match self {
            ClassAgreement::Compatible => "compatible",
            ClassAgreement::Incompatible => "incompatible",
            ClassAgreement::Absent => "absent",
        }
    }
}

/// The reading of a proposed unit, or the honest absence of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    /// Every constituent contributed one, so this is the reading the analyzer established.
    Established(String),
    /// At least one constituent had none. **Not a concatenation of the rest** — a partial reading
    /// presented as whole is a false claim, and furigana would be built from it.
    Incomplete,
}

impl Reading {
    pub fn established(&self) -> Option<&str> {
        match self {
            Reading::Established(reading) => Some(reading),
            Reading::Incomplete => None,
        }
    }
}

/// Where a proposal came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Baseline,
    LexicalLookup,
    InflectionRecovery,
    AlternativeAnalysis,
    Construction,
}

impl Origin {
    pub fn name(self) -> &'static str {
        match self {
            Origin::Baseline => "baseline",
            Origin::LexicalLookup => "lexical-lookup",
            Origin::InflectionRecovery => "inflection-recovery",
            Origin::AlternativeAnalysis => "alternative-analysis",
            Origin::Construction => "construction",
        }
    }
}

/// Why a candidate may or may not be chosen automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eligibility {
    Eligible,
    Rejected(&'static str),
    Unassessed,
}

impl Eligibility {
    pub fn is_eligible(&self) -> bool {
        matches!(self, Eligibility::Eligible)
    }
    pub fn reason(&self) -> &'static str {
        match self {
            Eligibility::Eligible => "eligible",
            Eligibility::Rejected(why) => why,
            Eligibility::Unassessed => "not assessed",
        }
    }
}

/// How a proposed dictionary form was arrived at, in a form the selector can act on.
///
/// **Typed, not prose.** A sentence explaining a derivation is for a reader; a selector that
/// parsed one would be interpreting English.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Derivation {
    /// Read directly off the source; no transformation.
    AsWritten,
    /// Recovered by reversing an inflection, with the chain retained for explanation.
    Inflection {
        /// The grammatical class the transformation assumed.
        class: &'static str,
        /// What established that the span is of that class. **Not the same question** as whether
        /// a reading matched.
        ground: InflectionGround,
        /// Each step, for showing. Never the evidence itself.
        steps: Vec<String>,
    },
}

/// What establishes that a span really is the kind of word a transformation assumed.
///
/// # Why composed morphology is a hypothesis
///
/// The analyzer described **かっこ** and **よかった**. It did not describe かっこよかった, because
/// under this segmentation that span is not a token and nothing ever analysed it. Taking the head
/// constituent's class as the span's class is a claim about **structure** — that these tokens are
/// one compound whose head carries the inflection — and it is a reasonable claim to test, not a
/// fact to assume.
///
/// So it is recorded as what it is. A selector may require more of a composed class than of one
/// the analyzer asserted about the whole span, and it can only do that if the difference survives
/// generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InflectionGround {
    /// The analyzer's own class for this exact token supports the transformation.
    TokenMorphology { token: usize, class: String },
    /// The analyzer's class for the span's **head constituent** supports it, on the hypothesis
    /// that the span is a compound headed by that token.
    ComposedMorphology { head: usize, class: String },
    /// No analyzer class was available, and the supporting **record's** own asserted class is
    /// what carried it.
    RecordClass,
    /// Nothing but a matching reading. A reading establishes a possible word and never its class,
    /// so this is an alternative to offer and never an answer to choose.
    ReadingOnly,
}

impl InflectionGround {
    pub fn name(&self) -> &'static str {
        match self {
            InflectionGround::TokenMorphology { .. } => "token-morphology",
            InflectionGround::ComposedMorphology { .. } => "composed-morphology",
            InflectionGround::RecordClass => "record-class",
            InflectionGround::ReadingOnly => "reading-only",
        }
    }

    /// Whether anything other than a matching reading supports the claim.
    pub fn is_grounded(&self) -> bool {
        !matches!(self, InflectionGround::ReadingOnly)
    }

    /// Whether the class was asserted about **this span**, rather than about a part of it.
    ///
    /// Reported rather than gated on, and that is a decision worth stating. The selector ends up
    /// requiring the same thing of both: the *record* must assert a compatible class for the
    /// whole unit. That requirement is strictly stronger than anything composed morphology could
    /// have carried on its own, so a separate weaker gate for it would be dead weight — while the
    /// distinction still matters for reporting and for the ablations in §5, which need to say
    /// which evidence a decision actually rested on.
    pub fn is_about_the_whole_span(&self) -> bool {
        matches!(
            self,
            InflectionGround::TokenMorphology { .. } | InflectionGround::RecordClass
        )
    }
}

/// One proposed interpretation of one span.
///
/// **One interpretation, not one span.** Two readings over the same bytes are two candidates, and
/// collapsing them would hide the disagreement a selector must abstain on.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub start: usize,
    pub end: usize,
    pub tokens: Vec<usize>,
    /// The source exactly as written. Never normalized, never rewritten.
    pub surface: String,
    /// The dictionary form proposed, which may differ in script from `surface`.
    pub dict_form: String,
    /// The reading of the surface as it appears.
    pub surface_reading: Reading,
    /// The reading of the proposed dictionary form.
    pub lemma_reading: Reading,
    pub origin: Origin,
    pub derivation: Derivation,
    /// Records supporting this interpretation. All describe the same word.
    pub evidence: Vec<EntryEvidence>,
    pub eligibility: Eligibility,
}

impl Candidate {
    /// Whether any record supports treating this span as a word.
    pub fn supported_as_lexical_unit(&self) -> bool {
        self.evidence
            .iter()
            .any(EntryEvidence::supports_lexical_unit)
    }

    /// The categories of evidence present, for reporting without judging.
    pub fn evidence_kinds(&self) -> Vec<AnswerKind> {
        let mut kinds: Vec<AnswerKind> = self.evidence.iter().map(|e| e.kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        kinds
    }

    pub fn joins(&self) -> usize {
        self.tokens.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::yomitan::Definition;

    fn entry(term: &str, reading: &str, sequence: i64, glosses: &[&str], tags: &[&str]) -> Entry {
        Entry {
            term: term.into(),
            reading: reading.into(),
            sequence,
            definition_tags: tags.iter().map(|t| (*t).to_string()).collect(),
            definitions: vec![Definition {
                glosses: glosses.iter().map(|g| (*g).to_string()).collect(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn dictionary_id(installed_as: &str) -> DictionaryId {
        DictionaryId {
            title: "Shared Title".into(),
            installed_as: installed_as.into(),
            generation: "g1".into(),
        }
    }

    fn evidence(id: EntryId, kind: AnswerKind, retrieved: Retrieved) -> EntryEvidence {
        EntryEvidence {
            id,
            term: "x".into(),
            reading: "x".into(),
            kind,
            matched: MatchKind::Spelling,
            retrieved,
            rules: Vec::new(),
            tags: Vec::new(),
            sense_tags: Vec::new(),
        }
    }

    fn id(
        installed_as: &str,
        sequence: Option<i64>,
        locator: &str,
        same_word_as: Option<i64>,
    ) -> EntryId {
        EntryId {
            dictionary: dictionary_id(installed_as),
            sequence,
            locator: locator.into(),
            same_word_as,
        }
    }

    /// **The sign is never equated on capability alone.**
    ///
    /// A dictionary owning a sequence bucket says it can be queried by sequence. Only a resolution
    /// that actually followed a cross-reference establishes that two records are one word.
    #[test]
    fn only_an_established_relationship_joins_two_sequences() {
        let target = id("jitendex", Some(1577490), "d1", None);
        let unresolved = id("jitendex", Some(-1577490), "d2", None);
        assert_ne!(
            target.same_word_key(),
            unresolved.same_word_key(),
            "no relationship was established, so the sign proves nothing"
        );

        let resolved = id("jitendex", Some(-1577490), "d2", Some(1577490));
        assert_eq!(
            target.same_word_key(),
            resolved.same_word_key(),
            "resolution followed this record to 1577490, and that is observed rather than assumed"
        );
    }

    /// **A term and a reading are not a locator.**
    #[test]
    fn two_records_sharing_a_spelling_and_reading_stay_distinct() {
        let adjective = entry("いい", "いい", 0, &["good"], &["adj-ix"]);
        let interjection = entry("いい", "いい", 0, &["yes"], &["int"]);
        assert_ne!(
            record_digest(&adjective),
            record_digest(&interjection),
            "same spelling, same reading, different morphology and senses — two records"
        );
        assert_eq!(
            record_digest(&adjective),
            record_digest(&entry("いい", "いい", 0, &["good"], &["adj-ix"])),
            "and the same record digests the same"
        );

        let first = id("d", None, &record_digest(&adjective), None);
        let second = id("d", None, &record_digest(&interjection), None);
        assert_ne!(first.same_word_key(), second.same_word_key());
    }

    /// Two installations sharing a title are two dictionaries.
    #[test]
    fn a_shared_title_does_not_collapse_two_installations() {
        let first = id("jitendex", Some(1577490), "d", None);
        let second = id("jitendex-2024", Some(1577490), "d", None);
        assert_eq!(first.dictionary.title, second.dictionary.title);
        assert_ne!(first.same_word_key(), second.same_word_key());
    }

    /// A republished index is a different generation.
    #[test]
    fn a_new_generation_is_not_the_old_one() {
        let mut later = id("jitendex", Some(1), "d", None);
        later.dictionary.generation = "g2".into();
        assert_ne!(
            id("jitendex", Some(1), "d", None).same_word_key(),
            later.same_word_key()
        );
    }

    /// **The check that used to pass everything.** Whole-query retrieval is not lexical support.
    #[test]
    fn a_name_or_a_pointer_cannot_qualify_a_lexical_unit() {
        let whole = Retrieved::Whole {
            query: "ヒフミ".into(),
        };
        for kind in [
            AnswerKind::NameOnly,
            AnswerKind::UnresolvedPointer,
            AnswerKind::NoDefinition,
        ] {
            assert!(
                !evidence(id("a", Some(1), "d", None), kind, whole.clone()).supports_lexical_unit(),
                "{} retrieved Whole still cannot support a word",
                kind.name()
            );
        }
        assert!(
            evidence(
                id("a", Some(1), "d", None),
                AnswerKind::OrdinaryDefinition,
                whole
            )
            .supports_lexical_unit(),
            "an ordinary definition for the whole query does"
        );
        assert!(
            !evidence(
                id("a", Some(1), "d", None),
                AnswerKind::OrdinaryDefinition,
                Retrieved::Component {
                    query: "神".into()
                }
            )
            .supports_lexical_unit(),
            "and an ordinary definition found for a component does not"
        );
    }

    /// Classification comes from the dictionary's own tags and text, not a heuristic.
    #[test]
    fn answer_kinds_are_read_from_the_record() {
        assert_eq!(
            AnswerKind::of(&entry(
                "格好いい",
                "かっこいい",
                1577490,
                &["attractive"],
                &[]
            )),
            AnswerKind::OrdinaryDefinition
        );
        assert_eq!(
            AnswerKind::of(&entry("ヒフミ", "ひふみ", 5000, &["Hifumi"], &["fem"])),
            AnswerKind::NameOnly
        );
        assert_eq!(
            AnswerKind::of(&entry(
                "抜きとる",
                "ぬきとる",
                -1478150,
                &["\u{27f6} 抜 ぬ き 取 と る"],
                &[]
            )),
            AnswerKind::UnresolvedPointer
        );
        assert_eq!(
            AnswerKind::of(&entry("x", "x", 1, &[""], &[])),
            AnswerKind::NoDefinition
        );
    }

    /// Every candidate can say why it was or was not eligible, including the ones that passed.
    #[test]
    fn eligibility_always_has_a_reason_to_report() {
        assert_eq!(Eligibility::Eligible.reason(), "eligible");
        assert!(Eligibility::Eligible.is_eligible());
        assert_eq!(
            Eligibility::Rejected("no whole-word entry").reason(),
            "no whole-word entry"
        );
        assert!(!Eligibility::Rejected("no whole-word entry").is_eligible());
        assert!(!Eligibility::Unassessed.is_eligible());
    }

    /// A partial reading is never presented as an established one.
    #[test]
    fn an_incomplete_reading_says_so() {
        assert_eq!(
            Reading::Established("かっこいい".into()).established(),
            Some("かっこいい")
        );
        assert_eq!(Reading::Incomplete.established(), None);
    }

    /// **Identity is not compatibility.** `adj-i` and `adj-ix` are one class written two ways;
    /// `v5r` and `v5k` are two.
    #[test]
    fn a_class_code_has_one_canonical_spelling() {
        assert_eq!(canonical_class("adj-i"), canonical_class("adj-ix"));
        assert_eq!(canonical_class("adj-i"), "adj-i");
        assert_ne!(canonical_class("v5r"), canonical_class("v5k"));
        assert_eq!(canonical_class("v5r"), "v5-r");
        assert_eq!(canonical_class("v1-s"), "v1-s");
        // Under-specified stays under-specified: `v5` is not spelled as any particular row.
        assert_ne!(canonical_class("v5"), canonical_class("v5r"));
        assert_eq!(canonical_class("  "), "");
    }

    /// **Class codes are compared as classes**, and an empty claim is absent rather than universal.
    #[test]
    fn class_codes_relate_explicitly_rather_than_by_string_prefix() {
        use ClassAgreement::{Absent, Compatible, Incompatible};

        // The analyzer may know the group where the dictionary files the row, and neither is
        // wrong about the other.
        assert_eq!(classes_agree("v5r", "v5"), Compatible);
        assert_eq!(classes_agree("v5", "v5r"), Compatible);
        assert_eq!(classes_agree("v5r", "v5r-i"), Compatible);
        assert_eq!(classes_agree("v1", "v1-s"), Compatible);
        assert_eq!(classes_agree("vs-i", "vs"), Compatible);
        // The よい/いい class is an i-adjective.
        assert_eq!(classes_agree("adj-ix", "adj-i"), Compatible);

        // Different rows, different families, different kinds of word.
        assert_eq!(classes_agree("v5r", "v5k"), Incompatible);
        assert_eq!(classes_agree("v1", "v5"), Incompatible);
        assert_eq!(classes_agree("adj-i", "v5r"), Incompatible);
        assert_eq!(classes_agree("adj-i", "adj-na"), Incompatible);
        // A bare `v` is not a family that `v5r` and `v1` both belong to.
        assert_eq!(classes_agree("v", "v5r"), Incompatible);

        // **The prefix bug, named.** Under string-prefix comparison every one of these agreed.
        for class in ["adj-i", "v5r", "v1", "vk", "n"] {
            assert_eq!(
                classes_agree("", class),
                Absent,
                "{class}: an empty claim asserts nothing and must not agree with it"
            );
            assert_eq!(classes_agree(class, "   "), Absent);
        }
    }

    /// The same, through a record: absent on either side stays absent.
    #[test]
    fn a_record_compared_with_an_empty_class_reports_absent() {
        let mut adjective = evidence(
            id("a", Some(1), "d", None),
            AnswerKind::OrdinaryDefinition,
            Retrieved::Whole {
                query: "たかい".into(),
            },
        );
        adjective.rules = vec!["adj-i".into()];
        assert_eq!(
            adjective.agrees_with_class("adj-i"),
            ClassAgreement::Compatible
        );
        assert_eq!(
            adjective.agrees_with_class("v5r"),
            ClassAgreement::Incompatible
        );
        assert_eq!(
            adjective.agrees_with_class(""),
            ClassAgreement::Absent,
            "an analyzer that said nothing has not agreed"
        );
    }
}
