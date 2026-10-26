//! Putting the chosen units into the token sequence the reader sees, and checking the result.
//!
//! # One path, for the reader and for measurement
//!
//! [`assembled`] is the only way a grouping reaches anything. Production calls it from
//! [`crate::study::analyse`]; an evaluation that wants to measure grouping calls the same
//! function. A measurement taken on a path the reader does not use measures something else, and
//! this project has had that bug before — which is why `study::analyse` and `study::study` are a
//! strict prefix of one another rather than two similar sequences.
//!
//! # Why the display pipeline does not run again
//!
//! `Pipeline::display` composes tokens, joins names and corrects readings. It has already run
//! over the baseline, and running it again over the assembled sequence would ask it to compose
//! what composition already produced. What *does* need recomputing after assembly — breakdowns,
//! patterns, sentence ranges, word keys, statuses, ruby — is recomputed by
//! [`crate::study::from_analysis`], which exists for exactly this: a token sequence assembled
//! from regions, handed on without a second pipeline pass. The hybrid already uses it this way.
//!
//! So the unit token is built here, completely, rather than left for a later pass to fill in.
//!
//! # Validation is not optional and not later
//!
//! `from_analysis` validates nothing, and `analyse` validates *before* this runs. An assembled
//! sequence that did not tile its source, or whose token said one thing while the text said
//! another, would therefore reach the reader unchecked. So the same span contract is applied to
//! the assembled tokens, and **a failure keeps the baseline** — the reader is left where they
//! were rather than shown a passage nothing verified.

use crate::analyzer::{validate_analysis, SpanFault};
use crate::dict::index::Dictionary;
use crate::model::{Analysis, Conjugation, Token};

use super::candidate::{Candidate, Origin};
use super::generate::Generated;
use super::select::{Decision, Outcome, Route};

/// What became of a passage: three outcomes, never two.
///
/// **A baseline is not a baseline.** "Nothing was eligible" and "units were formed and then
/// thrown away because the assembled sequence failed its own span contract" both leave the reader
/// with the analyzer's tokens, and they are opposite findings: the first is the ordinary answer
/// to a passage with nothing to group, the second is a defect here. Collapsing them into one
/// `is_baseline()` made a bug indistinguishable from a quiet day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Units were formed and published.
    Grouped,
    /// Nothing satisfied a route. The baseline stands, and that is the answer.
    Abstained,
    /// Units were formed, the assembled sequence failed validation, and the baseline was kept.
    /// **Always a defect**, never a property of the passage.
    Discarded,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Grouped => "grouped",
            Status::Abstained => "abstained",
            Status::Discarded => "discarded",
        }
    }
}

/// One grouping that reached the reader.
///
/// **Self-contained.** A decision indexes into the candidate list it was made over, which is no
/// use to anything holding the result afterwards, so what was decided is recorded by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub start: usize,
    pub end: usize,
    pub surface: String,
    /// The dictionary form the unit is looked up and stored under.
    pub dict_form: String,
    pub route: Route,
    /// Which mechanism proposed it — a whole-form hit, a recovered inflection, a construction.
    pub origin: Origin,
}

/// One region left as the analyzer segmented it, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abstention {
    pub start: usize,
    pub end: usize,
    pub reason: &'static str,
    /// How many validated interpretations were available but not chosen between.
    pub alternatives: usize,
}

/// What assembly produced, and what it refused.
#[derive(Debug, Clone)]
pub struct Assembly {
    /// The token sequence to publish. **The baseline itself** when nothing was chosen, or when
    /// the assembled sequence failed its own span contract.
    pub analysis: Analysis,
    /// Indices into [`Assembly::analysis`] of the tokens assembly created.
    pub units: Vec<usize>,
    /// What was grouped, in source order. **Diagnostics, by value**, so a caller can report every
    /// automatic decision without holding the candidate list it was made over.
    pub applied: Vec<Applied>,
    /// What was left alone, in source order. An abstention is an outcome and is reported as one.
    pub abstentions: Vec<Abstention>,
    /// Why the assembled sequence was discarded, if it was. Empty on the ordinary path, and a
    /// **defect report** rather than a refusal — every one of these is a bug here, not a property
    /// of the passage.
    pub faults: Vec<SpanFault>,
    /// Groupings that were chosen but could not be applied, with why. A decision that cannot be
    /// carried out is not silently dropped.
    pub unapplied: Vec<(usize, usize, &'static str)>,
    /// Which of the three things happened.
    pub status: Status,
}

impl Assembly {
    /// Whether the reader is seeing the baseline unchanged. **Says nothing about why** — use
    /// [`Assembly::status`] for that.
    pub fn is_baseline(&self) -> bool {
        self.units.is_empty()
    }

    /// Whether anything here needs a person to look at it.
    pub fn is_sound(&self) -> bool {
        self.status != Status::Discarded && self.faults.is_empty() && self.unapplied.is_empty()
    }

    /// One line for a log or a report: what happened, and what went wrong if anything did.
    pub fn summary(&self) -> String {
        format!(
            "{}: {} unit(s), {} abstention(s), {} fault(s), {} unapplied",
            self.status.name(),
            self.applied.len(),
            self.abstentions.len(),
            self.faults.len(),
            self.unapplied.len()
        )
    }
}

/// The conjugation a composed unit carries.
///
/// **The head's form, the unit's lemma.** かっこよかった is a past form — that is what the head
/// constituent よかった was analysed as, and joining tokens does not change which form the word is
/// in. What joining changes is *which word* is in it: no longer よい but 格好いい, read かっこいい.
///
/// # What this is and is not read by
///
/// Not the breakdown. [`crate::grammar::ladder`] recurses into a compound's components and never
/// looks at the compound's own conjugation, so the reader's explanation of the form comes from
/// the constituent that actually carries it — attributed to that component, which is where the
/// transformation happened. Pinned by a test, because it is behaviour worth noticing if it
/// changes rather than something to rediscover.
///
/// What *does* read it is anything asking "is this an inflected word, and which one": the sense
/// ranker, the interface's first-selection heuristic, and the analyzer-class question this layer
/// itself asks. Those want the unit's lemma, which is why it is set.
///
/// # Why the props keep the head's `base_reading`
///
/// Because the alternative fabricates a word. `fill_formations` builds each rung from
/// `base_reading`, and かっこいい + past through the regular rule is かっこかった — the suppletion
/// this layer exists to handle is exactly what a generator working from the unit's lemma reading
/// would get wrong. The head's よい → よかった is true, and the component attribution says whose
/// truth it is.
///
/// A head with no conjugation gives the unit none. `とか` is not an inflected anything.
fn composed_conjugation(unit: &Candidate, run: &[Token]) -> Option<Conjugation> {
    let head = run.last()?;
    let conjugation = head.conjugation.as_ref()?;
    Some(Conjugation {
        lemma: unit.dict_form.clone(),
        reading: unit
            .lemma_reading
            .established()
            .unwrap_or(&conjugation.reading)
            .to_string(),
        props: conjugation.props.clone(),
    })
}

/// One token standing for a run of them.
///
/// Every field is named rather than defaulted, so that adding one to the model forces a decision
/// about what a composed token does with it instead of silently inheriting a zero.
fn composed(unit: &Candidate, run: &[Token]) -> Token {
    Token {
        surface: unit.surface.clone(),
        dict_form: unit.dict_form.clone(),
        // The analyzer's reading of this source, and the unit's own dictionary-form reading. They
        // differ for a recovered form and coincide for an as-written one, and the difference is
        // what furigana and the word key respectively need.
        reading: unit
            .surface_reading
            .established()
            .unwrap_or_default()
            .to_string(),
        lemma_reading: unit
            .lemma_reading
            .established()
            .unwrap_or_default()
            .to_string(),
        // **A composed token does not inherit a constituent's role** — the model says so in place.
        // かっこ is a noun; かっこいい is not that noun.
        lexical_role: None,
        // Senses belong to the lookup the reader triggers by selecting the word, keyed on the
        // dictionary form above. Under the built-in analyzer no token carries them.
        senses: Vec::new(),
        conjugation: composed_conjugation(unit, run),
        // **Component access, retained.** The constituents are kept exactly as the analyzer left
        // them, which is what lets a reader still reach かっこ and いい separately.
        components: run.to_vec(),
        // Decided later, from the source, by `analyzer::mark_selectable`.
        selectable: false,
        // Recomputed against the assembled sequence by `study::from_analysis`; carrying the
        // constituents' would be claiming a construction about a span nothing recognised.
        constructions: Vec::new(),
        // The analyzer's alternatives were alternatives to a *constituent*. They are not
        // alternatives to this unit, and presenting them as such would be a new claim.
        alternatives: Vec::new(),
        // A break after the run is a break after the unit.
        line_break: run.last().map(|token| token.line_break).unwrap_or_default(),
        start: Some(unit.start),
        end: Some(unit.end),
        // Ichiran's per-word score is about the token it scored. Nothing here is comparable, and
        // inventing a number for a unit the analyzer never proposed is the thing `confidence`'s
        // own documentation warns against.
        confidence: None,
        romaji: None,
        // A doubt about any constituent's reading is a doubt about the unit's.
        reading_unverified: run.iter().any(|token| token.reading_unverified),
        reading_ambiguous: run.iter().any(|token| token.reading_ambiguous),
    }
}

/// Apply the decisions to the baseline, and validate what comes out.
///
/// `decisions` are [`super::select::decide`]'s, over `found`'s candidates. Abstentions are left
/// alone by construction: they name no candidate, so there is nothing to apply and the region
/// keeps the tokenization the analyzer gave it.
pub fn assembled(analysis: &Analysis, found: &Generated, decisions: &[Decision]) -> Assembly {
    let mut chosen: Vec<&Candidate> = decisions
        .iter()
        .filter_map(|decision| match &decision.outcome {
            Outcome::Grouped { candidate, .. } => found.candidates.get(*candidate),
            Outcome::Abstained { .. } => None,
        })
        .collect();
    chosen.sort_by_key(|unit| (unit.tokens.first().copied().unwrap_or_default(), unit.end));

    let abstentions: Vec<Abstention> = decisions
        .iter()
        .filter_map(|decision| match &decision.outcome {
            Outcome::Abstained {
                reason,
                alternatives,
            } => Some(Abstention {
                start: decision.start,
                end: decision.end,
                reason,
                alternatives: alternatives.len(),
            }),
            Outcome::Grouped { .. } => None,
        })
        .collect();

    let baseline = || Assembly {
        analysis: analysis.clone(),
        units: Vec::new(),
        applied: Vec::new(),
        abstentions: abstentions.clone(),
        faults: Vec::new(),
        unapplied: Vec::new(),
        status: Status::Abstained,
    };
    if chosen.is_empty() {
        return baseline();
    }

    let route_of = |unit: &Candidate| match unit.origin {
        Origin::Construction => Route::Construction,
        _ => Route::Lexical,
    };

    let mut unapplied: Vec<(usize, usize, &'static str)> = Vec::new();
    let mut applied: Vec<Applied> = Vec::new();
    let mut tokens: Vec<Token> = Vec::new();
    let mut units: Vec<usize> = Vec::new();
    let mut at = 0usize;

    for unit in chosen {
        let (Some(&from), Some(&to)) = (unit.tokens.first(), unit.tokens.last()) else {
            unapplied.push((unit.start, unit.end, "the unit names no tokens"));
            continue;
        };
        // **Two units over the same tokens is a contradiction, not a choice.** `decide` cannot
        // produce it — overlapping candidates are one region and one region yields one decision —
        // so reaching here means something upstream changed and the honest answer is to apply
        // neither rather than to pick.
        if from < at {
            unapplied.push((
                unit.start,
                unit.end,
                "another unit already claimed these tokens",
            ));
            continue;
        }
        if to >= analysis.tokens.len() || unit.tokens.len() != to - from + 1 {
            unapplied.push((unit.start, unit.end, "the unit's tokens are not a run"));
            continue;
        }
        tokens.extend_from_slice(&analysis.tokens[at..from]);
        units.push(tokens.len());
        tokens.push(composed(unit, &analysis.tokens[from..=to]));
        applied.push(Applied {
            start: unit.start,
            end: unit.end,
            surface: unit.surface.clone(),
            dict_form: unit.dict_form.clone(),
            route: route_of(unit),
            origin: unit.origin,
        });
        at = to + 1;
    }
    tokens.extend_from_slice(&analysis.tokens[at..]);

    let assembled = Analysis {
        execution: None,
        text: analysis.text.clone(),
        tokens,
    };
    let faults = validate_analysis(&assembled);
    if !faults.is_empty() {
        // **The baseline, not a half-applied sequence.** A reader left where they were has lost
        // nothing; a reader shown tokens that do not tile their own source has been given a worse
        // answer than the analyzer's.
        return Assembly {
            faults,
            unapplied,
            status: Status::Discarded,
            ..baseline()
        };
    }

    let status = if applied.is_empty() {
        Status::Abstained
    } else {
        Status::Grouped
    };
    Assembly {
        analysis: assembled,
        units,
        applied,
        abstentions,
        faults: Vec::new(),
        unapplied,
        status,
    }
}

/// **The one path.** Generate, decide, assemble, validate — for the reader and for measurement.
///
/// A caller that wants grouping calls this. There is deliberately no way to run the pieces in a
/// different order or to skip the validation at the end: a measurement taken over a different
/// sequence of steps is a measurement of something the reader never sees.
///
/// A baseline that fails its own span contract yields the baseline unchanged. Grouping has
/// nothing sound to reason from there, and `analyse` has already reported the fault.
pub fn regrouped(analysis: &Analysis, dictionaries: &[Dictionary]) -> Assembly {
    regrouped_in(
        analysis,
        crate::dict::evidence::Evidence::direct(dictionaries),
    )
}

/// [`regrouped`], reading through one request's evidence.
pub fn regrouped_in(
    analysis: &Analysis,
    dictionaries: crate::dict::evidence::Evidence<'_>,
) -> Assembly {
    let found = match super::generate::candidates_in(analysis, dictionaries) {
        Ok(found) => found,
        // **The baseline is already broken**, which `analyse` has reported. Grouping has nothing
        // sound to reason from, and the faults are carried so this is not mistaken for a quiet
        // passage with nothing to group.
        Err(faults) => {
            return Assembly {
                analysis: analysis.clone(),
                units: Vec::new(),
                applied: Vec::new(),
                abstentions: Vec::new(),
                faults,
                unapplied: Vec::new(),
                status: Status::Discarded,
            }
        }
    };
    let matches = crate::grammar::patterns(analysis);
    let places = super::select::occurrences(
        &analysis.tokens,
        &matches,
        super::select::GROUPED_CONSTRUCTIONS,
    );
    let (decisions, _) = super::select::decide(&analysis.tokens, &found, &places);
    assembled(analysis, &found, &decisions)
}
