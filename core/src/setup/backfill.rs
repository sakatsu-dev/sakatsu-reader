//! Building the English gloss index for dictionaries that were installed without one.
//!
//! # Why this is here rather than in the command that runs it
//!
//! It publishes a **new generation of every dictionary it touches**, which makes it one of the
//! operations that writes the resource tree — the same class as an install and a relocation, and
//! the one that was easiest to miss because nothing is downloaded and no directory is swapped. It
//! took no reservation at all until [`crate::setup::claim`] existed, so it could publish into a
//! location a move was in the middle of copying. `core/tests/resource_coordination.rs` reproduces
//! what that costs.
//!
//! Moving the loop here is what allows the claim to be taken in one place. What stays in the shell
//! is the thread it runs on, the progress event, and rebuilding the open readers afterwards — all
//! of which need an application handle, and none of which is policy.
//!
//! # Which directories
//!
//! [`Dictionary::discover`], the same policy the reader and the evaluation harness use. The
//! previous loop had its own enumeration; a second one is a second thing to get wrong, and it had
//! already been wrong once — a marker test that asked where the files used to be found nothing for
//! exactly the published installs it was meant to find.

use std::path::Path;

use crate::dict::Dictionary;
use crate::setup::claim::Claim;

/// What one backfill did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Backfilled {
    /// One entry per dictionary that gained an index, as `name (n entries)`.
    pub built: Vec<String>,
    /// The last dictionary that could not be indexed, and why.
    ///
    /// **The run continues past it**, and the last failure is the one reported — both of which are
    /// what the shell loop this was extracted from did. One unreadable container is not a reason to
    /// leave every other dictionary without English search.
    pub failed: Option<String>,
}

impl Backfilled {
    /// What to say about a finished run, in the words the log and the panel both use.
    pub fn summary(&self) -> String {
        if self.built.is_empty() {
            "nothing to build".to_string()
        } else {
            self.built.join(", ")
        }
    }
}

/// Build the gloss index for every dictionary under `dir` that lacks one.
///
/// **The witness, not the claim.** `dir` is a resolved resource location, so claiming in here would
/// leave the caller free to work out where the dictionaries live, wait behind somebody else's move,
/// and then index a directory that had been abandoned in the meantime. The caller claims first,
/// resolves second, and keeps the witness until it has finished rebuilding the open readers — the
/// session's handles are part of what this changes, and a move landing between the publication and
/// the reload would leave them pointing at the old location.
pub fn rebuild_gloss_indexes(dir: &Path, owned: &crate::setup::claim::Claimed) -> Backfilled {
    owned.require(Claim::Dictionaries);
    let mut outcome = Backfilled::default();
    for path in Dictionary::discover(dir) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let built = Dictionary::open(&path).and_then(|dictionary| {
            if dictionary.needs_secondary_index() {
                dictionary.index_secondary()
            } else {
                Ok(0)
            }
        });
        match built {
            Ok(0) => {}
            Ok(count) => outcome.built.push(format!("{name} ({count} entries)")),
            Err(error) => outcome.failed = Some(format!("{name}: {error}")),
        }
    }
    outcome
}
