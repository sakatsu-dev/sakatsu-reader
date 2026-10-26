//! The TypeScript declarations the reader programs against, generated from the Rust types.
//!
//! # Why this exists beside the golden rather than instead of it
//!
//! `ui/src/model.golden.json` is a serialized *example*: it proves the wire format for the values
//! the fixture happens to contain, and nothing about the ones it does not. That is why
//! `model.contract.ts` has to widen every literal union back to `string` — a JSON import loses
//! them — and why a renamed enum variant could pass the contract check once the golden was
//! regenerated.
//!
//! Generation closes exactly that gap. `LineBreak`, `WordStatus` and `Form` become literal unions
//! *derived from the Rust enums*, so renaming a variant changes this file, and a frontend still
//! written against the old name fails to compile.
//!
//! The two are complementary and neither is redundant:
//!
//! | | catches |
//! |---|---|
//! | golden | what serde actually emits, including field names and `null` shapes |
//! | generated | the type as Rust declares it, including every enum variant |
//!
//! **Where they disagree, the golden wins**, because it is bytes rather than a second opinion
//! about them. ts-rs's `serde-compat` is on by default, but `#[ts(…)]` attributes take precedence
//! over `#[serde(…)]` — so a disagreement means an incorrect `ts` attribute, not an incorrect
//! golden.
//!
//! # One file, in dependency order
//!
//! ts-rs can export a file per type. That would put two dozen files under `ui/src/`, each with its
//! own imports, for a contract the reader consumes as a single module. So this concatenates
//! `TS::decl` instead, in an order chosen so a declaration never refers to a name below it — which
//! TypeScript does not require but a person reading the file does.
//!
//! # Generating is not checking
//!
//! `core/src/bin/generate-bindings.rs` writes the file; `core/tests/typescript_contract.rs`
//! compares against it and writes nothing. They call [`render`] so they cannot disagree about what
//! the file should contain.
//!
//! **Deliberately not an `#[ignore]`d "regenerate" test.** This repository runs broad
//! `-- --ignored` sweeps, and one of those would have quietly rewritten a tracked contract file as
//! a side effect of asking for measurements. Routine verification must never rewrite what it is
//! verifying.
//!
//! # One warning is expected, and silencing it would be the bug
//!
//! Building with this feature prints:
//!
//! ```text
//! warning: failed to parse serde attribute
//!   | alias = "learning"
//!   = note: ts-rs failed to parse this attribute. It will be ignored.
//! ```
//!
//! That is `WordStatus::Seen`, which shipped as `Learning` and keeps `#[serde(alias =
//! "learning")]` so old store files still load. **Ignoring it is right.** An alias is a
//! deserialisation courtesy, not part of the wire format: the app only ever *writes* `"seen"`, so
//! a union of `"new" | "seen" | "known" | "ignored"` is the accurate type and adding `"learning"`
//! to it would tell the frontend it may send a name the backend has not written since the rename.
//!
//! So do not "fix" this by widening the union, and do not reach for `no-serde-warnings` — that
//! flag would hide every future attribute ts-rs genuinely could not honour, which is the class of
//! silent disagreement this whole module exists to prevent.

use ts_rs::{Config, TS};

/// Every type in the study and lookup contracts, in the order they are written.
///
/// Adding a type to `StudyView` and forgetting to add it here is caught by the contract test,
/// which compares the whole rendered file: the new field's type would appear in `StudyView`'s
/// declaration with no declaration of its own for the frontend to import.
fn declarations(cfg: &Config) -> Vec<(&'static str, String)> {
    vec![
        // Leaves first, so the file reads top-down.
        ("UpdatePhase", crate::update::UpdatePhase::decl(cfg)),
        ("UpdateStatus", crate::update::UpdateStatus::decl(cfg)),
        ("Sense", crate::model::Sense::decl(cfg)),
        ("Prop", crate::model::Prop::decl(cfg)),
        ("Conjugation", crate::model::Conjugation::decl(cfg)),
        ("Alternative", crate::model::Alternative::decl(cfg)),
        ("LineBreak", crate::model::LineBreak::decl(cfg)),
        (
            "ConstructionIdentity",
            crate::model::ConstructionIdentity::decl(cfg),
        ),
        (
            "ConstructionLookup",
            crate::model::ConstructionLookup::decl(cfg),
        ),
        (
            "ConstructionGrammar",
            crate::model::ConstructionGrammar::decl(cfg),
        ),
        ("SourceSpan", crate::model::SourceSpan::decl(cfg)),
        ("Construction", crate::model::Construction::decl(cfg)),
        ("LexicalRole", crate::model::LexicalRole::decl(cfg)),
        ("Token", crate::model::Token::decl(cfg)),
        ("AnalyzerPolicy", crate::model::AnalyzerPolicy::decl(cfg)),
        ("FallbackReason", crate::model::FallbackReason::decl(cfg)),
        ("FallbackScope", crate::model::FallbackScope::decl(cfg)),
        ("Fallback", crate::model::Fallback::decl(cfg)),
        (
            "ContractionFamily",
            crate::model::ContractionFamily::decl(cfg),
        ),
        ("StemEvidence", crate::model::StemEvidence::decl(cfg)),
        ("RepairNote", crate::model::RepairNote::decl(cfg)),
        ("Execution", crate::model::Execution::decl(cfg)),
        ("Analysis", crate::model::Analysis::decl(cfg)),
        ("SentenceRange", crate::model::SentenceRange::decl(cfg)),
        ("Form", crate::grammar::Form::decl(cfg)),
        ("Formation", crate::grammar::Formation::decl(cfg)),
        ("Step", crate::grammar::Step::decl(cfg)),
        ("Auxiliary", crate::grammar::compounds::Auxiliary::decl(cfg)),
        ("Breakdown", crate::grammar::Breakdown::decl(cfg)),
        ("Match", crate::grammar::Match::decl(cfg)),
        // What each rule position of a match consumed. The interface needs it to ask for an
        // expression offer over one occurrence inside a construction, and a `Match` whose element
        // spans the reader cannot see is a `Match` they cannot ask about.
        ("ElementSpan", crate::grammar::ElementSpan::decl(cfg)),
        // The expression offer the interface asks for when a reader selects part of a recognised
        // construction. Declared here for the same reason as everything else in this list: the
        // reader programs against these, and a type the generator does not know about is a type the
        // interface has to guess at.
        (
            "ExpressionOffer",
            crate::expression::ExpressionOffer::decl(cfg),
        ),
        // Where an occurrence sits inside the construction that recognised it — the parent's own
        // extent, which the offer deliberately does not claim.
        ("Occurrence", crate::expression::Occurrence::decl(cfg)),
        // How the offer's span stands to the selected token's. A literal union, because the panel
        // renders a different sentence for each state and a `string` would let a new one through
        // unhandled.
        (
            "SelectionExtent",
            crate::expression::SelectionExtent::decl(cfg),
        ),
        ("Ruby", crate::dict::Ruby::decl(cfg)),
        // Which secondary indexes each installed dictionary has. The maintenance panel programs
        // against this, and a reader cannot be offered a rebuild whose gain nobody can name.
        ("DictionaryUpkeep", crate::dict::Upkeep::decl(cfg)),
        ("WordStatus", crate::store::WordStatus::decl(cfg)),
        ("StudyView", crate::study::StudyView::decl(cfg)),
        // The lookup contract. `merge::Sense` and `yomitan::Example` are renamed on the way out:
        // `Sense` is already taken above by the analyzer's own sense — a different type with a
        // different shape — and `Example` is too general a name to hand the reader unqualified.
        // The names chosen are the ones the frontend was already using by hand.
        ("MergedSense", crate::dict::merge::Sense::decl(cfg)),
        ("Retrieved", crate::dict::merge::Retrieved::decl(cfg)),
        ("DictExample", crate::dict::merge::MergedExample::decl(cfg)),
        ("AlsoIn", crate::dict::merge::AlsoIn::decl(cfg)),
        ("NameHit", crate::dict::merge::NameHit::decl(cfg)),
        ("WordView", crate::dict::merge::WordView::decl(cfg)),
        (
            "VocabularySenseRank",
            crate::dict::context::RankedSense::decl(cfg),
        ),
        (
            "VocabularyRanking",
            crate::dict::context::VocabularyRanking::decl(cfg),
        ),
        (
            "ContextualWordView",
            crate::dict::context::ContextualWordView::decl(cfg),
        ),
        ("CharReading", crate::search::CharReading::decl(cfg)),
        ("KanjiView", crate::search::KanjiView::decl(cfg)),
        ("SearchForm", crate::search::Form::decl(cfg)),
        ("WordHit", crate::search::WordHit::decl(cfg)),
        ("Found", crate::search::Found::decl(cfg)),
        ("Tier", crate::setup::Tier::decl(cfg)),
        ("SetupPhase", crate::setup::transport::SetupPhase::decl(cfg)),
        (
            "SetupArtifactState",
            crate::setup::transport::SetupArtifactState::decl(cfg),
        ),
        (
            "SetupComponentState",
            crate::setup::transport::SetupComponentState::decl(cfg),
        ),
        (
            "StorageSummary",
            crate::setup::transport::StorageSummary::decl(cfg),
        ),
        (
            "SetupSnapshot",
            crate::setup::transport::SetupSnapshot::decl(cfg),
        ),
        (
            "SetupPreflight",
            crate::setup::transport::SetupPreflight::decl(cfg),
        ),
        (
            "CatalogSource",
            crate::setup::transport::CatalogSource::decl(cfg),
        ),
        (
            "CatalogEntry",
            crate::setup::transport::CatalogEntry::decl(cfg),
        ),
        // The two progress events. Generated for the reason the responses are, arrived at the hard
        // way: these were anonymous `json!` payloads with handwritten mirrors, and the mirrors were
        // already wrong — six declared fields against nine on the wire.
        (
            "FetchProgress",
            crate::setup::transport::FetchProgress::decl(cfg),
        ),
        (
            "SetupProgress",
            crate::setup::transport::SetupProgress::decl(cfg),
        ),
        // The persistence transport. These are the first types here that describe a **rejection**
        // rather than a response, and they are generated for the same reason the rest are: the
        // interface branches on `ErrorCode`, so a variant renamed in Rust must fail the frontend
        // that still names the old one. That is exactly what a golden cannot check — an error is
        // not in any successful response — and exactly what a literal union can.
        ("ErrorCode", crate::persistence::ErrorCode::decl(cfg)),
        (
            "PersistenceError",
            crate::persistence::PersistenceError::decl(cfg),
        ),
        // The recovery report. Generated for the same reason the error codes are: the panel
        // branches on these states to decide which actions to offer, so a variant renamed in Rust
        // must fail the frontend that still names the old one. A golden cannot check them —
        // an interrupted install appears in no successful response.
        (
            "DictionaryRecoveryState",
            crate::recovery::DictionaryRecoveryState::decl(cfg),
        ),
        (
            "DictionaryRecovery",
            crate::recovery::DictionaryRecovery::decl(cfg),
        ),
        (
            "InstallRecoveryState",
            crate::recovery::InstallRecoveryState::decl(cfg),
        ),
        ("LoadingOutcome", crate::recovery::LoadingOutcome::decl(cfg)),
        (
            "InstallRecovery",
            crate::recovery::InstallRecovery::decl(cfg),
        ),
        ("RecoveryReport", crate::recovery::RecoveryReport::decl(cfg)),
        ("WorkPriority", crate::request::WorkPriority::decl(cfg)),
        (
            "RequestIdentity",
            crate::request::RequestIdentity::decl(cfg),
        ),
        // What is running and what each operation used: Settings → Diagnostics and the status
        // phrase beside the capture hint program against these. After `RequestIdentity`, which an
        // operation result carries so the interface can match it to the request it owns.
        ("EngineRole", crate::runtime_status::EngineRole::decl(cfg)),
        ("Hosting", crate::runtime_status::Hosting::decl(cfg)),
        ("EngineState", crate::runtime_status::EngineState::decl(cfg)),
        (
            "Acceleration",
            crate::runtime_status::Acceleration::decl(cfg),
        ),
        (
            "EngineStatus",
            crate::runtime_status::EngineStatus::decl(cfg),
        ),
        ("CacheUse", crate::runtime_status::CacheUse::decl(cfg)),
        ("FailureCode", crate::runtime_status::FailureCode::decl(cfg)),
        ("Outcome", crate::runtime_status::Outcome::decl(cfg)),
        (
            "OperationResult",
            crate::runtime_status::OperationResult::decl(cfg),
        ),
        (
            "RuntimeStatus",
            crate::runtime_status::RuntimeStatus::decl(cfg),
        ),
    ]
}

/// The whole file, exactly as it should appear on disk.
pub fn render() -> String {
    let cfg = Config::default();
    let mut out = String::new();
    out.push_str(HEADER);
    for (name, decl) in declarations(&cfg) {
        out.push_str("\nexport ");
        out.push_str(decl.trim_end());
        out.push('\n');
        let _ = name;
    }
    out
}

const HEADER: &str = "\
/**
 * GENERATED FROM THE RUST TYPES. Do not edit.
 *
 *   cargo run -p sakatsu-core --features typescript --bin generate-bindings
 *
 * `core/tests/typescript_contract.rs` fails when this file stops matching the Rust types, and
 * `npm run check` fails when the code that consumes it stops matching this file.
 *
 * This is one half of the IPC contract. The other is `model.golden.json`, which is generated from
 * real serialized bytes and decides any disagreement: these declarations describe the types as
 * Rust declares them, the golden describes what serde actually puts on the wire. What this half
 * adds is the enums — every variant of `LineBreak`, `WordStatus` and `Form` is a literal here, so
 * renaming one in Rust breaks the frontend that still uses the old name. A JSON import cannot do
 * that, which is why `model.contract.ts` has to widen those same unions back to `string`.
 */
";
