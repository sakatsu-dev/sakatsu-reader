//! Combining two analyses of one passage, region by region.
//!
//! # Why regions
//!
//! `Auto` picks one engine for a whole passage. That is the wrong unit: the reported examples
//! include a single sentence where **each engine gives the better answer to a different part of
//! it** — Ichiran groups `なんで` where Lindera splits it into a pronoun and a particle and
//! retrieves a homophone; Lindera holds `怒んなきゃ` together where Ichiran cuts it at `怒ん` and
//! retrieves nothing at all. No choice between the two whole analyses can win that sentence.
//!
//! So the passage is cut at the positions **both** analyses agree on. Those are fixed points, and
//! between two consecutive ones the two either agree or they do not. Each such stretch is one
//! region, and a region is chosen whole: an alternative is always a **complete regional token
//! sequence**, never individual tokens taken from incompatible analyses.
//!
//! # What the rules here are, and are not
//!
//! **The two predicates below are experimental heuristics.** They encode what could be derived
//! from a handful of demonstrated failures, and each rules out a particular mistake rather than
//! establishing that an answer suits its context. A confidently wrong homophone still satisfies
//! "has an answer"; a grouping the dictionary lists can still be wrong here. Neither is an
//! established advantage, and nothing in this module should be read as one.
//!
//! What *is* established is narrower and worth keeping separate: retrieval now records whether an
//! answer addressed the whole token or fell back to a component ([`Retrieved`]), so "the panel is
//! full" and "the panel is about this word" are finally distinguishable. That is what the `神姫`
//! case needed — eleven senses, every one describing `神`.
//!
//! Component fallback is **useful** and stays. What is wrong is presenting it unlabelled.

use crate::dict::index::Dictionary;
use crate::dict::merge::Retrieved;
use crate::grammar::Match;
use crate::{expression, grammar, lookup};
use crate::{Analysis, Token};
use std::collections::BTreeSet;

// Compatibility for the frozen prototype and its diagnostics. Production owns the contract in
// `analyzer` and enforces it in `study::analyse`.
pub use crate::analyzer::{validate_analysis as validate, SpanFault};

/// Which analyzer produced an alternative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Lindera,
    Ichiran,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Lindera => "lindera",
            Engine::Ichiran => "ichiran",
        }
    }
}

/// Why two analyses could not be combined at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotComparable {
    /// They analysed different strings. A hybrid over two texts is not a hybrid.
    DifferentSource,
    /// One of them does not describe its own source.
    Invalid {
        engine: Engine,
        faults: Vec<SpanFault>,
    },
}

/// A stretch of the passage bounded by two positions both analyses cut at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
}

fn within(token: &Token, region: Region) -> bool {
    matches!((token.start, token.end), (Some(start), Some(end))
        if start >= region.start && end <= region.end)
}

/// The positions both analyses cut at, plus the passage's own ends.
fn fixed_points(left: &[Token], right: &[Token], length: usize) -> Vec<usize> {
    let cuts = |tokens: &[Token]| -> BTreeSet<usize> {
        tokens
            .iter()
            .filter_map(|token| Some([token.start?, token.end?]))
            .flatten()
            .collect()
    };
    let mut points: Vec<usize> = cuts(left).intersection(&cuts(right)).copied().collect();
    points.push(0);
    points.push(length);
    points.sort_unstable();
    points.dedup();
    points
}

/// Divide a passage into regions bounded by shared cuts.
///
/// **Both analyses are validated first.** The contract this documents is the one it enforces: same
/// source, grounded spans, in order, non-overlapping, spelling what they claim, and leaving
/// nothing but whitespace uncovered.
pub fn regions(left: &Analysis, right: &Analysis) -> Result<Vec<Region>, NotComparable> {
    if left.text != right.text {
        return Err(NotComparable::DifferentSource);
    }
    for (engine, analysis) in [(Engine::Lindera, left), (Engine::Ichiran, right)] {
        let faults = validate(analysis);
        if !faults.is_empty() {
            return Err(NotComparable::Invalid { engine, faults });
        }
    }
    let points = fixed_points(&left.tokens, &right.tokens, left.text.len());
    Ok(points
        .windows(2)
        .map(|pair| Region {
            start: pair[0],
            end: pair[1],
        })
        .collect())
}

/// What kind of thing the panel's leading answer is.
///
/// **Analyzer-provided senses are their own category**, not a dictionary answer for the whole
/// word. `merge` makes the analyzer's own senses primary when it has any, and only Ichiran
/// supplies them — so treating `inline_source: None` as "the whole token was answered" would
/// silently credit one engine with a verification that never happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Nothing at all: no senses, no name.
    Nothing,
    /// The analyzer's own senses lead. Whether a dictionary corroborates them is a separate
    /// question, recorded in [`TokenAssistance::whole_answered`].
    AnalyzerOnly,
    /// A dictionary's senses lead, retrieved for the token's own dictionary form.
    Dictionary { source: String },
    /// A dictionary's senses lead, retrieved for **a component** — so they describe a part.
    Component { source: String, component: String },
    /// Only a name entry could lead, because nothing else was found.
    NameOnly { source: String },
}

/// One sense as the panel would show it.
///
/// **Identity and content, not a tally.** Two answers from the same dictionary with the same number
/// of senses can mean entirely different things; comparing counts would call them equivalent and
/// retain Lindera without anyone seeing the difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayedSense {
    /// The dictionary entry this sense belongs to, when the source states one.
    pub sequence: Option<i64>,
    pub tags: Vec<String>,
    pub glosses: Vec<String>,
}

/// A recognised expression reachable from a token, with the evidence to judge its scope.
///
/// **The span is the point.** A pattern id alone says an offer exists, not what it covers — and an
/// offer for a smaller construction inside a region is no reason to decline a decision about the
/// region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferEvidence {
    pub pattern_id: String,
    pub expression: String,
    /// The offer's own validated byte span, as `expression::offered` established it.
    pub start: usize,
    pub end: usize,
}

impl OfferEvidence {
    /// Whether this offer explains the whole of `region`, rather than something inside it.
    pub fn covers(&self, region: Region) -> bool {
        self.start <= region.start && self.end >= region.end
    }
}

/// What a reader would actually receive for one token.
#[derive(Debug, Clone)]
pub struct TokenAssistance {
    pub surface: String,
    /// The lookup did not run. **Never conflated with an empty answer**: a dictionary that failed
    /// to open says nothing about whether the analysis was good.
    pub lookup_failed: Option<String>,
    /// The expression lookup did not run.
    ///
    /// Separate from `lookup_failed` and from having no offer. Folding a failure into "no offer"
    /// would let a broken grammar pack make a split candidate look unexplained, and so argue for
    /// merging it.
    pub expression_failed: Option<String>,
    /// The senses the panel would show, in the order it would show them.
    pub displayed: Vec<DisplayedSense>,
    /// The sense the contextual ranker favoured, if it committed to one.
    pub favoured: Option<usize>,
    /// What the leading answer is, and where it came from.
    pub answer: Answer,
    /// Some dictionary answered the token's **own dictionary form**, whatever ended up leading.
    pub whole_answered: bool,
    /// The name this word may be, when one leads or is disclosed.
    pub name: Option<String>,
    /// The reading the merge settled on, which a reader sees above the word.
    pub reading: Option<String>,
    /// A recognised expression the reader can reach from this token — `のに`, `には` today.
    ///
    /// Here because **a split candidate may already explain the construction correctly**. A rule
    /// that merged tokens to obtain an entry the reader could already reach would be claiming an
    /// improvement it did not make.
    pub expression_offer: Option<OfferEvidence>,
    /// Grammar patterns covering this token, by id.
    pub grammar: Vec<String>,
}

impl TokenAssistance {
    /// A token the reader would expect an answer for.
    pub fn expects_an_answer(&self) -> bool {
        self.surface.chars().any(|character| {
            !character.is_ascii_punctuation() && !is_japanese_punctuation(character)
        })
    }

    /// The component this token's leading senses actually describe, when they describe one.
    pub fn component_answer(&self) -> Option<&str> {
        match &self.answer {
            Answer::Component { component, .. } => Some(component),
            _ => None,
        }
    }

    /// Nothing was found, and the lookup did run. **Not** a failure.
    pub fn is_empty_answer(&self) -> bool {
        self.lookup_failed.is_none() && matches!(self.answer, Answer::Nothing)
    }

    /// Any lookup that did not run — word or expression.
    pub fn failure(&self) -> Option<&str> {
        self.lookup_failed
            .as_deref()
            .or(self.expression_failed.as_deref())
    }
}

fn is_japanese_punctuation(character: char) -> bool {
    matches!(
        character,
        '。' | '、' | '！' | '？' | '「' | '」' | '　' | '…' | '・' | '（' | '）' | '〜'
    )
}

/// One engine's complete token sequence for one region, with the assistance it produces.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub engine: Engine,
    pub region: Region,
    pub tokens: Vec<Token>,
    pub assistance: Vec<TokenAssistance>,
}

impl Candidate {
    pub fn surfaces(&self) -> Vec<&str> {
        self.tokens
            .iter()
            .map(|token| token.surface.as_str())
            .collect()
    }

    /// Tokens whose word or expression lookup could not be performed.
    pub fn failures(&self) -> Vec<(&str, &str)> {
        self.assistance
            .iter()
            .filter_map(|part| part.failure().map(|error| (part.surface.as_str(), error)))
            .collect()
    }

    /// Content tokens whose lookup ran and found nothing.
    pub fn unanswered(&self) -> Vec<&str> {
        self.assistance
            .iter()
            .filter(|part| part.is_empty_answer() && part.expects_an_answer())
            .map(|part| part.surface.as_str())
            .collect()
    }

    /// Tokens whose leading senses describe a component rather than the token.
    pub fn component_answers(&self) -> Vec<(&str, &str)> {
        self.assistance
            .iter()
            .filter_map(|part| match &part.answer {
                Answer::Component { component, .. } => {
                    Some((part.surface.as_str(), component.as_str()))
                }
                _ => None,
            })
            .collect()
    }

    /// Expressions already reachable from this candidate's tokens.
    pub fn expression_offers(&self) -> Vec<&OfferEvidence> {
        self.assistance
            .iter()
            .filter_map(|part| part.expression_offer.as_ref())
            .collect()
    }

    /// Whether the reader can already reach an expression explaining **this whole region**.
    ///
    /// An offer for something smaller inside the region does not count. `のに` explained inside a
    /// longer disputed stretch says nothing about whether the stretch should be merged.
    pub fn already_explains_the_region(&self) -> bool {
        self.expression_offers()
            .iter()
            .any(|offer| offer.covers(self.region))
    }

    /// Whether this candidate **is** the region's exact expression, answered as itself.
    ///
    /// Four things, each load-bearing:
    ///
    /// - **One token**, so it covers the region rather than dividing it.
    /// - **Answered** — not `Nothing`.
    /// - **Not through a component** — the `神姫` clause. Eleven senses retrieved through `神` make
    ///   the panel look authoritative about a word they do not describe.
    /// - **Corroborated by a dictionary answering the whole token** (`whole_answered`). Without
    ///   this, an analyzer's own senses would count as verified retrieval of the whole — and only
    ///   one of the two engines supplies any, so the rule would quietly favour it.
    ///
    /// Note what it does **not** ask: whether the token has components. `双星` has them and
    /// qualifies, because its own query succeeded and the fallback never ran.
    pub fn answers_the_whole_region(&self) -> bool {
        self.tokens.len() == 1
            && self.assistance.first().is_some_and(|part| {
                part.failure().is_none()
                    && part.whole_answered
                    && !matches!(part.answer, Answer::Nothing | Answer::Component { .. })
            })
    }

    /// The facts that affect reading, lookup and study behaviour.
    ///
    /// **Not byte-identical metadata.** Two engines differ in provenance, scores and internal
    /// bookkeeping for tokens a reader would experience identically; comparing all of that would
    /// make every region differ and abstention meaningless.
    fn comparable(&self) -> Vec<ComparableToken<'_>> {
        self.tokens
            .iter()
            .zip(&self.assistance)
            .map(|(token, part)| ComparableToken {
                surface: token.surface.as_str(),
                reading: token.reading.as_str(),
                dict_form: token.dict_form.as_str(),
                lemma_reading: token.lemma_reading.as_str(),
                components: token
                    .components
                    .iter()
                    .map(|component| component.dict_form.as_str())
                    .collect(),
                displayed: &part.displayed,
                favoured: part.favoured,
                answer: &part.answer,
                whole_answered: part.whole_answered,
                name: part.name.as_deref(),
                merged_reading: part.reading.as_deref(),
                expression_offer: part.expression_offer.as_ref(),
                grammar: &part.grammar,
            })
            .collect()
    }
}

#[derive(PartialEq)]
struct ComparableToken<'a> {
    surface: &'a str,
    reading: &'a str,
    dict_form: &'a str,
    lemma_reading: &'a str,
    components: Vec<&'a str>,
    /// The senses themselves, in order — **not a count**. Two answers from one dictionary with the
    /// same number of senses routinely mean different things.
    displayed: &'a [DisplayedSense],
    favoured: Option<usize>,
    answer: &'a Answer,
    whole_answered: bool,
    name: Option<&'a str>,
    merged_reading: Option<&'a str>,
    expression_offer: Option<&'a OfferEvidence>,
    grammar: &'a [String],
}

/// An analysis with the sentence-level work that assistance depends on, done once.
pub struct Prepared<'a> {
    pub engine: Engine,
    pub analysis: &'a Analysis,
    patterns: Vec<Match>,
}

impl<'a> Prepared<'a> {
    pub fn new(engine: Engine, analysis: &'a Analysis) -> Prepared<'a> {
        Prepared {
            engine,
            analysis,
            patterns: grammar::patterns(analysis),
        }
    }
}

/// Build one engine's candidate for a region, looking each token up **in its own full analysis**.
///
/// The whole token list goes to retrieval, not the region's slice: contextual ordering reads
/// neighbouring tokens, and a candidate evaluated without its neighbours is not what would ship.
pub fn candidate(
    prepared: &Prepared<'_>,
    region: Region,
    dictionaries: &[Dictionary],
) -> Candidate {
    let mut tokens = Vec::new();
    let mut assistance = Vec::new();
    for (index, token) in prepared.analysis.tokens.iter().enumerate() {
        if !within(token, region) {
            continue;
        }
        tokens.push(token.clone());
        assistance.push(assistance_for(
            prepared.analysis,
            &prepared.patterns,
            index,
            dictionaries,
        ));
    }
    Candidate {
        engine: prepared.engine,
        region,
        tokens,
        assistance,
    }
}

fn assistance_for(
    analysis: &Analysis,
    patterns: &[Match],
    index: usize,
    dictionaries: &[Dictionary],
) -> TokenAssistance {
    let tokens = &analysis.tokens;
    let token = &tokens[index];
    let covering: Vec<Match> = patterns
        .iter()
        .filter(|found| index >= found.first_token && index < found.last_token_end)
        .cloned()
        .collect();
    let grammar: Vec<String> = covering
        .iter()
        .map(|found| found.pattern_id.clone())
        .collect();

    // **A failed expression lookup is a failure, not an absence.** Swallowed with `.ok()`, a
    // broken grammar pack would make every split candidate look unexplained — and the merge rule
    // stands down precisely when a split already explains the region, so the failure would argue
    // for merging. `Ok(None)` stays distinguishable: that is a real answer.
    let (expression_offer, expression_failed) =
        match expression::offered(tokens, index, &covering, dictionaries) {
            Ok(Some(offer)) => (
                Some(OfferEvidence {
                    pattern_id: offer.pattern_id,
                    expression: offer.expression,
                    start: offer.start,
                    end: offer.end,
                }),
                None,
            ),
            Ok(None) => (None, None),
            Err(error) => (None, Some(format!("{error:?}"))),
        };

    let looked = match lookup::with_evidence(tokens, index, dictionaries) {
        Ok(looked) => looked,
        Err(error) => {
            // **A failure, not an absence.** Recorded so no rule can read a broken dictionary as
            // evidence that this engine segmented badly.
            return TokenAssistance {
                surface: token.surface.clone(),
                lookup_failed: Some(format!("{error:?}")),
                expression_failed,
                displayed: Vec::new(),
                favoured: None,
                answer: Answer::Nothing,
                whole_answered: false,
                name: None,
                reading: None,
                expression_offer,
                grammar,
            };
        }
    };
    let word = &looked.view.word;
    let whole_answered = looked
        .evidence
        .iter()
        .any(|source| matches!(source.retrieved, Retrieved::Whole { .. }));
    let scope_of = |title: &str| {
        looked
            .evidence
            .iter()
            .find(|source| source.title == title)
            .map(|source| source.retrieved.clone())
    };
    let answer = if word.inline.is_empty() {
        match word.name.as_ref().filter(|hit| hit.leads) {
            Some(hit) => Answer::NameOnly {
                source: hit.source.clone(),
            },
            None => Answer::Nothing,
        }
    } else {
        match &word.inline_source {
            // A promoted dictionary definition leads; which query reached it decides what it
            // describes.
            Some(source) => match scope_of(source) {
                Some(Retrieved::Component { query }) => Answer::Component {
                    source: source.clone(),
                    component: query,
                },
                _ => Answer::Dictionary {
                    source: source.clone(),
                },
            },
            // The analyzer's own senses lead. Its own category, deliberately.
            None => Answer::AnalyzerOnly,
        }
    };
    TokenAssistance {
        surface: token.surface.clone(),
        lookup_failed: None,
        expression_failed,
        displayed: word
            .inline
            .iter()
            .map(|sense| DisplayedSense {
                sequence: sense.sequence,
                tags: sense.tags.clone(),
                glosses: sense.glosses.clone(),
            })
            .collect(),
        favoured: looked.view.ranking.favoured,
        answer,
        whole_answered,
        name: word
            .name
            .as_ref()
            .map(|hit| format!("{} [{}]", hit.glosses.join(", "), hit.source)),
        reading: word.reading.clone(),
        expression_offer,
        grammar,
    }
}

/// The assistance a finished passage produces, **measured against that passage**.
///
/// **Not the same thing as a candidate's assistance, and not interchangeable with it.** A
/// candidate is measured inside the analysis it came from, because that is the context its engine
/// saw and therefore what explains why a rule fired. The assembled passage has different
/// neighbours — that is the whole point of splicing — so contextual ranking, grammar matching and
/// expression offers must all be recomputed here. Reporting a candidate's assistance as the
/// reader's would describe a passage that was never assembled.
pub fn assistance_of(analysis: &Analysis, dictionaries: &[Dictionary]) -> Vec<TokenAssistance> {
    let patterns = grammar::patterns(analysis);
    (0..analysis.tokens.len())
        .map(|index| assistance_for(analysis, &patterns, index, dictionaries))
        .collect()
}

/// What the selector decided for one region, and on what grounds.
///
/// **Preference, agreement, ignorance and breakage are four different things.** A selector that
/// returned Lindera and called it equivalence would satisfy a requirement to "keep Lindera where
/// evidence is insufficient" while deciding nothing at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// An experimental rule supports Lindera's sequence here.
    PreferLindera { rule: &'static str },
    /// An experimental rule supports Ichiran's.
    PreferIchiran { rule: &'static str },
    /// The two are the same in every respect a reader would notice. Not a decision.
    Equivalent,
    /// They differ and **no rule discriminates**. Lindera is kept *by policy*, not because it won.
    ///
    /// Never a Lindera rescue, and never counted as one.
    Abstain { reason: &'static str },
    /// A word or expression lookup did not run, so this region was **not evaluated**.
    ///
    /// Separate from abstention: abstention is a judgment about two answers, this is the absence
    /// of one of them. Lindera is retained so a passage still assembles, and the region is
    /// **counted in its own category and excluded from preference and equivalence metrics** — it
    /// is reported, not discarded.
    NotEvaluated { reason: &'static str },
}

impl Choice {
    /// Which sequence to assemble from. Anything but a preference for Ichiran keeps Lindera.
    pub fn engine(&self) -> Engine {
        match self {
            Choice::PreferIchiran { .. } => Engine::Ichiran,
            _ => Engine::Lindera,
        }
    }

    /// Whether a rule actually decided this, as opposed to agreement, policy or breakage.
    pub fn is_a_preference(&self) -> bool {
        matches!(
            self,
            Choice::PreferLindera { .. } | Choice::PreferIchiran { .. }
        )
    }
}

/// A named preference predicate: which engine this rule supports, if either.
type Rule = (&'static str, fn(&Candidate, &Candidate) -> Option<Engine>);

/// The experimental heuristics, in the order they are consulted.
///
/// **They decide boundary questions only.** Where the two analyses cut the region identically,
/// neither rule is competent — see [`decide`].
const RULES: &[Rule] = &[
    ("leaves-no-content-token-unanswered", no_unanswered_content),
    ("exact-expression-for-the-region", exact_expression),
];

/// The frozen identity of this rule set.
///
/// **Every count, decision and artifact is only about this version.** Adding a rule, changing a
/// clause or reordering [`RULES`] changes what the selector does, so results gathered under one
/// version are not comparable with another's — and a review packet whose rules have since moved is
/// a review of nothing. Bump it in the same commit as any change here.
pub const RULESET: &str = "hybrid-heuristics-v1-2026-09-14";

/// **Prefer the candidate that leaves no content token without any answer.**
///
/// Derived from `怒んなきゃ`: Lindera holds the contracted negative together and reaches 怒る, while
/// Ichiran cuts it at `怒ん` and reaches nothing under any query.
///
/// **What it does not establish.** Having an answer is not having the right one: `なん` is answered
/// — with 難, a homophone — and this rule would happily prefer it over a candidate that found
/// nothing. It removes one failure mode; it does not measure appropriateness.
///
/// *Counterexample it is built against:* **punctuation**, unanswered in both analyses of every
/// sentence here. Without the content-token filter the rule fires wherever a mark attaches
/// differently, deciding regions in which nothing is wrong.
fn no_unanswered_content(lindera: &Candidate, ichiran: &Candidate) -> Option<Engine> {
    match (
        lindera.unanswered().is_empty(),
        ichiran.unanswered().is_empty(),
    ) {
        (true, false) => Some(Engine::Lindera),
        (false, true) => Some(Engine::Ichiran),
        _ => None,
    }
}

/// **Prefer the candidate that covers the whole region with one token answered as itself.**
///
/// Derived from `なんで` and `あなたたち`, where answering the pieces separately produced 難 and 太刀.
/// It fires for Lindera too, on `裁いてやる`.
///
/// **What it does not establish.** That a grouping is listed in a dictionary does not make it right
/// here; `裁いてやる` is exactly that doubt, and the rule prefers it anyway.
///
/// *Counterexample 1 —* **`神姫` under Ichiran**: one token, eleven senses, every one retrieved
/// through `神`. A rule asking only "is it answered" prefers it, which is the reported failure.
///
/// *Counterexample 2 —* **`双星` under Lindera**: it *has* components and would be wrongly rejected
/// by a rule asking whether components exist; its own query succeeds, so the fallback never runs.
///
/// *Counterexample 3 —* a joined candidate whose senses come only from the analyzer.
/// Corroboration by a whole-token dictionary answer is required, or only the engine that supplies
/// inline senses could ever satisfy this.
///
/// **And it must not claim an expression the reader can already reach.** Where the split candidate
/// already offers an expression **covering this whole region**, merging gains nothing and the rule
/// stands down. The coverage test is not incidental: an offer is evidence about its own span, and
/// one explaining a smaller construction inside a disputed stretch says nothing about whether the
/// stretch should be merged. Checking only that *some* offer exists would let a `のに` inside a
/// longer region suppress a decision about the region.
fn exact_expression(lindera: &Candidate, ichiran: &Candidate) -> Option<Engine> {
    match (
        lindera.answers_the_whole_region()
            && ichiran.tokens.len() > 1
            && !ichiran.already_explains_the_region(),
        ichiran.answers_the_whole_region()
            && lindera.tokens.len() > 1
            && !lindera.already_explains_the_region(),
    ) {
        (true, false) => Some(Engine::Lindera),
        (false, true) => Some(Engine::Ichiran),
        _ => None,
    }
}

/// Choose between two candidates for one region.
///
/// **Agreement is checked over what a reader experiences, not over surfaces.** Identical
/// boundaries with different readings, lemmas, components or assistance are *not* equivalent —
/// `神姫` is cut the same way by both engines and answered completely differently — and calling
/// them equivalent would retain Lindera automatically while hiding exactly the complementary
/// behaviour this module exists to find.
pub fn decide(lindera: &Candidate, ichiran: &Candidate) -> Choice {
    // Word **or** expression: either failing leaves the comparison without a fact it needs.
    if !lindera.failures().is_empty() || !ichiran.failures().is_empty() {
        return Choice::NotEvaluated {
            reason: "a word or expression lookup did not run; this region is unevaluated, not                      decided",
        };
    }
    if lindera.comparable() == ichiran.comparable() {
        return Choice::Equivalent;
    }
    if lindera.surfaces() == ichiran.surfaces() {
        // **Same boundaries, different answers.** No rule here is about anything but boundaries,
        // and preferring on availability alone would credit whichever engine's word happens to sit
        // in a dictionary rather than whichever analysis is better.
        return Choice::Abstain {
            reason: "segmentation agrees; the difference is in readings or assistance, which no \
                     rule settles",
        };
    }
    for (name, rule) in RULES {
        match rule(lindera, ichiran) {
            Some(Engine::Lindera) => return Choice::PreferLindera { rule: name },
            Some(Engine::Ichiran) => return Choice::PreferIchiran { rule: name },
            None => {}
        }
    }
    Choice::Abstain {
        reason: "no rule discriminates; lindera retained by policy",
    }
}

/// One region's decision, with both alternatives kept so the competing analysis stays reachable.
#[derive(Debug, Clone)]
pub struct Decision {
    pub region: Region,
    pub choice: Choice,
    pub lindera: Candidate,
    pub ichiran: Candidate,
}

impl Decision {
    /// The sequence this decision contributes to the assembled passage.
    pub fn chosen(&self) -> &[Token] {
        match self.choice.engine() {
            Engine::Lindera => &self.lindera.tokens,
            Engine::Ichiran => &self.ichiran.tokens,
        }
    }
}

/// Decide every region of a passage, and assemble the result.
///
/// **The pipeline is not applied again.** Both analyses arrive already displayed, and this splices
/// finished regional sequences; running composition or reading correction over the join would
/// apply it twice to every token.
pub fn combine(
    lindera: &Analysis,
    ichiran: &Analysis,
    dictionaries: &[Dictionary],
) -> Result<(Vec<Decision>, Analysis), NotComparable> {
    let regions = regions(lindera, ichiran)?;
    let left = Prepared::new(Engine::Lindera, lindera);
    let right = Prepared::new(Engine::Ichiran, ichiran);
    let mut decisions = Vec::new();
    let mut tokens = Vec::new();
    for region in regions {
        let first = candidate(&left, region, dictionaries);
        let second = candidate(&right, region, dictionaries);
        let choice = decide(&first, &second);
        let decision = Decision {
            region,
            choice,
            lindera: first,
            ichiran: second,
        };
        tokens.extend_from_slice(decision.chosen());
        decisions.push(decision);
    }
    Ok((
        decisions,
        Analysis {
            execution: None,
            text: lindera.text.clone(),
            tokens,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grounded token spelling `surface` at `start`.
    fn token(surface: &str, start: usize) -> Token {
        Token {
            surface: surface.to_string(),
            dict_form: surface.to_string(),
            reading: surface.to_string(),
            lemma_reading: surface.to_string(),
            start: Some(start),
            end: Some(start + surface.len()),
            ..Token::default()
        }
    }

    fn analysis(text: &str, tokens: Vec<Token>) -> Analysis {
        Analysis {
            execution: None,
            text: text.to_string(),
            tokens,
        }
    }

    /// A candidate assembled by hand, so the selector can be tested without a dictionary.
    fn candidate_of(engine: Engine, parts: Vec<(Token, TokenAssistance)>) -> Candidate {
        let region = Region {
            start: parts
                .first()
                .and_then(|(token, _)| token.start)
                .unwrap_or(0),
            end: parts.last().and_then(|(token, _)| token.end).unwrap_or(0),
        };
        Candidate {
            engine,
            region,
            tokens: parts.iter().map(|(token, _)| token.clone()).collect(),
            assistance: parts.into_iter().map(|(_, part)| part).collect(),
        }
    }

    fn sense(sequence: i64, gloss: &str) -> DisplayedSense {
        DisplayedSense {
            sequence: Some(sequence),
            tags: vec!["noun".into()],
            glosses: vec![gloss.to_string()],
        }
    }

    fn base(surface: &str) -> TokenAssistance {
        TokenAssistance {
            surface: surface.to_string(),
            lookup_failed: None,
            expression_failed: None,
            displayed: Vec::new(),
            favoured: None,
            answer: Answer::Nothing,
            whole_answered: false,
            name: None,
            reading: Some(surface.to_string()),
            expression_offer: None,
            grammar: Vec::new(),
        }
    }

    fn answered(surface: &str) -> TokenAssistance {
        TokenAssistance {
            displayed: vec![sense(1, "a"), sense(2, "b"), sense(3, "c")],
            answer: Answer::Dictionary {
                source: "Jitendex".into(),
            },
            whole_answered: true,
            ..base(surface)
        }
    }

    fn empty(surface: &str) -> TokenAssistance {
        base(surface)
    }

    fn broken(surface: &str) -> TokenAssistance {
        TokenAssistance {
            lookup_failed: Some("the dictionary could not be read".into()),
            ..base(surface)
        }
    }

    fn offer(pattern_id: &str, expression: &str, start: usize, end: usize) -> OfferEvidence {
        OfferEvidence {
            pattern_id: pattern_id.to_string(),
            expression: expression.to_string(),
            start,
            end,
        }
    }

    /// **A dictionary that failed is not evidence about an analysis.**
    ///
    /// The failure arrives looking exactly like an empty answer — no senses, no name — and the
    /// first rule prefers whichever candidate has fewer of those. Left conflated, a broken
    /// dictionary would hand the other engine a "preference" it did nothing to earn, on a region
    /// nobody evaluated.
    #[test]
    fn a_failed_lookup_is_not_evaluated_rather_than_decided() {
        let left = candidate_of(
            Engine::Lindera,
            vec![(token("怒んなきゃ", 0), broken("怒んなきゃ"))],
        );
        let right = candidate_of(
            Engine::Ichiran,
            vec![
                (token("怒ん", 0), answered("怒ん")),
                (token("なきゃ", 6), answered("なきゃ")),
            ],
        );
        let choice = decide(&left, &right);
        assert!(
            matches!(choice, Choice::NotEvaluated { .. }),
            "expected an unevaluated region, got {choice:?}"
        );
        assert!(!choice.is_a_preference(), "and it is never counted as one");
        // Lindera is retained so a passage still assembles — but nothing was decided.
        assert_eq!(choice.engine(), Engine::Lindera);

        // The control: with the lookup working, the same shapes *are* decided, and by the rule
        // that reads unanswered content. Without this, the assertion above would also pass if
        // `decide` had simply stopped working.
        let working = candidate_of(
            Engine::Lindera,
            vec![(token("怒んなきゃ", 0), answered("怒んなきゃ"))],
        );
        let stranded = candidate_of(
            Engine::Ichiran,
            vec![
                (token("怒ん", 0), empty("怒ん")),
                (token("なきゃ", 6), answered("なきゃ")),
            ],
        );
        assert_eq!(
            decide(&working, &stranded),
            Choice::PreferLindera {
                rule: "leaves-no-content-token-unanswered"
            }
        );
    }

    /// **Same boundaries, different answers, and no rule for it.**
    ///
    /// Every rule here is about where the cuts fall. When both engines cut identically, a
    /// difference in what the dictionaries happen to hold is not evidence that either *analysis*
    /// is better — it is evidence about the dictionaries. Preferring on it would credit an engine
    /// for a coincidence.
    ///
    /// This is the `神姫` shape: one token either way, wildly different panels.
    #[test]
    fn availability_alone_does_not_decide_a_region() {
        let left = candidate_of(Engine::Lindera, vec![(token("神姫", 0), empty("神姫"))]);
        let mut rich = answered("神姫");
        rich.displayed = (1..=11).map(|n| sense(n, "god")).collect();
        let right = candidate_of(Engine::Ichiran, vec![(token("神姫", 0), rich)]);

        let choice = decide(&left, &right);
        assert!(
            matches!(choice, Choice::Abstain { .. }),
            "expected an abstention, got {choice:?}"
        );
        assert!(!choice.is_a_preference());
        assert_ne!(
            choice,
            Choice::Equivalent,
            "identical surfaces are not equivalence: the reader sees different things"
        );
    }

    /// Equivalence is reserved for candidates a reader could not tell apart.
    #[test]
    fn equivalence_means_the_reader_would_notice_nothing() {
        let same = || candidate_of(Engine::Lindera, vec![(token("双星", 0), answered("双星"))]);
        let mut other = same();
        other.engine = Engine::Ichiran;
        assert_eq!(decide(&same(), &other), Choice::Equivalent);

        // Change only the reading — same boundaries, same senses — and it is no longer equivalent.
        let mut different = same();
        different.engine = Engine::Ichiran;
        different.tokens[0].reading = "ふたぼし".into();
        assert!(matches!(
            decide(&same(), &different),
            Choice::Abstain { .. }
        ));
    }

    /// **A merge that buys nothing the reader could not already reach is not an improvement.**
    ///
    /// `expression_offer` is why this can be checked at all: the split candidate already leads the
    /// reader to the construction's own entry, so joining it gains no assistance.
    #[test]
    fn a_split_that_already_explains_the_expression_is_not_overruled() {
        let joined = candidate_of(Engine::Ichiran, vec![(token("のに", 0), answered("のに"))]);
        let mut first = answered("の");
        // The offer spans the whole region, 0..6 — which is what makes it relevant to it.
        first.expression_offer = Some(offer("noni", "のに", 0, 6));
        let split = candidate_of(
            Engine::Lindera,
            vec![(token("の", 0), first), (token("に", 3), answered("に"))],
        );
        assert!(
            matches!(decide(&split, &joined), Choice::Abstain { .. }),
            "the expression is already reachable from the split, so merging claims nothing"
        );

        // Without the offer, the same shapes reach the exact-expression rule.
        let mut without = split.clone();
        without.assistance[0].expression_offer = None;
        assert_eq!(
            decide(&without, &joined),
            Choice::PreferIchiran {
                rule: "exact-expression-for-the-region"
            }
        );
    }

    /// **Analyzer senses are not a dictionary answer for the whole word.**
    ///
    /// Only one of the two engines supplies inline senses. A rule that accepted them as proof the
    /// whole token was retrieved would prefer that engine wherever it spoke at all, which is a
    /// property of the adapter rather than of the analysis.
    #[test]
    fn analyzer_senses_alone_do_not_answer_the_whole_region() {
        let mut inline = answered("なんで");
        inline.answer = Answer::AnalyzerOnly;
        inline.whole_answered = false;
        let uncorroborated = candidate_of(Engine::Ichiran, vec![(token("なんで", 0), inline)]);
        assert!(!uncorroborated.answers_the_whole_region());

        // Corroborated by a dictionary answering the token's own form, it qualifies.
        let mut corroborated = uncorroborated.clone();
        corroborated.assistance[0].whole_answered = true;
        assert!(corroborated.answers_the_whole_region());

        // And a component answer never qualifies, however full the panel.
        let mut component = corroborated.clone();
        component.assistance[0].displayed = (1..=11).map(|n| sense(n, "god")).collect();
        component.assistance[0].answer = Answer::Component {
            source: "Jitendex".into(),
            component: "神".into(),
        };
        assert!(!component.answers_the_whole_region());
    }

    /// **Same dictionary, same number of senses, different meanings.**
    ///
    /// The comparison used to be a count, so this pair was `Equivalent` and Lindera was retained
    /// silently. Nothing about the token fields differs here — same surface, reading, dictionary
    /// form, same source, same three senses by arity — and a reader would be told two unrelated
    /// things.
    #[test]
    fn equal_sense_counts_are_not_equal_answers() {
        let left = candidate_of(Engine::Lindera, vec![(token("たち", 0), answered("たち"))]);

        let mut other = answered("たち");
        other.displayed = vec![
            sense(1_408_340, "long sword"),
            sense(1_416_220, "pluralizing suffix"),
            sense(1_551_240, "departure"),
        ];
        let right = candidate_of(Engine::Ichiran, vec![(token("たち", 0), other)]);

        assert_eq!(
            left.assistance[0].displayed.len(),
            right.assistance[0].displayed.len(),
            "the control is only a control if the counts really do match"
        );
        assert_eq!(left.assistance[0].answer, right.assistance[0].answer);
        assert!(
            matches!(decide(&left, &right), Choice::Abstain { .. }),
            "different senses are different assistance, whatever they number"
        );

        // Order alone is a difference too: the panel leads with whatever comes first.
        let mut reordered = right.clone();
        reordered.assistance[0].displayed.reverse();
        assert!(matches!(decide(&right, &reordered), Choice::Abstain { .. }));

        // And so is which sense the context favoured, with the list identical.
        let mut favoured = right.clone();
        favoured.assistance[0].favoured = Some(1);
        assert!(matches!(decide(&right, &favoured), Choice::Abstain { .. }));
    }

    /// **A failed expression lookup must not read as "nothing to explain".**
    ///
    /// The exact combination that matters: the word lookup succeeds, so the region looks perfectly
    /// evaluable, while the expression lookup failed. Left as `None`, the split candidate appears
    /// to offer no explanation and the merge rule fires — a preference argued from a broken
    /// grammar pack.
    #[test]
    fn a_failed_expression_lookup_is_not_an_absent_offer() {
        let joined = candidate_of(Engine::Ichiran, vec![(token("のに", 0), answered("のに"))]);

        let mut first = answered("の");
        first.expression_failed = Some("the pack could not be read".into());
        let split = candidate_of(
            Engine::Lindera,
            vec![(token("の", 0), first), (token("に", 3), answered("に"))],
        );

        assert!(
            split.assistance[0].lookup_failed.is_none(),
            "the word lookup succeeded — this is the combination that used to slip through"
        );
        let choice = decide(&split, &joined);
        assert!(
            matches!(choice, Choice::NotEvaluated { .. }),
            "expected an unevaluated region, got {choice:?}"
        );
        assert!(!choice.is_a_preference());

        // **`Ok(None)` stays a real answer.** With no offer and no failure, the same shapes reach
        // the rule and it decides — so the assertion above is about the failure, not about the
        // absence of an offer.
        let mut genuinely_none = split.clone();
        genuinely_none.assistance[0].expression_failed = None;
        assert_eq!(
            decide(&genuinely_none, &joined),
            Choice::PreferIchiran {
                rule: "exact-expression-for-the-region"
            }
        );
    }

    /// **An offer explains its own span, not everything around it.**
    ///
    /// Here the region runs 0..12 and the split candidate carries an offer covering only 0..6. The
    /// smaller construction being explained says nothing about whether the larger stretch should
    /// be merged, so it must not suppress the decision.
    #[test]
    fn an_offer_for_a_subexpression_does_not_explain_the_region() {
        let joined = candidate_of(
            Engine::Ichiran,
            vec![(token("のにさえ", 0), answered("のにさえ"))],
        );

        let mut inner = answered("の");
        inner.expression_offer = Some(offer("noni", "のに", 0, 6));
        let split = candidate_of(
            Engine::Lindera,
            vec![
                (token("の", 0), inner),
                (token("に", 3), answered("に")),
                (token("さえ", 6), answered("さえ")),
            ],
        );
        assert_eq!(split.region, Region { start: 0, end: 12 });
        assert!(
            !split.expression_offers().is_empty(),
            "there is an offer — the old check would have stopped here"
        );
        assert!(
            !split.already_explains_the_region(),
            "but it covers 0..6 of a 0..12 region"
        );
        assert_eq!(
            decide(&split, &joined),
            Choice::PreferIchiran {
                rule: "exact-expression-for-the-region"
            },
            "a subexpression must not suppress a decision about the whole region"
        );

        // Widen the same offer to the whole region and the rule stands down, as it should.
        let mut covering = split.clone();
        covering.assistance[0].expression_offer = Some(offer("noni", "のにさえ", 0, 12));
        assert!(covering.already_explains_the_region());
        assert!(matches!(decide(&covering, &joined), Choice::Abstain { .. }));
    }

    /// **The span contract is enforced, not merely documented.**
    ///
    /// Each fault would otherwise reach assembly as a wrong answer: an ungrounded token silently
    /// drops out of every region, an overlapping one is spliced twice, and uncovered content
    /// disappears from a passage that still looks complete.
    #[test]
    fn an_analysis_that_does_not_tile_its_source_is_rejected() {
        let good = analysis("双星神姫", vec![token("双星", 0), token("神姫", 6)]);
        assert!(validate(&good).is_empty());

        // An ungrounded token is two faults, not one: it claims no position, **and** the source
        // it should have covered is then covered by nothing. The second is the one that would
        // have shown up as text quietly missing from an assembled passage.
        let mut ungrounded = good.clone();
        ungrounded.tokens[1].start = None;
        assert!(matches!(
            validate(&ungrounded).as_slice(),
            [
                SpanFault::Ungrounded { index: 1, .. },
                SpanFault::Uncovered {
                    start: 6,
                    end: 12,
                    ..
                }
            ]
        ));

        let mut lying = good.clone();
        lying.tokens[1].surface = "神様".into();
        assert!(matches!(
            validate(&lying).as_slice(),
            [SpanFault::SurfaceDisagrees { index: 1, .. }]
        ));

        // Overlap reports itself **and** its consequence: the second token runs past the first,
        // and what follows it is then left uncovered. Both are true, and asserting only the
        // first would let the rest of the passage go missing unremarked.
        let overlapping = analysis("双星神姫", vec![token("双星", 0), token("星神", 3)]);
        assert!(matches!(
            validate(&overlapping).as_slice(),
            [
                SpanFault::OutOfOrder { index: 1, .. },
                SpanFault::Uncovered { .. }
            ]
        ));

        let short = analysis("双星神姫", vec![token("双星", 0)]);
        assert!(matches!(
            validate(&short).as_slice(),
            [SpanFault::Uncovered { start: 6, .. }]
        ));

        // Whitespace is not content: a gap over it is how every real analysis looks.
        let spaced = analysis("双星 神姫", vec![token("双星", 0), token("神姫", 7)]);
        assert!(validate(&spaced).is_empty());
    }

    /// **An invalid analysis stops the hybrid, rather than quietly shrinking it.**
    #[test]
    fn combining_refuses_an_analysis_that_fails_its_contract() {
        let good = analysis("双星神姫", vec![token("双星", 0), token("神姫", 6)]);
        let mut bad = good.clone();
        bad.tokens[1].start = None;

        let NotComparable::Invalid { engine, faults } = regions(&good, &bad).unwrap_err() else {
            panic!("expected the invalid analysis to be named");
        };
        assert_eq!(engine, Engine::Ichiran, "and to say which one it was");
        assert!(
            faults.contains(&SpanFault::Ungrounded {
                index: 1,
                surface: "神姫".into(),
            }),
            "the ungrounded token would otherwise vanish from an apparently successful result:              {faults:?}"
        );

        let elsewhere = analysis("別の文", vec![token("別の文", 0)]);
        assert_eq!(
            regions(&good, &elsewhere).unwrap_err(),
            NotComparable::DifferentSource
        );
    }

    /// Regions are cut only at positions **both** analyses assert.
    #[test]
    fn regions_are_bounded_by_shared_cuts() {
        let left = analysis(
            "なんであたし",
            vec![token("なん", 0), token("で", 6), token("あたし", 9)],
        );
        let right = analysis("なんであたし", vec![token("なんで", 0), token("あたし", 9)]);
        assert_eq!(
            regions(&left, &right).expect("comparable"),
            vec![Region { start: 0, end: 9 }, Region { start: 9, end: 18 }],
            "the disputed stretch is one region, not one per piece"
        );
    }
}
