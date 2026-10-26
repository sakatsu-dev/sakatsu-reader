//! Dictionaries: import, storage, lookup, and the policy for showing several
//! sources at once (`project notes` §6).
//!
//! The shape of this is set by three constraints:
//!
//! - **Yomitan format**, because that is what the dictionaries a learner already has
//!   are packaged as. See [`yomitan`].
//! - **No server and no native database.** A zip is imported once into a plain
//!   on-disk index whose lookups read only the bytes they need. See [`index`].
//! - **Data travels with its own licence.** Every dictionary's own attribution is
//!   captured at import and kept beside it (`project notes` §8).

pub mod context;
pub mod evidence;
pub mod frequency;
pub mod index;
pub mod kanji;
pub mod kanjivg;
pub mod merge;
pub mod names;
pub mod readings;
pub mod tags;
pub mod yomitan;

/// What one installed dictionary can and cannot answer.
///
/// **Separate from English search on purpose.** "Is the gloss index ready" answers one question —
/// can a reader type English — and it was the *only* question anything asked. A dictionary whose
/// gloss bucket exists reports ready, the Reference prompt stays hidden, and nothing ever mentions
/// the buckets it is still missing. That is how an index without `sequence` went unnoticed here:
/// English search worked, so the one surface that could have rebuilt it was never shown, while
/// every word whose spelling the dictionary files as a cross-reference displayed a pointer —
/// `⟶ 抜 ぬ き 取 と る` — instead of a definition.
///
/// Reported per dictionary rather than as one boolean, because "something needs building" cannot
/// say what building it would gain, and an interface that cannot say that is asking for consent it
/// has not informed.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
// `Upkeep` alone is too general a name to hand the reader unqualified, the same reason
// `merge::Sense` goes out as `MergedSense`.
#[cfg_attr(feature = "typescript", ts(rename = "DictionaryUpkeep"))]
pub struct Upkeep {
    /// The name this dictionary is installed under — see [`index::Dictionary::installed_as`].
    ///
    /// **The identity, where `title` is only a label.** Two installed dictionaries can share a
    /// title, so a list keyed on one collapses them into a single row and offers a rebuild for
    /// whichever it happened to keep.
    pub id: String,
    /// The dictionary's own title, as its `index.json` gives it. For showing, not for keying.
    pub title: String,
    /// English glosses are searchable.
    pub has_glosses: bool,
    /// A kanji's compounds can be listed.
    pub has_kanji: bool,
    /// A word's other spellings can be found — and, because a cross-reference is filed under the
    /// word it points at, this is the difference between a definition and an arrow.
    pub has_sequence: bool,
    /// Whether any of the above is missing, which is exactly what the backfill would build.
    pub needs_indexes: bool,
}

impl Upkeep {
    /// Read the buckets an open dictionary has.
    pub fn of(dictionary: &index::Dictionary) -> Upkeep {
        Upkeep {
            id: dictionary.installed_as(),
            title: dictionary.meta().title.clone(),
            has_glosses: dictionary.has_glosses(),
            has_kanji: dictionary.has_kanji(),
            has_sequence: dictionary.has_sequence(),
            needs_indexes: dictionary.needs_secondary_index(),
        }
    }
}

pub use context::{
    contextualize, contextualize_with_lookup, ContextualWordView, RankedSense, VocabularyRanking,
};
pub use index::{
    keep_current, outstanding_recoveries, restore_previous, Dictionary, DictionaryMeta, Identity,
    LookupHit, MatchKind, Outstanding, PreparedGeneration, RecoveryRecord, RecoveryState, TagInfo,
};
pub use kanji::{align, attested, furigana, Kanji, KanjiDic, Ruby};
pub use kanjivg::{
    is_drawing_defect, load_diagram, parse_diagram, KanjiDiagram, KanjiStroke, StrokeNumber,
};
pub use merge::{merge, Retrieved, SourceResult, WordView};
pub use names::{look_up_name, merge_names, FoundName};
pub use readings::merge_compounds;
pub use tags::TagGlossary;
pub use yomitan::{Definition, Entry, Example};

/// Why a dictionary could not be imported, opened, or read.
#[derive(Debug)]
pub enum DictError {
    /// The file or directory could not be read or written.
    Io(std::io::Error),
    /// The zip could not be opened, or an expected member was unreadable.
    Archive(String),
    /// Something inside was not the JSON it should have been.
    BadFormat(String),
    /// The zip is missing `index.json`, so it is not a Yomitan dictionary.
    NotADictionary,
    /// A recovery action described a transaction that is no longer the outstanding one.
    ///
    /// A recovery screen is a picture of a moment, and between drawing it and pressing a button
    /// the transaction it described can be resolved, superseded, or replaced by a second attempt.
    /// Refused rather than applied to whatever is outstanding now: the person authorised an action
    /// against a description they were shown, not against this one.
    StaleRecovery(String),
    /// A previous transaction on this container has not been resolved.
    ///
    /// One container holds one recovery record, so starting a second transaction over an
    /// unresolved one would overwrite the only description of what the first did — and with it the
    /// protection on the generation the first still needs. Refused rather than queued: the app has
    /// no business guessing which of two half-finished installs a person wanted.
    RecoveryOutstanding(String),
    /// Recovery cannot restore the generation named by the still-valid record.
    ///
    /// Kept separate from `RecoveryOutstanding`: the latter tells a publisher to resolve an older
    /// transaction first. Here the reader is already attempting that resolution and needs to know
    /// which retained bytes must be restored before retrying the same action.
    RecoveryTargetUnavailable(String),
    /// The reader asking for this holds a generation that is no longer the live one.
    ///
    /// Returned rather than silently doing the work, because the work would publish stale data:
    /// a backfill builds from the generation its reader resolved, and publishing that after a
    /// newer import had landed would quietly reinstate the older dictionary.
    Superseded,
}

impl std::fmt::Display for DictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DictError::Io(error) => write!(f, "{error}"),
            DictError::Archive(message) => write!(f, "archive could not be read: {message}"),
            DictError::BadFormat(message) => write!(f, "unexpected dictionary format: {message}"),
            DictError::Superseded => write!(
                f,
                "this dictionary was replaced while the request was in flight; retry with a fresh reader"
            ),
            DictError::StaleRecovery(detail) => write!(
                f,
                "this recovery information is out of date ({detail}); refresh and look again"
            ),
            DictError::RecoveryOutstanding(detail) => write!(
                f,
                "an earlier installation of this dictionary was not finished ({detail}); resolve it before publishing again"
            ),
            DictError::RecoveryTargetUnavailable(detail) => write!(
                f,
                "the previous dictionary installation cannot be restored yet ({detail}); its recovery record was kept, so restore the missing files and try again"
            ),
            DictError::NotADictionary => {
                write!(f, "no index.json — this is not a Yomitan dictionary")
            }
        }
    }
}

impl std::error::Error for DictError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DictError::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DictError {
    fn from(error: std::io::Error) -> Self {
        DictError::Io(error)
    }
}

impl From<serde_json::Error> for DictError {
    fn from(error: serde_json::Error) -> Self {
        DictError::BadFormat(error.to_string())
    }
}

impl From<zip::result::ZipError> for DictError {
    fn from(error: zip::result::ZipError) -> Self {
        DictError::Archive(error.to_string())
    }
}
