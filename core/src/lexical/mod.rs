//! A reusable lexical layer: what a span *could* be, kept apart from what was chosen.
//!
//! # The problem this exists for
//!
//! Lindera's units are sometimes smaller than the word a reader wants to look up. `かっこいい`
//! arrives as `かっこ` + `いい` and the panel offers **"geta"**; `とか` arrives as `と` + `か` and
//! offers "if; when" beside "indicates a question", while the grammar layer has already recognised
//! the construction the two belong to. The reader can see the word and cannot select it.
//!
//! # The shape of the answer
//!
//! Three stages that never merge into each other:
//!
//! 1. [`generate`] proposes units over a finished analysis, borrowing it.
//! 2. selection decides, conservatively, and is entitled to decide nothing.
//! 3. assembly rebuilds the analysis from whatever selection returned.
//!
//! Keeping them apart is what makes the middle one auditable. A layer that generated and chose in
//! one pass could only report its conclusions, and the question worth asking when a grouping does
//! not happen is *which* requirement it failed.
//!
//! # What it will not do
//!
//! **A dictionary entry is not a reason to group.** 格好 is a word, いい is a word, 格好いい is a
//! word, and only context separates them — `格好いい` is etymologically the first two. So a match
//! is evidence, eligibility is a conjunction of requirements, and where the requirements do not
//! settle it the baseline stands and the alternative is offered rather than applied. Abstention is
//! the ordinary outcome, not a failure of the mechanism.
//!
//! This layer also never rewrites source text. Spelling and span are preserved exactly; a proposed
//! dictionary form is recorded beside the surface, never in place of it.

pub mod assemble;
pub mod candidate;
pub mod contraction;
pub mod generate;
pub mod inflect;
pub mod repair;
pub mod select;

pub use assemble::{assembled, regrouped, regrouped_in, Abstention, Applied, Assembly, Status};
pub use candidate::{
    classes_agree, AnswerKind, Candidate, ClassAgreement, Derivation, DictionaryId, Eligibility,
    EntryEvidence, EntryId, InflectionGround, Origin, Reading,
};
pub use generate::{
    candidates, candidates_in, Failure, Generated, Refused, Unsearched, MOST_CHARS, MOST_TOKENS,
};
pub use inflect::{
    reading_lookup, recover_adjective, AdjectiveForm, LemmaProposals, LookupFailure, Proposal,
    ReadingHits, Recovered, Recovery, SameWordRelationship, TokenMorphology, VariantEvidence,
    TRANSFORMATION_CLASS,
};
pub use select::{decide, occurrences, Decision, Occurrence, Outcome, Route, Structure};
