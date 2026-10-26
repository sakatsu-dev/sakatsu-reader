//! What happens to a save when the store is not there, and how that is reported.
//!
//! # This is not a second `Store`
//!
//! [`Store`](crate::store::Store) already owns persistence: atomic writes, the migration backup
//! debt, the refusal to overwrite a settings file from a newer build, batched status reads, and
//! [`WordKey`](crate::store::WordKey). None of that is repeated here and none of it changed — a
//! wrapper that forwarded each method would add a layer with no decision in it.
//!
//! What *was* scattered across the shell is the **degradation policy**: what the app does when
//! `Store::open` failed and there is nothing to write to. That lived as four `ok_or_else` calls,
//! two `unwrap_or_default()`s and a private `unavailable()` helper in `commands.rs`, which meant
//! the answer to "what does the reader see when persistence is gone" was assembled differently at
//! each call site. It is one type and four functions here.
//!
//! # No store is a supported state, and it degrades in two directions
//!
//! A store that could not be opened leaves the reader reading. Reads answer with defaults —
//! every word unmarked, the shipped settings — because a reader with no saved progress is exactly
//! a reader who has made none. Writes **fail loudly**, because a mark that silently goes nowhere
//! is worse than one refused: the learner believes it was kept.
//!
//! # Why the error is typed rather than a sentence
//!
//! The interface has to treat three outcomes differently, and it used to tell them apart by
//! comparing the rejection text against the standing warning it was already showing. That worked,
//! and it made behaviour depend on prose: rewording a message in Rust would have silently changed
//! what the frontend drew. [`PersistenceError`] carries a [`code`](PersistenceError::code) that is
//! part of the contract and a `message` that is not, so the interface can branch on the first and
//! print the second.

use serde::Serialize;

use crate::model::Token;
use crate::store::{Counts, Settings, Store, StoreError, WordKey, WordStatus};

/// Why a save did not happen, in a form the interface can branch on.
///
/// The `code` is the contract; the `message` is for a person to read and is never parsed. Both
/// travel, because neither is sufficient: a code alone cannot say *which* file or *what* went
/// wrong with it, and a sentence alone is what the interface was reduced to string-matching.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersistenceError {
    pub code: ErrorCode,
    /// The whole sentence, from the side that knows what happened. **Never prefixed** by anything
    /// that passes it on — that is how a degraded session came to say the same thing three times,
    /// nested.
    pub message: String,
}

/// What kind of failure this is. Stable: the interface branches on these.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// There is no store at all — `Store::open` failed, and nothing can be saved this session.
    ///
    /// The reader is told once, by a standing warning, and individual refusals are answered with
    /// a short line rather than a repeat of the reason.
    StoreUnavailable,
    /// The store exists and declined this write to protect what is already on disk.
    ///
    /// A settings file from a newer build, or one whose migration backup could not be written.
    /// Unlike [`Self::StoreUnavailable`] this is specific to the settings file, and unlike
    /// [`Self::WriteFailed`] nothing was touched.
    SettingsReadOnly,
    /// The write was attempted and went wrong.
    WriteFailed,
    /// A recovery action named a transaction that is no longer the outstanding one.
    ///
    /// Its own code because the interface answers it differently from every other failure: nothing
    /// went wrong and nothing needs reporting as broken — the screen is describing a moment that
    /// has passed, so it refreshes and shows what is true now.
    StaleRecovery,
}

impl PersistenceError {
    /// The code, for a caller that has the error rather than its serialized form.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    fn unavailable(reason: Option<&str>) -> Self {
        PersistenceError {
            code: ErrorCode::StoreUnavailable,
            message: match reason {
                // Passed through whole. `open_store` writes a complete sentence and this is not
                // the place to have an opinion about it.
                Some(reason) => reason.to_string(),
                // `open_store` never produces a missing store without a reason; worded as a whole
                // sentence anyway, because whatever shows it will not add one.
                None => "No store is open, so nothing can be saved this session.".to_string(),
            },
        }
    }
}

impl From<StoreError> for PersistenceError {
    fn from(error: StoreError) -> Self {
        let code = match error {
            // Structural, not a guess at the wording — which is the reason `Refused` exists.
            StoreError::Refused(_) => ErrorCode::SettingsReadOnly,
            StoreError::Io(_) | StoreError::BadFormat(_) => ErrorCode::WriteFailed,
        };
        PersistenceError {
            code,
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PersistenceError {}

/// The store this invocation writes to, lent by whoever opened it.
///
/// The same lending shape as [`study::Resources`](crate::study::Resources), and for the same
/// reason: the caller owns the lifetime, and nothing here reaches for a handle of its own.
///
/// `store: None` is not an error state. It is the supported degradation, and `unavailable` is why
/// — kept beside it so a refusal can say what happened without the caller assembling the sentence
/// again at each site.
#[derive(Clone, Copy)]
pub struct Persistence<'a> {
    pub store: Option<&'a Store>,
    /// Why there is no store, when there is none. Ignored when there is one.
    pub unavailable: Option<&'a str>,
}

impl<'a> Persistence<'a> {
    /// A store that opened.
    pub fn open(store: &'a Store) -> Self {
        Persistence {
            store: Some(store),
            unavailable: None,
        }
    }

    /// No store, and the reason.
    pub fn none(reason: &'a str) -> Self {
        Persistence {
            store: None,
            unavailable: Some(reason),
        }
    }

    fn require(&self) -> Result<&Store, PersistenceError> {
        self.store
            .ok_or_else(|| PersistenceError::unavailable(self.unavailable))
    }

    /// Mark a word, and answer with the status now stored.
    ///
    /// **Read back rather than echoed.** Returning the requested status would make the interface
    /// believe a write it never confirmed; the store is asked what it now holds, so a caller that
    /// gets an answer at all can trust it. `WordKey::for_token` is unchanged and is what keeps
    /// homographs apart.
    pub fn mark(&self, token: &Token, status: WordStatus) -> Result<WordStatus, PersistenceError> {
        let store = self.require()?;
        let key = WordKey::for_token(token);
        store.set_status(key.clone(), status)?;
        Ok(store.status(&key))
    }

    /// Save settings, or say why not.
    ///
    /// Normalisation, the backup debt and the future-version refusal are all `Store`'s and are not
    /// repeated here — this decides only what happens when there is no store, and classifies what
    /// comes back.
    pub fn save_settings(&self, settings: Settings) -> Result<(), PersistenceError> {
        Ok(self.require()?.set_settings(settings)?)
    }

    /// The saved settings, or the shipped defaults when there is no store.
    ///
    /// Degrades rather than failing: a reader whose store did not open still gets an app that
    /// works, and the standing warning is what tells them their changes will not survive.
    pub fn settings(&self) -> Settings {
        self.store.map(Store::settings).unwrap_or_default()
    }

    /// How many words are marked, by status. Zeroes when there is no store, which is honest —
    /// nothing is stored, so nothing is marked.
    pub fn counts(&self) -> Counts {
        self.store.map(Store::counts).unwrap_or_default()
    }
}
