//! **Contracted verb forms the engine split, rejoined — only where the dictionary accounts for the
//! whole span.**
//!
//! Ichiran's own segmentation breaks dialect and casual contractions: 間違っとる arrives as 間 |
//! 違っとる with the second half unanswered, 怒んなきゃ as 怒 | ん | なきゃ. Neither is a conversion
//! defect — the raw output already has them — and neither is in
//! the engine's first five segmentations. So the repair is local, after the engine, and narrow:
//!
//! 1. **Trigger, structural.** A token ending in a contraction tail ([`contraction::TAILS`]). A
//!    trigger opens an investigation and authorises nothing.
//! 2. **Advice.** The built-in analyzer reads the sentence, which composes these auxiliaries
//!    (`lindera::is_contracted_auxiliary`). Its token must end exactly where the trigger ends and
//!    start where one of the engine's tokens starts, so the run it names is a union of whole
//!    tokens. **Advice is a proposal, never an authority**: して回っとる comes back as one token
//!    with the lemma する, and する cannot derive it, so nothing happens.
//! 3. **Validation, every check required.** The run is at least two tokens or one the engine left
//!    unanswered; no whitespace or punctuation inside; within the size limits. The advised lemma
//!    is looked up by spelling, **and the reading index is searched for its reading directly**
//!    (`lookup::whole_word_by_reading`) — the spelling-first lookup alone stops at the first
//!    spelling match and would hide the homophones — so a word that also derives the span is found
//!    and competes. Either search failing abstains. Each record must be an ordinary definition of a verb class compatible with the
//!    advice, and must derive the **whole span** from its own spelling and reading
//!    ([`contraction::derive`]) with the advisor's reading. Exactly one word may survive, with one
//!    derivation. Any lookup failure abstains.
//! 4. **Apply.** One token over the run: source spelling and span, the entry's lemma, the derived
//!    reading, and parts that are the derivation's two source slices — never the expanded form.
//!    The parts are validated before the token is kept.
//!
//! Everything is accounted for in [`Account`], abstentions included, so a measurement can say what
//! was tried and why nothing changed as well as what did.

use std::collections::{BTreeMap, BTreeSet};

use super::candidate::{
    classes_agree, AnswerKind, ClassAgreement, DictionaryId, EntryEvidence, EntryId,
};
use super::contraction::{self, Derived, TAILS};
use crate::analyzer::assist::{
    Advice, Advisor, AnalyzerAdvisor, Assistance, AssistanceMode, Stage,
};
use crate::analyzer::Analyzer;
use crate::dict::evidence::{Evidence, Source};
use crate::dict::index::Dictionary;
use crate::dict::merge::Retrieved;
use crate::kana::{is_kana, is_kanji, to_hiragana};
use crate::model::{
    Analysis, Conjugation, ContractionFamily, Prop, RepairNote, StemEvidence, Token,
};

/// A lookup route: the spelling-first reader's lookup, or the reading index asked directly.
#[derive(Clone, Copy)]
enum Route {
    Spelling,
    Reading,
}

impl Route {
    fn ask(
        self,
        source: &Source<'_>,
        query: &str,
    ) -> Result<Vec<crate::lookup::WholeWord>, crate::dict::DictError> {
        match self {
            Route::Spelling => source.whole_word(query),
            Route::Reading => source.whole_word_by_reading(query),
        }
    }
}

/// A sentence longer than this keeps the engine's output. The advisor reads a sentence at a time,
/// and an unbounded sentence is an unbounded cost for a repair that touches one word.
pub const MAX_SENTENCE_CHARS: usize = 256;
/// The longest run of the engine's tokens one repair may join.
pub const MAX_RUN_TOKENS: usize = 8;
/// The longest span, in characters, one repair may cover.
pub const MAX_RUN_CHARS: usize = 64;

/// Why an investigation changed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Abstain {
    /// No advisor could be loaded.
    AdvisorUnavailable,
    /// The advisor failed on the sentence.
    AdvisorFailed(String),
    /// The sentence exceeds [`MAX_SENTENCE_CHARS`].
    SentenceTooLong,
    /// The trigger or a sentence boundary has no source position.
    Ungrounded,
    /// No advice token ends where the trigger ends, or it starts inside one of the engine's tokens.
    NotAligned,
    /// The run exceeds [`MAX_RUN_TOKENS`] or [`MAX_RUN_CHARS`].
    TooLong,
    /// Whitespace, punctuation or a gap inside the run.
    Barrier,
    /// The advice carries no verb conjugation to derive from.
    NotAVerb,
    /// A dictionary lookup failed. Never read as an absence.
    LookupFailed(String),
    /// The advice gives no lemma reading, so the reading index cannot be searched and the field
    /// of competitors is unknown.
    IncompleteSearch,
    /// No record derives the span with the advisor's reading.
    NoDerivation,
    /// More than one word derives the span: 取る and 撮る, かかる's several entries.
    CompetingWords(usize),
    /// One word derives the span in more than one way.
    AmbiguousDerivation,
    /// The repaired token's parts failed validation.
    ComponentFault(String),
    /// The run overlaps a run already repaired.
    Overlap,
    /// The repaired analysis failed the span contract, so the engine's own output was kept.
    SpanContract(String),
}

impl Abstain {
    pub fn name(&self) -> &'static str {
        match self {
            Abstain::AdvisorUnavailable => "advisor-unavailable",
            Abstain::AdvisorFailed(_) => "advisor-failed",
            Abstain::SentenceTooLong => "sentence-too-long",
            Abstain::Ungrounded => "ungrounded",
            Abstain::NotAligned => "not-aligned",
            Abstain::TooLong => "too-long",
            Abstain::Barrier => "barrier",
            Abstain::NotAVerb => "not-a-verb",
            Abstain::LookupFailed(_) => "lookup-failed",
            Abstain::IncompleteSearch => "incomplete-search",
            Abstain::NoDerivation => "no-derivation",
            Abstain::CompetingWords(_) => "competing-words",
            Abstain::AmbiguousDerivation => "ambiguous-derivation",
            Abstain::ComponentFault(_) => "component-fault",
            Abstain::Overlap => "overlap",
            Abstain::SpanContract(_) => "span-contract",
        }
    }
}

/// What one investigation concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Repaired(RepairNote),
    Abstained(Abstain),
}

/// One trigger, followed through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Investigation {
    /// The trigger token's index in the engine's analysis.
    pub token: usize,
    /// Byte span: the run's when advice aligned, the trigger's otherwise.
    pub start: usize,
    pub end: usize,
    /// How many of the engine's tokens the run covered; 1 before advice aligned.
    pub tokens: usize,
    pub outcome: Outcome,
}

/// Everything the stage did to one passage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Account {
    pub investigations: Vec<Investigation>,
    /// Tail-ending tokens the engine already answered whole — 読みとらなきゃ — which are not
    /// investigated at all.
    pub answered: usize,
    /// Sentences the advisor was asked to read.
    pub advised_sentences: usize,
    /// Whole-word dictionary lookups made.
    pub dictionary_calls: usize,
}

impl Account {
    /// The repairs, as recorded on the analysis's execution.
    pub fn notes(&self) -> Vec<RepairNote> {
        self.investigations
            .iter()
            .filter_map(|investigation| match &investigation.outcome {
                Outcome::Repaired(note) => Some(note.clone()),
                Outcome::Abstained(_) => None,
            })
            .collect()
    }

    pub fn abstentions(&self) -> impl Iterator<Item = (&Investigation, &Abstain)> {
        self.investigations
            .iter()
            .filter_map(|investigation| match &investigation.outcome {
                Outcome::Abstained(reason) => Some((investigation, reason)),
                Outcome::Repaired(_) => None,
            })
    }
}

/// The repaired analysis and the account. `advisor` is `None` when none could be loaded, and then
/// every trigger abstains.
///
/// Every sentence is put to `advisor` and every lookup to the dictionaries directly, as before
/// requests had a memory: [`repaired_in`] with nothing kept.
pub fn repaired(
    analysis: &Analysis,
    dictionaries: &[Dictionary],
    advisor: Option<&dyn Analyzer>,
) -> (Analysis, Account) {
    let adapted = advisor.map(AnalyzerAdvisor);
    let assistance = Assistance::with_provider(
        AssistanceMode::Legacy,
        adapted.as_ref().map(|advisor| advisor as &dyn Advisor),
    );
    repaired_in(analysis, Evidence::direct(dictionaries), &assistance)
}

/// [`repaired`], with the advice put to one request as [`Stage::Sentence`] questions and every
/// lookup made through its evidence.
///
/// **The per-sentence memory below is this pass's own**, and keeps what it always kept: an
/// abstention included, so two triggers in one sentence whose advice failed abstain together
/// rather than asking twice. The request remembers only answers, so the next pass asks again.
pub fn repaired_in(
    analysis: &Analysis,
    lookups: Evidence<'_>,
    assistance: &Assistance<'_>,
) -> (Analysis, Account) {
    let mut account = Account::default();
    let sentences = crate::model::sentence_ranges(&analysis.tokens);
    // Advice is per sentence and memoized: two triggers in one sentence ask once.
    let mut advice: BTreeMap<usize, Result<Vec<Token>, Abstain>> = BTreeMap::new();
    let mut accepted: Vec<(usize, usize, Token)> = Vec::new();

    for (index, token) in analysis.tokens.iter().enumerate() {
        if !TAILS.iter().any(|tail| token.surface.ends_with(tail)) {
            continue;
        }
        let mut investigation = Investigation {
            token: index,
            start: token.start.unwrap_or_default(),
            end: token.end.unwrap_or_default(),
            tokens: 1,
            outcome: Outcome::Abstained(Abstain::Ungrounded),
        };
        let (Some(_), Some(end)) = (token.start, token.end) else {
            account.investigations.push(investigation);
            continue;
        };
        let Some(sentence) = sentences.iter().position(|range| range.contains(index)) else {
            account.investigations.push(investigation);
            continue;
        };
        let advised = advice.entry(sentence).or_insert_with(|| {
            advise(
                analysis,
                sentences[sentence].first_token,
                sentences[sentence].last_token_end,
                assistance,
                &mut account.advised_sentences,
            )
        });
        let advised = match advised {
            Ok(tokens) => tokens,
            Err(reason) => {
                investigation.outcome = Outcome::Abstained(reason.clone());
                account.investigations.push(investigation);
                continue;
            }
        };
        let Some((first, advice_token)) = align(analysis, index, end, advised) else {
            // A lone token the engine answered is not a trigger: its answer stands.
            if answered(token) {
                account.answered += 1;
            } else {
                assistance.unused(Stage::Sentence);
                investigation.outcome = Outcome::Abstained(Abstain::NotAligned);
                account.investigations.push(investigation);
            }
            continue;
        };
        let run = &analysis.tokens[first..=index];
        if run.len() == 1 && answered(token) {
            account.answered += 1;
            continue;
        }
        let start = run[0].start.unwrap_or_default();
        investigation.start = start;
        investigation.tokens = run.len();
        if accepted.last().is_some_and(|(_, last, _)| first <= *last) {
            investigation.outcome = Outcome::Abstained(Abstain::Overlap);
            account.investigations.push(investigation);
            continue;
        }
        investigation.outcome = match investigate(
            analysis,
            lookups,
            run,
            start,
            end,
            advice_token,
            &mut account.dictionary_calls,
        ) {
            Ok((note, replacement)) => {
                accepted.push((first, index, replacement));
                Outcome::Repaired(note)
            }
            Err(reason) => Outcome::Abstained(reason),
        };
        account.investigations.push(investigation);
    }

    if accepted.is_empty() {
        return (analysis.clone(), account);
    }
    let mut tokens = Vec::with_capacity(analysis.tokens.len());
    let mut at = 0;
    for (first, last, replacement) in accepted {
        tokens.extend_from_slice(&analysis.tokens[at..first]);
        tokens.push(replacement);
        at = last + 1;
    }
    tokens.extend_from_slice(&analysis.tokens[at..]);
    (
        Analysis {
            text: analysis.text.clone(),
            tokens,
            execution: analysis.execution.clone(),
        },
        account,
    )
}

/// Whether the engine answered a token itself: senses, or a conjugation it recognised.
fn answered(token: &Token) -> bool {
    !token.senses.is_empty() || token.conjugation.is_some()
}

/// The advisor's tokens for one sentence, positioned in the passage.
fn advise(
    analysis: &Analysis,
    first_token: usize,
    last_token_end: usize,
    assistance: &Assistance<'_>,
    asked: &mut usize,
) -> Result<Vec<Token>, Abstain> {
    // First, as it always was: with no advisor, every trigger abstains for that reason and no
    // other, whatever the sentence.
    if assistance.provider().is_none() {
        return Err(Abstain::AdvisorUnavailable);
    }
    let tokens = &analysis.tokens[first_token..last_token_end];
    let start = tokens
        .iter()
        .find_map(|token| token.start)
        .ok_or(Abstain::Ungrounded)?;
    let end = tokens
        .iter()
        .rev()
        .find_map(|token| token.end)
        .ok_or(Abstain::Ungrounded)?;
    let sentence = analysis.text.get(start..end).ok_or(Abstain::Ungrounded)?;
    if sentence.chars().count() > MAX_SENTENCE_CHARS {
        return Err(Abstain::SentenceTooLong);
    }
    let advice = match assistance.ask(Stage::Sentence, sentence) {
        Advice::Answered(advice) => {
            *asked += 1;
            advice
        }
        Advice::Unavailable => return Err(Abstain::AdvisorUnavailable),
        Advice::Failed(error) => {
            *asked += 1;
            return Err(Abstain::AdvisorFailed(error.to_string()));
        }
    };
    Ok(advice
        .tokens
        .iter()
        .cloned()
        .map(|mut token| {
            token.start = token.start.map(|at| at + start);
            token.end = token.end.map(|at| at + start);
            token
        })
        .collect())
}

/// The advice token ending exactly at `end`, and the engine token its start falls on.
fn align<'a>(
    analysis: &Analysis,
    index: usize,
    end: usize,
    advice: &'a [Token],
) -> Option<(usize, &'a Token)> {
    let advice_token = advice.iter().find(|token| token.end == Some(end))?;
    let start = advice_token.start?;
    let first = (0..=index)
        .rev()
        .find(|&candidate| analysis.tokens[candidate].start == Some(start))?;
    Some((first, advice_token))
}

/// Every check, in order; the repair and its token, or why not.
fn investigate(
    analysis: &Analysis,
    lookups: Evidence<'_>,
    run: &[Token],
    start: usize,
    end: usize,
    advice: &Token,
    calls: &mut usize,
) -> Result<(RepairNote, Token), Abstain> {
    let surface = analysis.text.get(start..end).ok_or(Abstain::Ungrounded)?;
    if run.len() > MAX_RUN_TOKENS || surface.chars().count() > MAX_RUN_CHARS {
        return Err(Abstain::TooLong);
    }
    let contiguous = run
        .windows(2)
        .all(|pair| pair[0].end.is_some() && pair[0].end == pair[1].start);
    if !contiguous || !surface.chars().all(|c| is_kana(c) || is_kanji(c)) {
        return Err(Abstain::Barrier);
    }
    let Some(class) = advice
        .conjugation
        .as_ref()
        .and_then(|conjugation| conjugation.props.first())
        .map(|prop| prop.pos.clone())
        .filter(|pos| !pos.is_empty())
    else {
        return Err(Abstain::NotAVerb);
    };
    let advised_reading = to_hiragana(&advice.reading);

    // **Two routes, both required.** The advised lemma by spelling, as a reader's lookup asks it;
    // and every record filed under the lemma's **reading**, asked of the reading index directly.
    // The spelling route alone is spelling-first, so a kana lemma that matches one spelling never
    // reaches the homophones behind it: なる finds the adnominal なる and stops, and a kana-spelled
    // verb would hide every kanji entry that derives the span as well. Uniqueness is only a claim
    // once the whole field has been searched, so a search that cannot run is an abstention.
    let lemma_reading = to_hiragana(&advice.lemma_reading);
    if lemma_reading.is_empty() {
        return Err(Abstain::IncompleteSearch);
    }

    // Every record either route reached, **once**: the same record found by both is one piece of
    // evidence, not two.
    let mut records: Vec<EntryEvidence> = Vec::new();
    let mut seen: BTreeSet<EntryId> = BTreeSet::new();
    for source in lookups.sources() {
        let routes = [
            (advice.dict_form.as_str(), Route::Spelling),
            (lemma_reading.as_str(), Route::Reading),
        ];
        for (query, route) in routes {
            *calls += 1;
            let hits = route
                .ask(&source, query)
                .map_err(|error| Abstain::LookupFailed(error.to_string()))?;
            for found in &hits {
                let evidence = EntryEvidence::read(
                    source.dictionary,
                    found,
                    Retrieved::Whole {
                        query: query.to_string(),
                    },
                );
                if seen.insert(evidence.id.clone()) {
                    records.push(evidence);
                }
            }
        }
    }

    // Per word: every derivation that reproduces the span with the advisor's reading, and every
    // reading any derivation of it gave before that filter.
    type Word = (Vec<(EntryEvidence, Derived)>, Vec<String>);
    let mut words: BTreeMap<(DictionaryId, Option<i64>, String), Word> = BTreeMap::new();
    for evidence in records {
        if evidence.kind != AnswerKind::OrdinaryDefinition {
            continue;
        }
        let derivations = derivations_of(&evidence, surface, &class);
        if derivations.is_empty() {
            continue;
        }
        let word = words.entry(evidence.id.same_word_key()).or_default();
        for derived in derivations {
            if !word.1.contains(&derived.reading) {
                word.1.push(derived.reading.clone());
            }
            if derived.reading == advised_reading {
                word.0.push((evidence.clone(), derived));
            }
        }
    }
    let surviving: Vec<&Word> = words.values().filter(|word| !word.0.is_empty()).collect();
    let word = match surviving.as_slice() {
        [] => return Err(Abstain::NoDerivation),
        [one] => *one,
        many => return Err(Abstain::CompetingWords(many.len())),
    };

    // One word, one derivation. The same derivation reached through two records of the word — its
    // kanji and kana forms — is one; prefer the record whose headword spelled the stem.
    let mut distinct: Vec<&(EntryEvidence, Derived)> = Vec::new();
    for candidate in &word.0 {
        let same = |other: &&(EntryEvidence, Derived)| {
            other.1.family == candidate.1.family
                && other.1.nasal == candidate.1.nasal
                && other.1.reading == candidate.1.reading
                && other.1.pieces == candidate.1.pieces
        };
        match distinct.iter().position(same) {
            Some(at)
                if candidate.1.stem == StemEvidence::Headword
                    && distinct[at].1.stem != StemEvidence::Headword =>
            {
                distinct[at] = candidate;
            }
            Some(_) => {}
            None => distinct.push(candidate),
        }
    }
    let [(evidence, derived)] = distinct.as_slice() else {
        return Err(Abstain::AmbiguousDerivation);
    };
    let evidence = evidence.clone();
    let derived = derived.clone();

    let lemma_reading = to_hiragana(&evidence.reading);
    let mut token = rejoined(
        run,
        surface,
        start,
        end,
        &evidence,
        &lemma_reading,
        &derived,
        &class,
    );
    // More than one reading of the word derives this span — 怒る is おこる and いかる — and the
    // advisor's picked one. That is exactly the caveat `reading_ambiguous` exists to draw.
    token.reading_ambiguous = word.1.len() > 1;
    // The parts are checked before the token is kept: a repair whose pieces do not spell the
    // source is not a repair.
    let probe = Analysis {
        text: analysis.text.clone(),
        tokens: vec![token.clone()],
        execution: None,
    };
    if let Some(fault) = crate::analyzer::validate_components(&probe).first() {
        return Err(Abstain::ComponentFault(format!("{fault:?}")));
    }
    Ok((
        RepairNote {
            start,
            end,
            family: derived.family,
            nasal: derived.nasal,
            lemma: evidence.term.clone(),
            lemma_reading,
            reading: derived.reading.clone(),
            sequence: evidence.id.sequence,
            stem: derived.stem,
            replaced: run.len() as u32,
        },
        token,
    ))
}

/// Every derivation of `surface` this record supports, under each verb class it asserts that is
/// compatible with the advice's.
fn derivations_of(evidence: &EntryEvidence, surface: &str, advised: &str) -> Vec<Derived> {
    let mut classes = evidence.class_evidence();
    classes.extend(evidence.sense_class());
    classes.sort();
    classes.dedup();
    // The irregular godan codes conjugate in ways `formation` does not model — ある's negative is
    // ない, 問う's te-form 問うて — so a record asserting one derives nothing rather than something
    // wrong under the plain `v5` its rules also carry.
    if classes
        .iter()
        .any(|class| matches!(class.as_str(), "v5aru" | "v5r-i" | "v5u-s"))
    {
        return Vec::new();
    }
    let mut found: Vec<Derived> = Vec::new();
    for class in classes {
        if !(class.starts_with("v5") || class.starts_with("v1"))
            || classes_agree(&class, advised) != ClassAgreement::Compatible
        {
            continue;
        }
        // 行く is the one irregular godan `formation` models, by its specific code; a record's
        // rules say only `v5`, so the advisor's code is what names it.
        let class = if advised.eq_ignore_ascii_case("v5k-s") {
            "v5k-s".to_string()
        } else {
            class
        };
        let mut forms = contraction::derive(
            surface,
            &evidence.term,
            &evidence.reading,
            &class,
            StemEvidence::Headword,
        );
        if evidence.term != evidence.reading {
            forms.extend(contraction::derive(
                surface,
                &evidence.reading,
                &evidence.reading,
                &class,
                StemEvidence::Reading,
            ));
        }
        for form in forms {
            if !found.contains(&form) {
                found.push(form);
            }
        }
    }
    found
}

/// The repaired token. Every field named, as `assemble::composed` does, so a field added to the
/// model forces a decision here too.
#[allow(clippy::too_many_arguments)]
fn rejoined(
    run: &[Token],
    surface: &str,
    start: usize,
    end: usize,
    evidence: &EntryEvidence,
    lemma_reading: &str,
    derived: &Derived,
    class: &str,
) -> Token {
    let (form, neg) = match derived.family {
        ContractionFamily::TeOru => ("Conjunctive (~te)", false),
        ContractionFamily::NegativeConditional => ("Provisional (~eba)", true),
    };
    // The contraction rung, named by what it contracts. `grammar::derivation` ladders a token that
    // records one from this chain — base form, then contraction — rather than from the parts,
    // which are source slices and not complete forms.
    let contracted = match derived.family {
        ContractionFamily::TeOru if derived.pieces[1].reading == "どる" => {
            "Contracted ～でおる (～どる)".to_string()
        }
        ContractionFamily::TeOru => "Contracted ～ておる (～とる)".to_string(),
        ContractionFamily::NegativeConditional => {
            format!("Contracted ～なければ (～{})", derived.pieces[1].reading)
        }
    };
    // The tail's own dictionary form: the contracted おる for `～とる`, the contraction itself for
    // `～なきゃ`, which dictionaries list as one. Never とる, which would look up 取る.
    let [stem, tail] = &derived.pieces;
    let tail_surface = &surface[tail.start..tail.end];
    let tail_lemma = match derived.family {
        ContractionFamily::TeOru => "おる",
        ContractionFamily::NegativeConditional => tail_surface,
    };
    let components = vec![
        part(
            &surface[stem.start..stem.end],
            &evidence.term,
            &stem.reading,
            lemma_reading,
            start + stem.start,
            start + stem.end,
        ),
        part(
            tail_surface,
            tail_lemma,
            &tail.reading,
            tail_lemma,
            start + tail.start,
            start + tail.end,
        ),
    ];
    Token {
        surface: surface.to_string(),
        dict_form: evidence.term.clone(),
        reading: derived.reading.clone(),
        lemma_reading: lemma_reading.to_string(),
        lexical_role: None,
        // As for a grouped unit: senses belong to the lookup the reader triggers, keyed on the
        // dictionary form above.
        senses: Vec::new(),
        conjugation: Some(Conjugation {
            lemma: evidence.term.clone(),
            reading: lemma_reading.to_string(),
            props: vec![
                Prop {
                    pos: class.to_string(),
                    form: form.to_string(),
                    neg,
                    fml: false,
                    via_depth: 1,
                    base_reading: lemma_reading.to_string(),
                    is_base: true,
                },
                Prop {
                    pos: class.to_string(),
                    form: contracted,
                    neg: false,
                    fml: false,
                    via_depth: 0,
                    base_reading: String::new(),
                    is_base: false,
                },
            ],
        }),
        components,
        // Decided later, from the source, by `analyzer::mark_selectable`.
        selectable: false,
        constructions: Vec::new(),
        // The engine's alternatives were for the pieces it split, not for this word.
        alternatives: Vec::new(),
        line_break: run.last().map(|token| token.line_break).unwrap_or_default(),
        start: Some(start),
        end: Some(end),
        // Nothing scored this word; inventing a number is what `confidence` warns against.
        confidence: None,
        romaji: None,
        // Derived from a dictionary record and agreed by the advisor: not a guess.
        reading_unverified: false,
        reading_ambiguous: false,
    }
}

/// One source slice of the repaired word.
fn part(
    surface: &str,
    dict_form: &str,
    reading: &str,
    lemma_reading: &str,
    start: usize,
    end: usize,
) -> Token {
    Token {
        surface: surface.to_string(),
        dict_form: dict_form.to_string(),
        reading: reading.to_string(),
        lemma_reading: lemma_reading.to_string(),
        lexical_role: None,
        senses: Vec::new(),
        conjugation: None,
        components: Vec::new(),
        selectable: false,
        constructions: Vec::new(),
        alternatives: Vec::new(),
        line_break: Default::default(),
        start: Some(start),
        end: Some(end),
        confidence: None,
        romaji: None,
        reading_unverified: false,
        reading_ambiguous: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(surface: &str, start: usize) -> Token {
        Token {
            surface: surface.into(),
            start: Some(start),
            end: Some(start + surface.len()),
            ..Default::default()
        }
    }

    /// No advisor, no repair — and the trigger is still accounted for.
    #[test]
    fn without_an_advisor_every_trigger_abstains() {
        let text = "間違っとる";
        let analysis = Analysis {
            text: text.into(),
            tokens: vec![token("間", 0), token("違っとる", 3)],
            execution: None,
        };
        let (out, account) = repaired(&analysis, &[], None);
        assert_eq!(out, analysis);
        assert_eq!(account.investigations.len(), 1);
        assert_eq!(
            account.investigations[0].outcome,
            Outcome::Abstained(Abstain::AdvisorUnavailable)
        );
    }

    /// A token not ending in a tail is not looked at, and costs nothing.
    #[test]
    fn a_passage_with_no_tail_is_not_investigated() {
        let analysis = Analysis {
            text: "水を飲む".into(),
            tokens: vec![token("水", 0), token("を", 3), token("飲む", 6)],
            execution: None,
        };
        let (out, account) = repaired(&analysis, &[], None);
        assert_eq!(out, analysis);
        assert_eq!(account, Account::default());
    }
}
