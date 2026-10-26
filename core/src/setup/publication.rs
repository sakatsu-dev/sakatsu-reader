//! Publication of validated resource sets, shared by online and offline installers.
//!
//! Every failed rename takes the same rollback path, including moving an old file aside.
//! Rollback failures retain the backup and name it in the error; cleanup must never destroy
//! the only surviving old copy merely because the installer is returning an error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Whether replacing this directory would replace one or more dictionaries.
///
/// # Two shapes, and the important one is the parent
///
/// The check began by looking only at the destination's own markers, which missed the case it was
/// written for: `destination("dictionaries")` is the **parent** — `dictionaries/`, holding
/// `jitendex/` and `jmnedict/` — and the parent carries no `current` and no `g-` directory of its
/// own. The original defect replaced exactly that parent, so a guard that inspected only the
/// destination would have let it straight through.
///
/// So a directory counts if it is a dictionary itself *or* contains one as an immediate child.
/// That is the whole of the real layout; nothing nests deeper.
///
/// # Legacy dictionaries have no generation markers at all
///
/// A directory imported before generations existed is its own generation: `meta.json` and
/// `terms.ndjson`, no `current`, no `g-`. Readers still lease it by path, so replacing it destroys
/// bytes in exactly the same way — and it is precisely the install that has *not* been re-imported
/// recently, which makes it the more likely one to still be sitting there.
fn is_dictionary_container(destination: &Path) -> bool {
    if is_one_dictionary(destination) {
        return true;
    }
    std::fs::read_dir(destination).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().is_dir() && is_one_dictionary(&entry.path()))
    })
}

/// Whether this directory *is* a dictionary — generation-based or legacy.
fn is_one_dictionary(dir: &Path) -> bool {
    if dir.join("current").is_file() {
        return true;
    }
    // The legacy layout: the dictionary is the directory. Both files, because `meta.json` alone is
    // a common enough name to catch something unrelated.
    if dir.join("meta.json").is_file() && dir.join("terms.ndjson").is_file() {
        return true;
    }
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().is_dir()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("g-"))
        })
    })
}

/// Where a publication was sending each component, written before anything moved.
///
/// # Why the destinations cannot be reconstructed later
///
/// Recovery first asked `commands::destination(id)` at startup — today's configuration — for a
/// backup made by yesterday's. The resource directories honour `SAKATSU_*_DIR` overrides and a
/// device-data location that a person can change, so a backup could have been restored to a path
/// the interrupted install had never heard of. The mapping has to come from the install that made
/// it, so it is written down before the first rename.
///
/// A root with a backup and no journal is **not** guessed at. It is reported and retained: an old
/// install predating this file, or one whose journal did not survive, is exactly the case where
/// putting bytes somewhere on a guess is least affordable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublicationJournal {
    pub members: Vec<JournalMember>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JournalMember {
    pub id: String,
    pub destination: PathBuf,
}

impl PublicationJournal {
    fn path(root: &Path) -> PathBuf {
        root.join("publication.json")
    }

    /// Write it atomically, before any rename. A torn journal is an unidentified root, which is
    /// recoverable-by-hand rather than recoverable-by-routine, so it is worth the rename.
    fn write(&self, root: &Path) -> Result<(), String> {
        let staged = root.join(".publication.json.tmp");
        std::fs::write(
            &staged,
            serde_json::to_string_pretty(self)
                .map_err(|error| error.to_string())?
                .as_bytes(),
        )
        .map_err(|error| error.to_string())?;
        std::fs::rename(&staged, Self::path(root)).map_err(|error| {
            let _ = std::fs::remove_file(&staged);
            error.to_string()
        })
    }

    fn read(root: &Path) -> Result<Option<PublicationJournal>, String> {
        match std::fs::read_to_string(Self::path(root)) {
            Ok(text) => {
                let journal: PublicationJournal =
                    serde_json::from_str(&text).map_err(|error| error.to_string())?;
                // Validated, not merely parsed. An id that is not a single path component, or a
                // relative destination, would send a restore somewhere nobody chose.
                for member in &journal.members {
                    let mut parts = Path::new(&member.id).components();
                    if !matches!(parts.next(), Some(std::path::Component::Normal(_)))
                        || parts.next().is_some()
                    {
                        return Err(format!(
                            "journal member {:?} is not a plain name",
                            member.id
                        ));
                    }
                    if !member.destination.is_absolute() {
                        return Err(format!(
                            "journal member {} has a relative destination {}",
                            member.id,
                            member.destination.display()
                        ));
                    }
                }
                Ok(Some(journal))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// One backup that could be put back, and where its own install meant it to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub root: PathBuf,
    pub id: String,
    pub destination: PathBuf,
    pub saved: PathBuf,
}

/// What one interrupted root offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interrupted {
    /// Nothing was left behind.
    Nothing,
    /// A backup exists but the root does not say where it belongs. Reported, never guessed.
    Unidentified { root: PathBuf, reason: String },
    /// Backups whose destinations the root itself recorded.
    Candidates(Vec<Candidate>),
}

/// Read what one root left behind, from its own journal.
pub fn interrupted_publication(root: &Path) -> Interrupted {
    let backup = root.join("backup");
    if !backup.is_dir() {
        return Interrupted::Nothing;
    }
    let saved: Vec<String> = match std::fs::read_dir(&backup) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .collect(),
        Err(error) => {
            return Interrupted::Unidentified {
                root: root.to_path_buf(),
                reason: format!("the backup could not be read: {error}"),
            }
        }
    };
    if saved.is_empty() {
        return Interrupted::Nothing;
    }
    let journal = match PublicationJournal::read(root) {
        Ok(Some(journal)) => journal,
        Ok(None) => {
            return Interrupted::Unidentified {
                root: root.to_path_buf(),
                reason: "it has no publication journal, so where its files belong is unknown"
                    .to_string(),
            }
        }
        Err(reason) => {
            return Interrupted::Unidentified {
                root: root.to_path_buf(),
                reason: format!("its publication journal is unusable: {reason}"),
            }
        }
    };

    let mut candidates = Vec::new();
    for id in saved {
        let Some(member) = journal.members.iter().find(|member| member.id == id) else {
            return Interrupted::Unidentified {
                root: root.to_path_buf(),
                reason: format!("its journal does not say where {id} belongs"),
            };
        };
        candidates.push(Candidate {
            root: root.to_path_buf(),
            id,
            destination: member.destination.clone(),
            saved: backup.join(&member.id),
        });
    }
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    Interrupted::Candidates(candidates)
}

/// What a recovery sweep did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Recovered {
    /// Destinations put back, as `(id, destination)`.
    pub restored: Vec<(String, PathBuf)>,
    /// Backups whose destination is **already present**, so nothing was touched.
    ///
    /// Either the swap completed and only the cleanup did not, or something has been installed
    /// since. Both are cases where restoring would destroy a live copy to reinstate an older one.
    ///
    /// **This means what it says.** A restoration that *failed* used to land here too, which told
    /// a reader the destination already existed when it may well not have — the one thing they
    /// would check first, reported wrongly. Those are [`Self::failed`] now.
    pub retained: Vec<(String, PathBuf)>,
    /// Restorations that were attempted and did not work.
    ///
    /// Kept separate because the two need different words and different actions: a retained backup
    /// is a decision nobody has made, and this is a failure somebody has to look at. The reason
    /// travels with it, because "recovery failed" without one is not something a person can act on.
    pub failed: Vec<FailedRestoration>,
    /// Destinations more than one abandoned install offers to restore.
    ///
    /// **Never resolved by enumeration order.** Two roots holding a backup for the same missing
    /// path is not a case where "the first one found" is a defensible answer — they are different
    /// installs from different moments, and choosing between them is a person's call. Reported
    /// with every claimant so that choice can be made.
    pub ambiguous: Vec<(PathBuf, Vec<PathBuf>)>,
    /// Roots that left something behind without saying where it belongs.
    pub unidentified: Vec<(PathBuf, String)>,
}

/// A restoration that was attempted and did not work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedRestoration {
    pub id: String,
    /// Where it was going.
    pub destination: PathBuf,
    /// Where the bytes still are — nothing is lost by a failure here.
    pub saved: PathBuf,
    pub reason: String,
}

/// Restore what is unambiguous, and report the rest.
///
/// **Gathered before anything is moved.** The sweep used to restore as it walked, so the first root
/// the filesystem happened to enumerate won a destination and any other claimant was merely
/// "retained" afterwards — an answer decided by directory order, which is no answer at all.
pub fn recover_interrupted_publications(roots: &[PathBuf]) -> Recovered {
    let mut recovered = Recovered::default();
    let mut candidates: Vec<Candidate> = Vec::new();
    for root in roots {
        match interrupted_publication(root) {
            Interrupted::Nothing => {}
            Interrupted::Unidentified { root, reason } => {
                recovered.unidentified.push((root, reason))
            }
            Interrupted::Candidates(found) => candidates.extend(found),
        }
    }

    // One pass to find contested destinations, before a single rename.
    let mut by_destination: BTreeMap<PathBuf, Vec<Candidate>> = BTreeMap::new();
    for candidate in candidates {
        by_destination
            .entry(candidate.destination.clone())
            .or_default()
            .push(candidate);
    }

    for (destination, claimants) in by_destination {
        if claimants.len() > 1 {
            recovered.ambiguous.push((
                destination,
                claimants
                    .into_iter()
                    .map(|candidate| candidate.root)
                    .collect(),
            ));
            continue;
        }
        let candidate = &claimants[0];
        if destination.exists() {
            recovered
                .retained
                .push((candidate.id.clone(), candidate.saved.clone()));
            continue;
        }
        if let Some(parent) = destination.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                recovered.failed.push(FailedRestoration {
                    id: candidate.id.clone(),
                    destination: destination.clone(),
                    saved: candidate.saved.clone(),
                    reason: format!("the destination's parent could not be created: {error}"),
                });
                continue;
            }
        }
        match std::fs::rename(&candidate.saved, &destination) {
            Ok(()) => recovered
                .restored
                .push((candidate.id.clone(), destination.clone())),
            // Nothing is lost by this: the backup is still exactly where it was, which is what the
            // reason and the retained path together have to make clear.
            Err(error) => recovered.failed.push(FailedRestoration {
                id: candidate.id.clone(),
                destination: destination.clone(),
                saved: candidate.saved.clone(),
                reason: error.to_string(),
            }),
        }
    }
    recovered
}

/// Publish prepared files or directories. The caller owns the staging directory and must
/// retain it on error. `id` is a single path component used only for the backup name.
pub fn publish_prepared(
    root: &Path,
    prepared: &[(String, PathBuf, PathBuf)],
) -> Result<(), String> {
    publish_with_commit(root, prepared, || Ok(()))
}

/// Activate a whole offline package: directory-swap components and prepared dictionary
/// generations, together.
///
/// # Two publication mechanisms, one transaction
///
/// Most components are a set of files, and replacing them is a directory swap with the rollback
/// [`publish_with_commit`] already implements. **Dictionaries are not**, and treating them as one
/// is the defect this function exists to remove: their live container holds immutable generations
/// that open readers hold leases on, so swapping the container away takes bytes a reader is still
/// resolving to. `core/tests/offline_dictionary_publication.rs` is that reproduction, and one of
/// its two faces is silent: a missing shard directory is cached as an empty map, so the reader
/// reports "no such word" — indistinguishable from a dictionary that genuinely lacks it. A missing
/// file errors instead. The silent face is why it could have shipped.
///
/// So dictionaries are prepared in their live container ([`Dictionary::prepare`]) and activated
/// here, inside the same rollback boundary as the swaps.
///
/// # What is atomic, and what is merely recoverable
///
/// Stated exactly, because the difference is what a caller may rely on:
///
/// | | |
/// |---|---|
/// | One directory swap | **two renames** — the old directory aside, then the replacement in. Not an atomic replacement: an interruption between them leaves the destination absent, with the old copy under `backup/<id>`. |
/// | One dictionary activation | a rename into `g-N` and one atomic pointer write. The pointer write is the switch and *is* atomic, so a reader sees the old generation or the new one, never neither. |
/// | **The package as a whole** | **not atomic** — it is *recoverable*, and only as far as the rollback itself succeeds. |
///
/// **There is no instant at which all components change together, and none is claimed.** A reader
/// looking at the exact moment between two activations sees one new component and one old one;
/// both are internally consistent and both answer.
///
/// **Nor is recovery unconditional.** Rolling back is itself file-system work and can fail —
/// a destination that cannot be renamed back, a pointer that cannot be rewritten. When that
/// happens nothing is silently abandoned: the backup directory is retained, the surviving copies
/// are named in the error, and no superseded generation has been reclaimed, so the state is
/// recoverable by hand. What is guaranteed is narrower than "everything is as it was" and worth
/// stating exactly: **no live reader's bytes are destroyed, and whatever could not be restored is
/// reported rather than lost.**
///
/// Pruning is deferred until the whole activation has succeeded. That is what makes the roll-back
/// above possible: the superseded generation is still on disk until [`PreparedGeneration::settle`]
/// runs, one call after the last thing that could fail.
pub fn publish_offline(
    root: &Path,
    prepared: &[(String, PathBuf, PathBuf)],
    dictionaries: Vec<crate::dict::PreparedGeneration>,
) -> Result<(), String> {
    let mut dictionaries = dictionaries;
    let outcome = publish_with_commit(root, prepared, || {
        for index in 0..dictionaries.len() {
            if let Err(error) = dictionaries[index].activate() {
                // Newest first, so each container passes back through the state before it.
                let mut failures = Vec::new();
                for earlier in (0..index).rev() {
                    if let Err(failed) = dictionaries[earlier].roll_back() {
                        failures.push(failed.to_string());
                    }
                }
                // A roll-back that itself failed is worth saying out loud: the generation bytes
                // are all still on disk — nothing was pruned — so this is recoverable by hand,
                // and silence would be the only thing making it not.
                let trailing = if failures.is_empty() {
                    String::new()
                } else {
                    format!(
                        " Some dictionaries could not be returned to their previous generation \
                         ({}); their old generations are still on disk.",
                        failures.join("; ")
                    )
                };
                return Err(format!(
                    "Could not activate a dictionary: {error}.{trailing}"
                ));
            }
        }
        Ok(())
    });

    match outcome {
        Ok(()) => {
            // Past the last thing that could fail, so the superseded generations are no longer
            // needed for a roll-back. Lease-aware: whatever a live reader still holds stays until
            // that reader is dropped, which is the guarantee the whole exercise is about.
            // A settle that cannot clear its record has not finished: the record left behind
            // blocks every later reclamation on that container and refuses the next install. The
            // package succeeded, so this is reported as what it is — a completed publication with
            // outstanding bookkeeping — rather than passed over in silence.
            let mut outstanding = Vec::new();
            for prepared in dictionaries.drain(..) {
                if let Err(error) = prepared.settle() {
                    outstanding.push(error.to_string());
                }
            }
            if outstanding.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "The package was published, but its recovery records could not be cleared ({}).                      The dictionaries are the new ones; resolve the records before installing again.",
                    outstanding.join("; ")
                ))
            }
        }
        // Dropping an unactivated `PreparedGeneration` removes its staging directory; one that was
        // activated and rolled back leaves its generation in place, to be reclaimed by a later
        // publish. Where a roll-back *failed*, the error above names it and the bytes are still on
        // disk — the one thing this must not do is return quietly as though they were not.
        Err(error) => Err(error),
    }
}

/// Include the final activation edge in the rollback boundary.
pub fn publish_with_commit(
    root: &Path,
    prepared: &[(String, PathBuf, PathBuf)],
    commit: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    let mut destinations = std::collections::HashSet::new();
    for (id, _, destination) in prepared {
        let mut parts = Path::new(id).components();
        if !matches!(parts.next(), Some(std::path::Component::Normal(_)))
            || parts.next().is_some()
            || !ids.insert(id)
            || !destinations.insert(destination)
        {
            return Err(format!("Invalid or repeated publication member {id:?}"));
        }
    }
    // **A dictionary container is never a directory-swap member, and this refuses rather than
    // trusting callers to remember.**
    //
    // Its generations are immutable and open readers hold leases on them by path, so replacing the
    // container takes bytes a live reader is still resolving to — silently, because a missing
    // shard reads as "no such word". `core/tests/offline_dictionary_publication.rs` reproduces it.
    // The offline installer used to do exactly this; it now prepares generations in place and
    // activates them through `publish_offline`.
    //
    // Checked here because a test that the *current* shell picks the right path cannot stop the
    // next caller picking the wrong one. This can.
    for (id, _, destination) in prepared {
        if is_dictionary_container(destination) {
            return Err(format!(
                "{id} names a dictionary container ({}). Publish it as immutable generations instead — see Dictionary::prepare.",
                destination.display()
            ));
        }
    }

    let backup = root.join("backup");
    // A stale backup may be somebody's only surviving copy. Never clear it for a retry.
    std::fs::create_dir(&backup)
        .map_err(|error| format!("Cannot create fresh backup {}: {error}", backup.display()))?;
    // **Before the first rename**, so an interruption anywhere after this leaves evidence of where
    // each component belongs. Reconstructing that later from the running configuration is what
    // would send a backup somewhere its own install had never heard of.
    PublicationJournal {
        members: prepared
            .iter()
            .map(|(id, _, destination)| JournalMember {
                id: id.clone(),
                destination: destination.clone(),
            })
            .collect(),
    }
    .write(root)?;
    let mut old = Vec::new();
    let mut published = Vec::new();
    let result = (|| {
        for (id, staged, live) in prepared {
            if let Some(parent) = live.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let saved = backup.join(id);
            if live.exists() {
                std::fs::rename(live, &saved).map_err(|error| error.to_string())?;
                old.push((saved, live.clone()));
            }
            std::fs::rename(staged, live).map_err(|error| error.to_string())?;
            published.push((live.clone(), staged.clone()));
        }
        commit()
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        // Return new files to staging rather than deleting them. A retry can use verified
        // downloads, and a failed restore cannot silently erase either generation.
        for (live, staged) in published.iter().rev() {
            if let Err(error) = std::fs::rename(live, staged) {
                failures.push(format!("{}: {error}", live.display()));
            }
        }
        for (saved, live) in old.iter().rev() {
            if let Err(error) = std::fs::rename(saved, live) {
                failures.push(format!("{}: {error}", live.display()));
            }
        }
        if failures.is_empty() {
            let _ = std::fs::remove_dir(&backup);
            return Err(error);
        }
        return Err(format!(
            "{error}; restoration also failed: {}. Recovery files retained at {}",
            failures.join("; "),
            backup.display()
        ));
    }
    // The complete new set is live. A locked backup is a disk cost, not a failed install.
    let _ = std::fs::remove_dir_all(&backup);
    Ok(())
}

/// Publish a named set into one destination, using a fresh recovery directory per attempt.
pub fn publish_set(stage: &Path, destination: &Path, names: &[&str]) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let root = destination.join(format!(
        ".publication-{}-{nonce}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let prepared: Vec<_> = names
        .iter()
        .map(|name| (name.to_string(), stage.join(name), destination.join(name)))
        .collect();
    let result = publish_prepared(&root, &prepared);
    // remove_dir only succeeds when the recovery directory is empty, never when it holds data.
    let _ = std::fs::remove_dir(&root);
    result
}
