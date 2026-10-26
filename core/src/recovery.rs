//! What an interrupted installation left, in one shape the interface can draw.
//!
//! # Why this exists beside the two mechanisms
//!
//! Recovery has two halves that fail differently. A dictionary is immutable generations with a
//! pointer and a record; everything else is a directory swap with a backup and a journal. Both are
//! modelled where they belong — [`dict::index`](crate::dict::index) and
//! [`setup::publication`](crate::setup::publication) — and neither is a good thing to hand a
//! reader, because a person with an unfinished install does not have two problems.
//!
//! So this is the reporting layer: one report, four distinctions that matter to somebody deciding
//! what to do, and nothing about generations or renames.
//!
//! # What the interface must be able to tell apart
//!
//! - **an unresolved transaction** — finishable, and the only shape where restoring is offered;
//! - **a competing publication** — something newer is live, so restoring would discard it;
//! - **a record nobody can read** — the state cannot be established, which blocks reclamation and
//!   the next install, and needs a person rather than a button;
//! - **a restoration that was attempted and failed** — distinct from a destination that already
//!   exists, which the first version conflated and which told readers something that may be false.
//!
//! Every one of those carries what it would take to act: which transaction, which generation, and
//! where the bytes are now.

use std::path::Path;

use serde::Serialize;

use crate::dict::{Outstanding, RecoveryState};

/// Everything outstanding, for one report.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RecoveryReport {
    /// Dictionaries with an unfinished publication.
    pub dictionaries: Vec<DictionaryRecovery>,
    /// Components whose files were moved aside and not replaced.
    pub installs: Vec<InstallRecovery>,
}

impl RecoveryReport {
    /// Whether there is anything at all to show. An empty report is not drawn.
    pub fn is_empty(&self) -> bool {
        self.dictionaries.is_empty() && self.installs.is_empty()
    }

    /// Whether anything here needs a person to choose rather than a routine to finish.
    pub fn needs_a_decision(&self) -> bool {
        self.dictionaries.iter().any(|entry| {
            matches!(
                entry.state,
                DictionaryRecoveryState::Conflicting | DictionaryRecoveryState::Unreadable
            )
        }) || self.installs.iter().any(|entry| {
            entry.state != InstallRecoveryState::Restored || entry.loading == LoadingOutcome::Failed
        })
    }
}

/// One dictionary with an unfinished publication.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictionaryRecovery {
    /// What to call it — the container's own directory name, which is what the reader installed.
    pub name: String,
    /// The full path, for a diagnostic somebody can copy.
    pub path: String,
    pub state: DictionaryRecoveryState,
    /// The transaction an action must name to be accepted. Absent only when the record is
    /// unreadable, which is the one case where nothing can be acted on.
    pub transaction: Option<String>,
    /// The generation that transaction published, which an action must also name.
    pub published: Option<String>,
    /// What the container pointed at before — a generation, or `null` for a legacy layout.
    pub previous: Option<String>,
    /// What it points at now, when that is neither of the above.
    pub live: Option<String>,
    /// Why it cannot be read, for [`DictionaryRecoveryState::Unreadable`].
    pub detail: Option<String>,
    /// Whether a repair has already been attempted and failed.
    pub already_tried: bool,
}

/// How far a dictionary's unfinished publication got.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictionaryRecoveryState {
    /// The switch landed and nothing has resolved it. Both actions are available.
    Unresolved,
    /// Something newer is live. Restoring would discard it, so only keeping is offered.
    Conflicting,
    /// The container is already where the record wanted it; only the record is left.
    CleanupOnly,
    /// The switch never landed. Nothing was changed, so nothing needs undoing.
    NeverSwitched,
    /// The record cannot be read, so the state cannot be established. **No action is offered**:
    /// this needs a person looking at the file, and until then nothing on this dictionary is
    /// reclaimed and no new install of it will start.
    Unreadable,
}

/// One component whose files an interrupted install left moved aside.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallRecovery {
    /// The component id, as the catalog names it.
    pub id: String,
    pub state: InstallRecoveryState,
    /// Where it was going, or is now.
    pub destination: Option<String>,
    /// Where the bytes are, when they are not at the destination.
    pub saved: Option<String>,
    /// Every abandoned install offering the same destination, for
    /// [`InstallRecoveryState::Competing`].
    pub claimants: Vec<String>,
    /// Why, for the states that have a reason.
    pub detail: Option<String>,
    /// Whether the restored files were also **loaded** into this session, and how that is known.
    ///
    /// **Restoring a file and being able to use it are different things**, and the interface has to
    /// say which happened: a component put back on disk but not adopted is one the reader will not
    /// see until they restart, and reporting that as success would be the fix reading as a
    /// different bug.
    pub loading: LoadingOutcome,
}

/// Whether a restored component is usable in this session.
///
/// **Not inferred from having called the adoption path.** Every one of these is established by
/// asking the handle whether it now holds anything — a reload that ran and produced an empty table
/// is a failure the reader needs to hear about, and it looks identical to a successful one from the
/// caller's side.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadingOutcome {
    /// Reloaded, and the handle now holds it. Usable now.
    Loaded,
    /// The files are on disk and the reload did not produce anything usable. The reason travels in
    /// [`InstallRecovery::detail`].
    Failed,
    /// Nothing needed reloading — the component is read when it is first used, so the restored
    /// files are already what the next use will find.
    NotNeeded,
    /// Genuinely not established.
    ///
    /// Its own value rather than being folded into "restart to use it": telling somebody to restart
    /// when nothing knows whether they need to is a guess wearing the clothes of advice.
    Unknown,
}

/// What happened, or did not, to an interrupted component.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallRecoveryState {
    /// Put back. See [`InstallRecovery::loaded`] for whether this session can use it yet.
    Restored,
    /// A backup beside a destination that already exists. Nothing was touched.
    Retained,
    /// Restoration was attempted and failed. The bytes are still at `saved`.
    Failed,
    /// More than one abandoned install offers this destination. Choosing is a person's call.
    Competing,
    /// Something was left behind without saying where it belongs.
    Unidentified,
}

/// Build the dictionary half of a report.
pub fn dictionaries(dir: &Path) -> Vec<DictionaryRecovery> {
    crate::dict::outstanding_recoveries(dir)
        .into_iter()
        .map(|(container, outcome)| {
            let name = container
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| container.display().to_string());
            let path = container.display().to_string();
            match outcome {
                Err(detail) => DictionaryRecovery {
                    name,
                    path,
                    state: DictionaryRecoveryState::Unreadable,
                    transaction: None,
                    published: None,
                    previous: None,
                    live: None,
                    detail: Some(detail),
                    already_tried: false,
                },
                Ok(outstanding) => {
                    let record = outstanding.record().clone();
                    let (state, live) = match &outstanding {
                        Outstanding::Unresolved { .. } => {
                            (DictionaryRecoveryState::Unresolved, None)
                        }
                        Outstanding::ConflictingPublication { live, .. } => {
                            (DictionaryRecoveryState::Conflicting, live.clone())
                        }
                        Outstanding::CleanupOnly { .. } => {
                            (DictionaryRecoveryState::CleanupOnly, None)
                        }
                        Outstanding::NeverSwitched { .. } => {
                            (DictionaryRecoveryState::NeverSwitched, None)
                        }
                    };
                    DictionaryRecovery {
                        name,
                        path,
                        state,
                        transaction: Some(record.transaction.clone()),
                        published: Some(record.published.clone()),
                        previous: record.previous.clone(),
                        live,
                        detail: None,
                        already_tried: record.state == RecoveryState::RollBackFailed,
                    }
                }
            }
        })
        .collect()
}
