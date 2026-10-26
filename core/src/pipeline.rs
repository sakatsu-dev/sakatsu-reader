//! The dictionary passes that run between the analyzer and the reader, in one place.
//!
//! # What this is, and what it deliberately is not
//!
//! An [`Analyzer`](crate::analyzer::Analyzer) returns tokens. Before a reader sees them the app
//! runs two dictionary passes that **rewrite the token list** — compounds and names are joined —
//! and one that **rewrites readings in place**. Everything after that (grammar breakdown, pattern
//! matching, sentence ranges, `WordKey` resolution, furigana) is a pure function of a finished
//! token list, and stays where it is.
//!
//! So the boundary here is exactly *the stages that mutate `analysis.tokens`*, which is why
//! `commands::analyze` is this module and `commands::study` is this module plus its derivations.
//!
//! # Two entry points, because two things are genuinely wanted
//!
//! [`Pipeline::segmentation`] is joins only. [`Pipeline::display`] is joins **and** reading
//! correction. The difference is not a detail: joining a word changes the denominator of a reading
//! measurement, so a harness that wants raw heteronym accuracy wants the first and a harness that
//! wants what the reader sees wants the second. Naming them stops the choice being made by
//! whichever private helper someone happened to reach for.
//!
//! # One snapshot for the whole invocation
//!
//! [`Pipeline::new`] takes a borrowed slice of dictionaries and holds it across every stage. That
//! is the point rather than an implementation convenience: the joins and the correction used to
//! resolve the live dictionary set **separately**, so an import landing between them would join a
//! passage against one generation and correct it against another. `dict::index`'s generation
//! leases guarantee a reader keeps its files; a borrow that spans both stages is what carries the
//! guarantee up into the pipeline.
//!
//! # Reporting rather than logging
//!
//! Both entry points return counts and log nothing. The desktop shell logs them; the evaluation
//! harnesses do not want a log line per fixture. Behaviour is identical either way, and this way
//! neither caller has to opt out of the other's decision.

use crate::dict::evidence::Evidence;
use crate::dict::index::Dictionary;
use crate::dict::readings::Corrections;
use crate::model::{Analysis, Token};

/// What the name pass did, for a caller that wants to say so.
///
/// Compounds are not reported separately because `merge_compounds` joins only where the dictionary
/// spells the whole differently than its parts, which is rare enough that a count of it says less
/// than the names count does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Joins {
    /// Tokens after compounds were joined, before names were.
    pub before: usize,
    /// Tokens after names were joined.
    pub after: usize,
}

impl Joins {
    /// Whether the name pass changed anything.
    pub fn rejoined(&self) -> bool {
        self.before != self.after
    }
}

/// Everything [`Pipeline::display`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Displayed {
    /// The name pass.
    pub joins: Joins,
    /// The reading pass.
    pub readings: Corrections,
}

/// The dictionary passes, over one pinned set of dictionaries.
#[derive(Debug, Clone, Copy)]
pub struct Pipeline<'a> {
    evidence: Evidence<'a>,
}

impl<'a> Pipeline<'a> {
    /// Pin a dictionary set for the whole invocation.
    ///
    /// The caller resolves the live set **once** and lends it here. This type never reaches for a
    /// live handle of its own, which is what makes "the joins and the correction saw the same
    /// dictionaries" a property of the type rather than a promise in a comment.
    pub fn new(dictionaries: &'a [Dictionary]) -> Pipeline<'a> {
        Pipeline::with_evidence(Evidence::direct(dictionaries))
    }

    /// The same, reading through one request's evidence, so a lookup another stage of the request
    /// already made is not made again.
    pub fn with_evidence(evidence: Evidence<'a>) -> Pipeline<'a> {
        Pipeline { evidence }
    }

    /// The dictionaries this pipeline is running against.
    pub fn dictionaries(&self) -> &'a [Dictionary] {
        self.evidence.dictionaries()
    }

    /// A reading the dictionary gives a whole spelling, where it differs from the spelling itself.
    ///
    /// **This closure is the thing that was actually duplicated.** It existed byte-identically in
    /// the desktop command and in the test harness; the call *order* around it was described in
    /// prose in both places and got out of step anyway. The condition matters in all three parts:
    /// the hit must be for this exact term (a lookup can return inflections and near matches), the
    /// reading must exist, and it must differ from the term — a kana headword reads as itself and
    /// tells `merge_compounds` nothing.
    fn compound_reading(&self, surface: &str) -> Option<String> {
        self.evidence.sources().find_map(|source| {
            source.lookup(surface).ok()?.into_iter().find_map(|hit| {
                (hit.entry.term == surface
                    && !hit.entry.reading.is_empty()
                    && hit.entry.reading != hit.entry.term)
                    .then_some(hit.entry.reading)
            })
        })
    }

    /// Joins only: compounds, then names. No reading is touched.
    ///
    /// **Compounds before names, because a compound reading is the stronger claim.**
    /// `merge_compounds` joins only where the dictionary spells the whole differently than its
    /// parts — 受付 + 係 is うけつけがかり, not うけつけかかり — so it never competes with a name for
    /// the same run: a name dictionary would have to disagree with an ordinary entry about a
    /// compound it also lists. Getting this order wrong measures a pipeline nobody ships; skipping
    /// the passes entirely understated long-unit F1 by 2.7 points against gold.
    ///
    /// With no dictionaries open the tokens come back untouched, which is the honest answer rather
    /// than an error: the analyzer's segmentation is still a segmentation.
    pub fn segmentation(&self, tokens: &[Token]) -> (Vec<Token>, Joins) {
        if self.evidence.is_empty() {
            return (tokens.to_vec(), Joins::default());
        }
        let joined = crate::dict::merge_compounds(tokens, |surface| self.compound_reading(surface));
        let before = joined.len();
        let named = crate::dict::merge_names(&joined, |surface| {
            crate::dict::names::look_up_name_in(self.evidence, surface)
        });
        let after = named.len();
        (named, Joins { before, after })
    }

    /// The joins, then the reading correction — what the reader is shown.
    ///
    /// **Correction after joining, not before.** A joined word is a different question from its
    /// parts: 受付係 wants うけつけがかり, and asking about 係 alone cannot produce it. See
    /// `dict::readings` for why a spelling with two readings on record is deliberately left alone
    /// rather than guessed at.
    pub fn display(&self, analysis: &mut Analysis) -> Displayed {
        let (tokens, joins) = self.segmentation(&analysis.tokens);
        analysis.tokens = tokens;
        let readings = crate::dict::readings::correct_in(analysis, self.evidence);
        Displayed { joins, readings }
    }
}
