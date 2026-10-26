//! Carrying completed cases between invocations, safely.
//!
//! Extracted from `colloquial_comparison.rs`, where it was written and proved, and generalised so
//! a second evaluation can key on the fields *it* needs rather than on that harness's five.
//!
//! # What makes a resume safe is the identity, not the file
//!
//! Reusing a case measured by a different binary, against different dictionaries, over a different
//! sample, through a different engine route, or scored by a different method would silently mix two
//! measurements into one report — and the artifact would name only the second. So all of those are
//! part of the key, a mismatch refuses rather than resuming, and **the refusal names which field
//! differed** so it can be diagnosed instead of guessed at.
//!
//! # Two things the original did that this does not
//!
//! - **It wrote with a plain `fs::write`.** That truncates the destination first, so an invocation
//!   interrupted mid-write left a half-written checkpoint where a usable one had been — losing the
//!   work the checkpoint existed to protect. Writes now go to a staging file and are renamed into
//!   place, so the destination is either the old checkpoint or the new one and never a fragment.
//! - **A refused checkpoint was discarded**, and the next `save` overwrote it. Refusing to *reuse*
//!   evidence is right; deleting it is not. A refused file is now moved aside under a name derived
//!   from its own bytes, so it survives and the run says where it went — and **if it cannot be
//!   preserved, nothing replaces it.** Preservation that fails quietly and then lets the write
//!   proceed is the same data loss with an extra step.

// Every integration test binary compiles `common/` whole, so a harness that does not
// touch this module leaves all of it unread. Same reason `context_retrieval.rs` carries
// this.
#![allow(dead_code)]

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One invocation's wall-clock allowance.
///
/// Exceeding it leaves the remaining cases *incomplete*, which fails the run — a partial
/// measurement must never read as a finished one — and [`Resume`] lets the next invocation finish
/// them.
pub const BUDGET: Duration = Duration::from_secs(60 * 60);

/// The fields a resume is keyed on, in a stable order.
///
/// A map rather than a fixed struct because different evaluations are made incomparable by
/// different things: one cares about an Ichiran route, another about which sentences were sampled
/// and which scoring rule was applied. **A field a harness forgets to add is a way two runs can
/// differ without the resume noticing**, so the type makes adding one cheap and the artifact
/// records every field by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity(BTreeMap<String, String>);

impl Identity {
    pub fn new() -> Identity {
        Identity(BTreeMap::new())
    }

    pub fn with(mut self, field: &str, value: impl Into<String>) -> Identity {
        self.0.insert(field.to_string(), value.into());
        self
    }

    pub fn describe(&self) -> Value {
        json!(self.0)
    }

    /// Which fields differ, **named individually so a refusal is diagnosable**.
    ///
    /// A field present in one identity and absent from the other counts as a difference: a run that
    /// started keying on the sampled workload must not silently reuse cases from one that did not.
    pub fn differences(&self, other: &Identity) -> Vec<String> {
        let mut differences = Vec::new();
        for (field, value) in &self.0 {
            match other.0.get(field) {
                Some(theirs) if theirs == value => {}
                Some(_) => differences.push(field.clone()),
                None => differences.push(format!("{field} (absent from the recorded run)")),
            }
        }
        for field in other.0.keys() {
            if !self.0.contains_key(field) {
                differences.push(format!("{field} (recorded but no longer keyed on)"));
            }
        }
        differences.sort();
        differences
    }
}

/// Per-case results carried between invocations.
pub struct Resume {
    path: PathBuf,
    identity: Identity,
    completed: BTreeMap<String, Value>,
    refused: Option<String>,
    superseded: Option<PathBuf>,
    /// Why the previous checkpoint could not be kept. **Blocks [`Resume::save`]**: replacing a
    /// checkpoint that could not be preserved destroys it just as surely as deleting it.
    preservation_failed: Option<String>,
}

impl Resume {
    /// Open a checkpoint, reusing it only under a matching identity.
    pub fn open(path: PathBuf, identity: Identity) -> Resume {
        let mut resume = Resume {
            path,
            identity,
            completed: BTreeMap::new(),
            refused: None,
            superseded: None,
            preservation_failed: None,
        };
        let Ok(raw) = std::fs::read(&resume.path) else {
            return resume;
        };
        let Ok(document) = serde_json::from_slice::<Value>(&raw) else {
            resume.refused = Some("the resume file could not be parsed".into());
            resume.supersede(&raw);
            return resume;
        };
        let recorded = Identity(
            document["identity"]
                .as_object()
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(|(field, value)| {
                            value
                                .as_str()
                                .map(|value| (field.clone(), value.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        );
        let differences = resume.identity.differences(&recorded);
        if !differences.is_empty() {
            resume.refused = Some(format!(
                "the recorded run differs in: {}. Reusing it would mix two measurements into one \
                 report, so it is not resumed",
                differences.join(", ")
            ));
            resume.supersede(&raw);
            return resume;
        }
        if let Some(cases) = document["cases"].as_object() {
            for (id, record) in cases {
                resume.completed.insert(id.clone(), record.clone());
            }
        }
        resume
    }

    /// Move a checkpoint this run will not reuse out of the way, **without destroying it**.
    ///
    /// Named from its own bytes, so moving the same file twice is idempotent and two different
    /// refused checkpoints never collide.
    fn supersede(&mut self, raw: &[u8]) {
        let digest = sakatsu_core::hashing::hex(Sha256::digest(raw));
        let aside = self
            .path
            .with_extension(format!("superseded-{}.json", &digest[..16]));

        // **An occupied aside path is not proof of preservation.** The name derives from the
        // bytes, so a file already there *should* hold them — but "should" is not a check, and a
        // directory, a truncated file or an unrelated file of the same name would all have been
        // accepted as a successful preservation and licensed the overwrite that follows.
        match std::fs::metadata(&aside) {
            Ok(existing) if existing.is_file() => {
                match std::fs::read(&aside) {
                    Ok(bytes) if bytes == raw => self.superseded = Some(aside),
                    Ok(_) => {
                        self.preservation_failed = Some(format!(
                            "{} already exists and does not hold the checkpoint's bytes",
                            aside.display()
                        ))
                    }
                    Err(error) => {
                        self.preservation_failed = Some(format!(
                            "{} exists but could not be read to verify it ({error})",
                            aside.display()
                        ))
                    }
                }
                return;
            }
            Ok(_) => {
                self.preservation_failed = Some(format!(
                    "{} is occupied by something that is not a file",
                    aside.display()
                ));
                return;
            }
            Err(_) => {}
        }

        match std::fs::rename(&self.path, &aside) {
            Ok(()) => self.superseded = Some(aside),
            Err(error) => {
                self.preservation_failed = Some(format!(
                    "could not move {} aside ({error})",
                    self.path.display()
                ))
            }
        }
    }

    /// Why the previous checkpoint could not be preserved, when it could not be.
    pub fn preservation_failed(&self) -> Option<&str> {
        self.preservation_failed.as_deref()
    }

    /// Cases an earlier invocation finished, empty when nothing is reusable.
    pub fn completed(&self) -> &BTreeMap<String, Value> {
        &self.completed
    }

    /// Why a checkpoint was not reused, when one was found and rejected.
    pub fn refused(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// Where a rejected checkpoint was kept.
    pub fn superseded(&self) -> Option<&Path> {
        self.superseded.as_deref()
    }

    /// Where the staging file goes. Beside the destination, so the rename stays on one filesystem.
    fn staging(&self) -> PathBuf {
        self.path.with_extension("partial")
    }

    /// Write the checkpoint **atomically**.
    ///
    /// The destination is replaced by a rename, so a reader — or a later invocation — sees either
    /// the previous checkpoint or this one. It never sees half of either.
    pub fn save(&self, cases: &BTreeMap<String, Value>) -> Result<(), String> {
        // **Nothing replaces a checkpoint that could not be kept.** Otherwise the refusal to reuse
        // it — which is a safety property — becomes the occasion for losing it.
        if let Some(reason) = &self.preservation_failed {
            return Err(format!(
                "refusing to replace {}: the checkpoint it would overwrite could not be preserved                  ({reason})",
                self.path.display()
            ));
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let document = json!({
            "identity": self.identity.describe(),
            "note": "reusable only under an identical identity; see common::resume",
            "cases": cases,
        });
        let body = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("could not serialise the resume file: {error}"))?;
        let staging = self.staging();
        std::fs::write(&staging, body)
            .map_err(|error| format!("could not write {}: {error}", staging.display()))?;
        std::fs::rename(&staging, &self.path)
            .map_err(|error| format!("could not publish {}: {error}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Identity {
        Identity::new()
            .with("executable", "abc")
            .with("source_tree", "def")
            .with("workload", "ghi")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("sakatsu-resume-tests").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir.join("resume.json")
    }

    fn one(id: &str) -> BTreeMap<String, Value> {
        BTreeMap::from([(id.to_string(), json!({ "outcome": "failed" }))])
    }

    #[test]
    fn a_matching_identity_restores_the_cases() {
        let path = scratch("matching");
        Resume::open(path.clone(), identity())
            .save(&one("case-1"))
            .expect("save");

        let resumed = Resume::open(path, identity());
        assert!(resumed.refused().is_none());
        assert_eq!(resumed.completed().len(), 1);
        assert_eq!(
            resumed.completed()["case-1"]["outcome"],
            json!("failed"),
            "a case comes back as what it was, never as a pass"
        );
    }

    /// **A refusal names the field**, so it can be diagnosed rather than guessed at.
    #[test]
    fn a_differing_field_refuses_and_says_which() {
        let path = scratch("differing");
        Resume::open(path.clone(), identity())
            .save(&one("case-1"))
            .expect("save");

        let moved_on = identity().with("workload", "a different sample");
        let resumed = Resume::open(path, moved_on);
        let refusal = resumed.refused().expect("refused");
        assert!(refusal.contains("workload"), "{refusal}");
        assert!(
            !refusal.contains("executable"),
            "only the changed field: {refusal}"
        );
        assert!(resumed.completed().is_empty(), "nothing is reused");
    }

    /// A field one run keys on and the other does not is a difference, not a match.
    #[test]
    fn a_newly_keyed_field_refuses_rather_than_matching_loosely() {
        let path = scratch("newly-keyed");
        Resume::open(path.clone(), identity())
            .save(&one("case-1"))
            .expect("save");

        let stricter = identity().with("scoring_method", "v2");
        let resumed = Resume::open(path, stricter);
        let refusal = resumed.refused().expect("refused");
        assert!(refusal.contains("scoring_method"), "{refusal}");
    }

    /// **Refusing to reuse is not licence to delete.**
    #[test]
    fn a_refused_checkpoint_is_kept_rather_than_destroyed() {
        let path = scratch("kept");
        Resume::open(path.clone(), identity())
            .save(&one("case-1"))
            .expect("save");
        let original = std::fs::read(&path).expect("read");

        let resumed = Resume::open(path.clone(), identity().with("workload", "different"));
        let aside = resumed
            .superseded()
            .expect("the old checkpoint is kept somewhere");
        assert_eq!(
            std::fs::read(aside).expect("the superseded file reads"),
            original,
            "the refused checkpoint survives byte for byte"
        );

        // And the new run may now write its own without having destroyed the old evidence.
        resumed.save(&one("case-2")).expect("save");
        assert!(
            aside.exists(),
            "still there after the new checkpoint is written"
        );
    }

    /// An unparseable checkpoint is refused and kept, not silently treated as absent.
    #[test]
    fn an_unparseable_checkpoint_is_refused_and_kept() {
        let path = scratch("unparseable");
        std::fs::write(&path, b"{ not json").expect("write");
        let resumed = Resume::open(path, identity());
        assert!(resumed.refused().is_some());
        assert!(resumed.completed().is_empty());
        assert!(
            resumed.superseded().is_some_and(|aside| aside.exists()),
            "an unreadable checkpoint is evidence too"
        );
    }

    /// **A failed staging write leaves the previous checkpoint usable.**
    ///
    /// Stated at the width it is actually proved: a directory occupies the staging path, so the
    /// write fails at exactly the point a `fs::write` straight to the destination would already
    /// have truncated it. That is the difference between the two designs, and it is what this
    /// shows.
    ///
    /// It does **not** reproduce a killed process mid-write, and it says nothing about durability
    /// across power loss — `rename` is atomic with respect to readers, not a guarantee that the
    /// bytes reached the platter.
    #[test]
    fn a_failed_write_does_not_destroy_the_previous_checkpoint() {
        let path = scratch("interrupted");
        let resume = Resume::open(path.clone(), identity());
        resume.save(&one("case-1")).expect("the first save works");
        let before = std::fs::read(&path).expect("read");

        std::fs::create_dir_all(path.with_extension("partial")).expect("block the staging path");
        let failure = resume
            .save(&one("case-2"))
            .expect_err("the staged write cannot succeed");
        assert!(failure.contains("could not write"), "{failure}");

        assert_eq!(
            std::fs::read(&path).expect("the destination still reads"),
            before,
            "the previous checkpoint survived a failed write"
        );
    }

    /// **Preservation that fails must stop the overwrite**, or refusing to reuse a checkpoint
    /// becomes the occasion for destroying it.
    #[test]
    fn a_checkpoint_that_cannot_be_preserved_is_not_replaced() {
        let path = scratch("unpreservable");
        Resume::open(path.clone(), identity())
            .save(&one("case-1"))
            .expect("save");
        let original = std::fs::read(&path).expect("read");

        // Block the aside path with a directory, so the move cannot happen.
        let raw = std::fs::read(&path).expect("read");
        let digest = sakatsu_core::hashing::hex(Sha256::digest(&raw));
        let aside = path.with_extension(format!("superseded-{}.json", &digest[..16]));
        std::fs::create_dir_all(&aside).expect("occupy the aside path");

        let resumed = Resume::open(path.clone(), identity().with("workload", "different"));
        assert!(resumed.refused().is_some(), "the identity still differs");
        assert!(
            resumed.preservation_failed().is_some(),
            "the failure to preserve must be recorded"
        );
        assert!(resumed.superseded().is_none(), "nothing was preserved");

        let refusal = resumed
            .save(&one("case-2"))
            .expect_err("saving must refuse rather than overwrite unpreserved evidence");
        assert!(refusal.contains("could not be preserved"), "{refusal}");
        assert_eq!(
            std::fs::read(&path).expect("read"),
            original,
            "the original checkpoint is untouched"
        );
    }

    /// An aside path already holding *different* bytes is not proof the checkpoint was kept.
    #[test]
    fn an_occupied_aside_path_is_verified_rather_than_trusted() {
        let path = scratch("occupied");
        Resume::open(path.clone(), identity())
            .save(&one("case-1"))
            .expect("save");
        let original = std::fs::read(&path).expect("read");
        let digest = sakatsu_core::hashing::hex(Sha256::digest(&original));
        let aside = path.with_extension(format!("superseded-{}.json", &digest[..16]));
        std::fs::write(&aside, b"not the checkpoint").expect("occupy with a different file");

        let resumed = Resume::open(path.clone(), identity().with("workload", "different"));
        assert!(
            resumed.preservation_failed().is_some(),
            "a same-named file with different bytes must not count as preservation"
        );
        assert!(resumed.save(&one("case-2")).is_err());
        assert_eq!(std::fs::read(&path).expect("read"), original);
    }

    /// The same checkpoint superseded twice is idempotent — the bytes are already there.
    #[test]
    fn preserving_the_same_checkpoint_twice_is_accepted() {
        let path = scratch("idempotent");
        Resume::open(path.clone(), identity())
            .save(&one("case-1"))
            .expect("save");
        let original = std::fs::read(&path).expect("read");
        let digest = sakatsu_core::hashing::hex(Sha256::digest(&original));
        let aside = path.with_extension(format!("superseded-{}.json", &digest[..16]));
        std::fs::write(&aside, &original).expect("already preserved, byte for byte");

        let resumed = Resume::open(path.clone(), identity().with("workload", "different"));
        assert!(
            resumed.preservation_failed().is_none(),
            "identical bytes at the aside path really are the checkpoint"
        );
        assert_eq!(resumed.superseded(), Some(aside.as_path()));
        resumed
            .save(&one("case-2"))
            .expect("and the new run may proceed");
    }

    /// Nothing at all is not a refusal — a first run has no checkpoint and that is ordinary.
    #[test]
    fn an_absent_checkpoint_is_not_a_refusal() {
        let resumed = Resume::open(scratch("absent"), identity());
        assert!(resumed.refused().is_none());
        assert!(resumed.superseded().is_none());
        assert!(resumed.completed().is_empty());
    }
}
