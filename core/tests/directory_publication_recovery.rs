//! The directory-swap half of publication: failed restoration, and interruption.
//!
//! `offline_dictionary_publication.rs` covers the dictionary half — generations, leases, recovery
//! records. This covers the other mechanism, which most components use and which has a different
//! failure shape: **a swap is two renames**, the old destination aside into `backup/<id>` and the
//! replacement in. Neither the pair nor the rollback is atomic, and both can fail part-way.
//!
//! What is asserted here is what a caller may rely on when they do:
//!
//! - a restoration that fails **retains the backup and names it**, rather than returning an error
//!   that leaves the only surviving copy somewhere nobody will look;
//! - an interruption between the two renames is **recoverable**, which until now it was not — the
//!   installer's root is a per-run temporary directory, so a retained backup was a directory
//!   nobody would ever open again;
//! - where each backup belongs comes from **the install that made it**, through a journal written
//!   before the first rename, not from whatever the configuration says today;
//! - two installs offering the same destination are **reported, never decided by directory order**;
//! - recovery is repeatable, and refuses to overwrite a destination that is present.

use sakatsu_core::setup::publication::{
    interrupted_publication, publish_prepared, publish_with_commit,
    recover_interrupted_publications, Interrupted,
};
use std::path::Path;

#[path = "common/scratch.rs"]
mod scratch_guard;

fn scratch(name: &str) -> scratch_guard::Scratch {
    scratch_guard::Scratch::new("publication", name)
}

/// A directory holding one named file, so a restoration can be checked by its contents rather than
/// by its mere existence — an empty directory in the right place would satisfy the weaker check.
fn tree(path: &Path, marker: &str) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join("marker.txt"), marker.as_bytes()).unwrap();
}

fn marker_of(path: &Path) -> String {
    std::fs::read_to_string(path.join("marker.txt")).unwrap_or_default()
}

/// Leave a root in the state an interruption between the two renames produces: the destination
/// missing, the backup holding the only copy, and the journal the real publication wrote.
fn interrupt_a_publication(root: &Path, destination: &Path, marker: &str) {
    let staged = root.join("staged");
    tree(&staged, "new");
    tree(destination, marker);
    let members = [("component".to_string(), staged, destination.to_path_buf())];
    publish_with_commit(root, &members, || Err("stopped".to_string()))
        .expect_err("the commit refused");
    // The rollback put the old copy back and cleared the empty backup directory, so both halves
    // are recreated here: the backup holding the only copy, and the destination gone. That is
    // where a process stopped between the two renames would have left it.
    let backup = root.join("backup");
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::rename(destination, backup.join("component")).unwrap();
}

/// A restoration that cannot complete keeps the backup and says where it is.
#[test]
fn a_failed_restoration_retains_the_backup_and_names_it() {
    let root = scratch("failed-restore");
    let staged = root.join("staged");
    let live = root.join("live");
    tree(&staged, "new");
    tree(&live, "old");

    let members = [("component".to_string(), staged.clone(), live.clone())];
    let error = publish_with_commit(&root, &members, || {
        // The swap has happened by now. Occupying the staging path here is what makes the
        // *rollback* fail rather than the publication.
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("occupied"), b"x").unwrap();
        Err("the commit refused".to_string())
    })
    .expect_err("the commit failed, so this must too");

    assert!(
        error.contains("the commit refused"),
        "the original reason survives: {error}"
    );
    assert!(
        error.contains("restoration also failed"),
        "and the second failure is reported rather than hidden: {error}"
    );
    let backup = root.join("backup");
    assert!(
        error.contains(&backup.display().to_string()),
        "the error must name where the only surviving copy is: {error}"
    );
    assert_eq!(
        marker_of(&backup.join("component")),
        "old",
        "and the old bytes are actually there, not merely referenced"
    );
}

/// An interruption between the two renames is recoverable, using the journal the install wrote.
#[test]
fn an_interrupted_swap_is_recovered_from_the_retained_backup() {
    let root = scratch("interrupted");
    let live = root.join("live");
    interrupt_a_publication(&root, &live, "old");
    assert!(!live.exists(), "the destination is gone, as it is mid-swap");

    let recovered = recover_interrupted_publications(std::slice::from_ref(&root.0));
    assert_eq!(
        recovered.restored,
        vec![("component".to_string(), live.clone())]
    );
    assert!(recovered.retained.is_empty() && recovered.ambiguous.is_empty());
    assert_eq!(
        marker_of(&live),
        "old",
        "the old copy is back where it belongs"
    );
}

/// Recovery can be run twice.
///
/// A repair may itself be interrupted, so running it again must be safe rather than an error or a
/// second restoration over the first.
#[test]
fn recovery_is_repeatable() {
    let root = scratch("repeatable");
    let live = root.join("live");
    interrupt_a_publication(&root, &live, "old");

    let first = recover_interrupted_publications(std::slice::from_ref(&root.0));
    assert_eq!(first.restored.len(), 1);

    let second = recover_interrupted_publications(std::slice::from_ref(&root.0));
    assert_eq!(second, Default::default(), "a second run has nothing to do");
    assert_eq!(marker_of(&live), "old", "and has not disturbed the first");
}

/// Recovery will not put an older copy over a live one.
#[test]
fn recovery_refuses_to_overwrite_a_destination_that_is_present() {
    let root = scratch("present");
    let live = root.join("live");
    interrupt_a_publication(&root, &live, "older");
    // Something is installed there again before the repair runs.
    tree(&live, "new");

    let recovered = recover_interrupted_publications(std::slice::from_ref(&root.0));
    assert!(recovered.restored.is_empty());
    assert_eq!(recovered.retained.len(), 1, "reported, not acted on");
    assert_eq!(marker_of(&live), "new", "the live copy is untouched");
    assert_eq!(
        marker_of(&root.join("backup").join("component")),
        "older",
        "and the backup is kept rather than swept away by a repair that decided not to use it"
    );
}

/// **Where a backup belongs comes from the install that made it.**
///
/// Recovery first asked the running configuration, which is a different question: the resource
/// directories honour overrides a person can change, so a backup could have been restored to a
/// path its own install had never heard of. The journal is written before the first rename.
#[test]
fn recovery_uses_the_destination_the_install_recorded() {
    let root = scratch("journalled");
    let recorded = root.join("where-it-belonged");
    interrupt_a_publication(&root, &recorded, "old");
    let elsewhere = root.join("where-configuration-points-now");

    let recovered = recover_interrupted_publications(std::slice::from_ref(&root.0));
    assert_eq!(
        recovered.restored,
        vec![("component".to_string(), recorded.clone())],
        "the journal decided"
    );
    assert_eq!(marker_of(&recorded), "old");
    assert!(!elsewhere.exists(), "and nothing was put anywhere else");
}

/// A root with a backup and no journal is retained and reported, never guessed at.
#[test]
fn a_root_without_a_journal_is_reported_rather_than_guessed() {
    let root = scratch("no-journal");
    tree(&root.join("backup").join("component"), "old");

    match interrupted_publication(&root) {
        Interrupted::Unidentified { reason, .. } => assert!(
            reason.contains("journal"),
            "the reason must say why: {reason}"
        ),
        other => panic!("a root with no journal must not be actionable: {other:?}"),
    }

    let recovered = recover_interrupted_publications(std::slice::from_ref(&root.0));
    assert!(recovered.restored.is_empty());
    assert_eq!(recovered.unidentified.len(), 1);
    assert_eq!(
        marker_of(&root.join("backup").join("component")),
        "old",
        "and its bytes are left exactly where they are"
    );
}

/// A malformed journal is unidentified too, not an absent backup.
#[test]
fn an_unusable_journal_is_not_treated_as_no_backup() {
    let root = scratch("bad-journal");
    tree(&root.join("backup").join("component"), "old");
    std::fs::write(root.join("publication.json"), b"{ not json").unwrap();

    let recovered = recover_interrupted_publications(std::slice::from_ref(&root.0));
    assert_eq!(recovered.unidentified.len(), 1, "{recovered:?}");
    assert!(recovered.restored.is_empty());
}

/// A relative destination is refused.
///
/// Validated rather than merely parsed: a relative path resolves against whatever the process
/// happens to consider its working directory, which is not a place anybody chose.
#[test]
fn a_journal_with_a_relative_destination_is_refused() {
    let root = scratch("relative-journal");
    tree(&root.join("backup").join("component"), "old");
    std::fs::write(
        root.join("publication.json"),
        br#"{"members":[{"id":"component","destination":"somewhere/relative"}]}"#,
    )
    .unwrap();

    let recovered = recover_interrupted_publications(std::slice::from_ref(&root.0));
    assert_eq!(recovered.unidentified.len(), 1, "{recovered:?}");
    assert!(recovered.restored.is_empty());
}

/// **Two abandoned installs offering the same destination is not decided by directory order.**
///
/// The sweep used to restore as it walked, so whichever root the filesystem enumerated first won
/// and the other was merely "retained" afterwards. They are different installs from different
/// moments; choosing between them is a person's call.
#[test]
fn competing_backups_are_reported_rather_than_chosen_by_enumeration_order() {
    let shared = scratch("competing");
    let destination = shared.join("contested");

    let mut roots = Vec::new();
    for (name, marker) in [("first", "from-one"), ("second", "from-two")] {
        let root = shared.join(name);
        std::fs::create_dir_all(&root).unwrap();
        interrupt_a_publication(&root, &destination, marker);
        roots.push(root);
    }
    assert!(!destination.exists(), "both installs left it missing");

    let recovered = recover_interrupted_publications(&roots);
    assert!(
        recovered.restored.is_empty(),
        "neither may be chosen: {recovered:?}"
    );
    assert_eq!(recovered.ambiguous.len(), 1);
    assert_eq!(recovered.ambiguous[0].0, destination);
    assert_eq!(
        recovered.ambiguous[0].1.len(),
        2,
        "both claimants are named"
    );
    assert!(!destination.exists(), "and nothing was put there");
}

/// A publication that succeeds leaves nothing to recover.
///
/// The counterpart, so the others cannot pass by finding a backup that is always there.
#[test]
fn a_successful_publication_leaves_no_backup() {
    let root = scratch("clean");
    let staged = root.join("staged");
    let live = root.join("live");
    tree(&staged, "new");
    tree(&live, "old");

    publish_prepared(&root, &[("component".to_string(), staged, live.clone())]).unwrap();

    assert_eq!(marker_of(&live), "new");
    assert!(!root.join("backup").exists(), "nothing is retained");
    assert_eq!(
        recover_interrupted_publications(std::slice::from_ref(&root.0)),
        Default::default(),
        "and nothing to recover"
    );
}

/// A restoration that fails is **not** reported as a destination that already exists.
///
/// Both outcomes used to land in `retained`, which told a reader the destination was occupied when
/// it may well not have been — the first thing they would check, reported wrongly, with the actual
/// reason dropped. They are separate outcomes now, and the failure carries where the bytes still
/// are.
#[test]
fn a_failed_restoration_is_reported_as_a_failure_not_as_a_conflict() {
    let root = scratch("restore-fails");
    // A destination whose parent cannot be created, because a *file* sits where the directory
    // would have to go.
    let occupied_parent = root.join("blocked");
    std::fs::write(&occupied_parent, b"not a directory").unwrap();
    let destination = occupied_parent.join("component");

    let staged = root.join("staged");
    tree(&staged, "new");
    let members = [("component".to_string(), staged, destination.clone())];
    // The publication itself fails, which is fine: what matters is that the journal and a backup
    // are on disk for the recovery to work from.
    let _ = publish_with_commit(&root, &members, || Err("stopped".to_string()));
    let backup = root.join("backup");
    std::fs::create_dir_all(backup.join("component")).unwrap();
    std::fs::write(backup.join("component").join("marker.txt"), b"old").unwrap();

    let recovered = recover_interrupted_publications(std::slice::from_ref(&root.0));

    assert!(
        recovered.retained.is_empty(),
        "a failure is not a destination-already-exists: {recovered:?}"
    );
    assert_eq!(recovered.failed.len(), 1, "{recovered:?}");
    let failure = &recovered.failed[0];
    assert_eq!(failure.id, "component");
    assert_eq!(failure.destination, destination);
    assert!(
        !failure.reason.is_empty(),
        "a failure nobody can act on is not a report"
    );
    assert_eq!(
        marker_of(&failure.saved),
        "old",
        "and it names where the bytes still are, which is the part that matters"
    );
}
