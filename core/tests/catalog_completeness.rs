//! Every artifact in the catalog, and what this release knows about it.
//!
//! # Why this could not be written before
//!
//! `expected_bytes` and `sha256` were `match self.file` tables a hundred lines from the artifacts
//! they described, keyed on filename and disambiguated by `if self.url.contains(…)`. Two
//! consequences followed, and both were live:
//!
//! - **They could not be enumerated.** Asking "which artifacts have no digest?" meant reading two
//!   match statements against a list of artifacts by eye. So nobody did, and the tables kept two
//!   arms for a manga-ocr export the catalog had stopped shipping — dead entries that could not be
//!   reached, because no artifact had that URL any more.
//! - **An unknown filename returned `None` in silence**, which `setup_manager.rs` turned into an
//!   empty expected-hash string and then compared a real digest against.
//!
//! With the values beside the URL, both questions are a `for` loop. This file is that loop, and it
//! pins the answers so a gap becomes a decision somebody made rather than one nobody noticed.
//!
//! It needs nothing installed: the catalog is compiled in.

use std::collections::BTreeSet;

use sakatsu_core::setup::catalog;

/// Every artifact, as `component/file`.
fn artifacts() -> Vec<(String, &'static catalog::Artifact)> {
    catalog::all()
        .iter()
        .flat_map(|component| {
            component
                .artifacts
                .iter()
                .map(move |artifact| (format!("{}/{}", component.id, artifact.file), artifact))
        })
        .collect()
}

/// The artifacts this release knowingly ships without an exact digest.
///
/// **A list, not a rule.** Each is here because no publisher digest was available at the pinned
/// commit, and each is still size-checked. Adding to it is a decision that shows up in a diff.
const UNVERIFIED: &[&str] = &[
    // The llama.cpp Windows build and the manga-tuned VLM weights: release assets whose publisher
    // states no digest. `min_bytes` and the exact size are what stand behind them.
    "paddle-vl/llama-vulkan.zip",
    "paddle-vl/PaddleOCR-VL-For-Manga-BF16.gguf",
    "paddle-vl/PaddleOCR-VL-For-Manga-mmproj-BF16.gguf",
    // (FuguMT used to be listed here, served from a moving `main`. Since 2026-10-18 it is pinned to
    // commit 9b09cd7e with exact sizes and digests, and the files at that commit are byte-identical
    // to the copy every translation measurement used.)
    //
    // Optional extras fetched from a moving branch, so a digest would be a claim about a file
    // that can change under it. Pinning the commit is what would fix each.
    "pronunciation/tofugu-pronunciation-master.zip",
    "frequency-jpdb/jpdb-frequency.zip",
    "frequency-bccwj/bccwj-frequency.zip",
];

/// Which artifacts carry an exact digest, said exactly.
#[test]
fn every_artifact_without_a_digest_is_one_this_release_named() {
    let expected: BTreeSet<&str> = UNVERIFIED.iter().copied().collect();
    let actual: BTreeSet<String> = artifacts()
        .into_iter()
        .filter(|(_, artifact)| !artifact.verifiable())
        .map(|(name, _)| name)
        .collect();
    let actual_refs: BTreeSet<&str> = actual.iter().map(String::as_str).collect();

    assert_eq!(
        actual_refs, expected,
        "the set of artifacts with no pinned digest changed. Adding one is a decision — a \
         download that cannot be checked against a digest is checked only against its size — so \
         it belongs in UNVERIFIED with a reason, not in a diff nobody reads."
    );
}

/// An artifact with a digest also has an exact size, and vice versa where it should.
///
/// The pair is what `setup_manager` verifies against, and a digest with no size means a truncated
/// download is caught one step later than it could be.
#[test]
fn a_digest_never_arrives_without_a_size() {
    for (name, artifact) in artifacts() {
        if artifact.verifiable() {
            assert!(
                artifact.expected_bytes().is_some(),
                "{name} pins a digest but no exact size; the two are verified together"
            );
        }
        assert!(
            artifact.min_bytes > 0,
            "{name} has no floor at all, so an error page served with a 200 would pass"
        );
    }
}

/// No two artifacts in one component land on the same filename.
///
/// **This is the failure the old tables were shaped around.** Two artifacts sharing a filename is
/// why the byte and hash lookups needed `if self.url.contains(…)` at all, and why an artifact
/// could not be asked what it expected without also being asked where it came from. Within a
/// component they must not collide — they land in the same directory — and across components a
/// repeated name is fine and common (`config.json`).
#[test]
fn filenames_do_not_collide_inside_a_component() {
    for component in catalog::all() {
        let mut seen = BTreeSet::new();
        for artifact in component.artifacts {
            assert!(
                seen.insert(artifact.file),
                "{} lists {} twice; they would overwrite each other on disk",
                component.id,
                artifact.file
            );
        }
    }
}

/// The gate can detect what it claims to detect.
///
/// Every drift check in this repository carries one of these, and this project treats that as
/// mandatory — `credits_match_manifests.rs` set the pattern.
#[test]
fn assert_the_gate_has_teeth() {
    let all = artifacts();
    assert!(
        all.len() > 20,
        "only {} artifacts were enumerated, which is too few to be the catalog — a loop that \
         found nothing would pass every assertion above it",
        all.len()
    );
    assert!(
        all.iter().any(|(_, artifact)| artifact.verifiable()),
        "no artifact carries a digest at all, so the UNVERIFIED comparison is vacuous"
    );
    assert!(
        !UNVERIFIED.is_empty(),
        "an empty UNVERIFIED list would make the equality above pass by describing nothing"
    );
    // The named list must actually name artifacts that exist. A stale entry would quietly widen
    // the allowance for whatever replaced it — the same failure the segmentation gate's pinned
    // exceptions guard against.
    let names: BTreeSet<String> = all.iter().map(|(name, _)| name.clone()).collect();
    for entry in UNVERIFIED {
        assert!(
            names.contains(*entry),
            "UNVERIFIED names {entry}, which is not in the catalog. A stale allowance is how a \
             gap gets inherited by something else."
        );
    }
}
