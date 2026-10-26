//! Resource relocation must preserve readers and every original until activation succeeds.
//!
//! Both operations take a `Claimed` witness: the caller owns the whole tree **before** it works
//! out which directories to move, because those paths answer from the location a move changes. The
//! registry is process-global, so these run one at a time — or the second reports the first's move
//! rather than the thing it is about.
use sakatsu_core::dict::Dictionary;
use sakatsu_core::setup::claim::{claim, Claim, Claimed};
use sakatsu_core::setup::resources::{reclaim_relocated, relocate};
use std::io::Write;
use std::path::{Path, PathBuf};

static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialised() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Ownership of the whole tree, as every caller of these two takes it before resolving anything.
fn owned() -> Claimed {
    claim([Claim::Everything]).expect("nothing else is running")
}

/// A scratch root that is removed however the test ends.
///
/// **A guard rather than a call at the end.** Cleanup written as the last line of a test only runs
/// when the test passes — which is how 2,229 of these came to be sitting in the temporary directory,
/// and how the disk filled during this work.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn root(name: &str) -> Scratch {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "sakatsu-relocation-{name}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    Scratch(root.canonicalize().unwrap())
}

#[test]
fn old_readers_survive_the_move_and_old_files_wait_for_restart() {
    let _order = serialised();
    let root = root("reader");
    let archive = root.join("fixture.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    for (name, value) in [
        (
            "index.json",
            serde_json::json!({"title":"Relocation fixture","format":3}),
        ),
        (
            "term_bank_1.json",
            serde_json::json!([
                ["dog", "dog", "n", "", 0, ["old dog"], 1, ""],
                ["bird", "bird", "n", "", 0, ["old bird"], 2, ""]
            ]),
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(value.to_string().as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let source = root.join("old-dictionary");
    let held = Dictionary::import(&archive, &source).unwrap();
    assert_eq!(held.lookup("dog").unwrap().len(), 1);
    let target = root.join("new");
    std::fs::create_dir(&target).unwrap();
    let destination = target.join("dictionaries");
    let mut activated = false;
    relocate(
        &owned(),
        &target,
        &[(source.clone(), destination.clone())],
        true,
        |_| {
            activated = true;
            Ok(())
        },
    )
    .unwrap();
    assert!(activated);
    assert_eq!(
        held.lookup("bird").unwrap().len(),
        1,
        "unwarmed old files remain readable"
    );
    assert!(source.exists());
    assert!(
        !reclaim_relocated(&owned(), &target, std::slice::from_ref(&source)).is_empty(),
        "an override still using the old path protects it"
    );
    drop(held);
    assert!(reclaim_relocated(&owned(), &target, std::slice::from_ref(&destination)).is_empty());
    assert!(!source.exists());
    assert_eq!(
        Dictionary::open(&destination)
            .unwrap()
            .lookup("dog")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn failed_activation_restores_the_destination_and_keeps_the_source() {
    let _order = serialised();
    let root = root("activation");
    let source = root.join("source");
    let target = root.join("target");
    let destination = target.join("data");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(source.join("model"), "source").unwrap();
    std::fs::write(destination.join("model"), "previous target").unwrap();
    assert!(relocate(
        &owned(),
        &target,
        &[(source.clone(), destination.clone())],
        true,
        |_| Err("location write refused".into())
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(destination.join("model")).unwrap(),
        "previous target"
    );
    assert_eq!(
        std::fs::read_to_string(source.join("model")).unwrap(),
        "source"
    );
    assert!(!target.join("retired-resources.json").exists());
}

#[test]
fn cleanup_never_removes_files_changed_since_the_move() {
    let _order = serialised();
    let root = root("changed");
    let source = root.join("source");
    let target = root.join("target");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(source.join("model"), "source").unwrap();
    relocate(
        &owned(),
        &target,
        &[(source.clone(), target.join("data"))],
        true,
        |_| Ok(()),
    )
    .unwrap();
    std::fs::write(source.join("added-later"), "keep me").unwrap();
    assert!(!reclaim_relocated(&owned(), &target, &[]).is_empty());
    assert_eq!(
        std::fs::read_to_string(source.join("added-later")).unwrap(),
        "keep me"
    );
    assert!(target.join("retired-resources.json").exists());
}

/// A move with one small tree, for the cleanup cases below.
fn moved(name: &str) -> (Scratch, PathBuf, PathBuf) {
    let root = root(name);
    let source = root.join("source");
    let target = root.join("target");
    let destination = target.join("data");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(source.join("model"), "source").unwrap();
    relocate(
        &owned(),
        &target,
        &[(source.clone(), destination.clone())],
        true,
        |_| Ok(()),
    )
    .unwrap();
    (root, source, destination)
}

/// **A changed destination stops the cleanup too, not only a changed source.**
///
/// The consent the move recorded was for one pair of trees. If what is now live has been rewritten
/// since — a repair, an import, a hand edit — then removing the old copy is removing the only thing
/// that still matches what was agreed to.
#[test]
fn cleanup_keeps_the_old_copy_when_the_new_location_changed() {
    let _order = serialised();
    let (root, source, destination) = moved("changed-destination");
    std::fs::write(destination.join("model"), "rewritten since the move").unwrap();

    let notices = reclaim_relocated(&owned(), &root.join("target"), &[]);
    assert!(
        notices.iter().any(|notice| notice.contains("changed")),
        "{notices:?}"
    );
    assert!(source.exists(), "the old copy is the only agreed-on match");
    assert!(root.join("target").join("retired-resources.json").exists());
}

/// A file somebody still has open keeps its folder, with a notice naming where it is.
///
/// The real Windows shape: an inference session or an antivirus scanner holding one file with no
/// sharing. That stops the removal at whichever step reaches the file first — the verification read
/// or the delete itself — and either way the folder stays. What must not happen is the journal
/// being deleted anyway, because that is the record saying this folder is still owed a removal.
#[cfg(windows)]
#[test]
fn cleanup_keeps_a_folder_whose_file_is_locked_and_says_where_it_is() {
    use std::os::windows::fs::OpenOptionsExt;

    let _order = serialised();
    let (root, source, _destination) = moved("locked");
    // No sharing at all: the delete fails rather than being deferred.
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(source.join("model"))
        .unwrap();

    let notices = reclaim_relocated(&owned(), &root.join("target"), &[]);
    assert!(
        notices
            .iter()
            .any(|notice| notice.contains(&source.display().to_string())),
        "the notice has to name the folder, or it is not actionable: {notices:?}"
    );
    assert!(source.exists());
    assert!(
        root.join("target").join("retired-resources.json").exists(),
        "the journal survives so the next start can finish the job"
    );
    drop(locked);
}

/// A record nothing can read removes nothing and says so.
#[test]
fn an_unreadable_retirement_record_removes_nothing() {
    let _order = serialised();
    let (root, source, _destination) = moved("malformed");
    let journal = root.join("target").join("retired-resources.json");
    std::fs::write(&journal, b"{not json at all").unwrap();

    let notices = reclaim_relocated(&owned(), &root.join("target"), &[]);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("Cannot read"), "{notices:?}");
    assert!(source.exists());
    assert!(
        journal.exists(),
        "and the record is kept for a person to look at"
    );
}

/// A record from a newer version is kept rather than guessed at.
#[test]
fn a_retirement_record_from_another_version_is_kept() {
    let _order = serialised();
    let (root, source, _destination) = moved("future-record");
    let journal = root.join("target").join("retired-resources.json");
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    record["version"] = serde_json::json!(99);
    std::fs::write(&journal, serde_json::to_vec(&record).unwrap()).unwrap();

    let notices = reclaim_relocated(&owned(), &root.join("target"), &[]);
    assert!(
        notices.iter().any(|notice| notice.contains("Unknown")),
        "{notices:?}"
    );
    assert!(source.exists());
    assert!(journal.exists());
}

/// A cleanup that was interrupted after removing some folders finishes the rest.
///
/// The half-done state is not an error: a folder that is already gone is a folder that needs
/// nothing, and the run must still reach the ones behind it and then clear the journal.
#[test]
fn cleanup_finishes_after_a_previous_run_removed_some_of_it() {
    let _order = serialised();
    let root = root("partial");
    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    let first = root.join("first");
    let second = root.join("second");
    for (source, name) in [(&first, "one"), (&second, "two")] {
        std::fs::create_dir_all(source).unwrap();
        std::fs::write(source.join("model"), name).unwrap();
    }
    relocate(
        &owned(),
        &target,
        &[
            (first.clone(), target.join("first")),
            (second.clone(), target.join("second")),
        ],
        true,
        |_| Ok(()),
    )
    .unwrap();

    // What an interrupted previous cleanup leaves: one source gone, the journal still naming both.
    std::fs::remove_dir_all(&first).unwrap();

    let notices = reclaim_relocated(&owned(), &target, &[]);
    assert!(notices.is_empty(), "{notices:?}");
    assert!(
        !second.exists(),
        "the folder that was still owed is removed"
    );
    assert!(
        !target.join("retired-resources.json").exists(),
        "and the journal goes once nothing is left owing"
    );
}

/// The move refuses to start while the previous one still has copies awaiting cleanup.
///
/// Two retirement records cannot coexist — there is one journal — so this is the guard that keeps
/// the second move from overwriting the first move's consent.
#[test]
fn a_second_move_waits_for_the_previous_cleanup() {
    let _order = serialised();
    let (root, _source, _destination) = moved("second-move");
    let target = root.join("target");
    let again = root.join("again");
    std::fs::create_dir_all(&again).unwrap();
    std::fs::write(again.join("model"), "another").unwrap();

    let refused = relocate(
        &owned(),
        &target,
        &[(again.clone(), target.join("again"))],
        true,
        |_| panic!("a second move must not reach activation"),
    )
    .expect_err("the previous move is still owed a cleanup");
    assert!(refused.contains("Restart"), "{refused}");
}

/// **A failed move leaves no copies behind, and says so by leaving nothing to find.**
///
/// The staging directory was being kept on every failure, which read as untidiness and was worth
/// establishing rather than assuming: what it holds after a rolled-back publication is `tree-N`,
/// which are verified copies of sources that are still exactly where they were. Preparation, not
/// backups. It goes.
#[test]
fn a_failed_move_discards_the_copies_it_had_prepared() {
    let _order = serialised();
    let root = root("failed-publication");
    let source = root.join("source");
    let target = root.join("target");
    let destination = target.join("data");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(source.join("model"), "source").unwrap();
    std::fs::write(destination.join("model"), "previous target").unwrap();

    let error = relocate(
        &owned(),
        &target,
        &[(source.clone(), destination.clone())],
        true,
        |_| Err("location write refused".into()),
    )
    .expect_err("activation refused");
    assert!(error.contains("location write refused"), "{error}");
    assert!(
        !error.contains("kept at"),
        "nothing was retained, so nothing should be offered to go and look at: {error}"
    );

    let staging: Vec<_> = std::fs::read_dir(&target)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".relocation-"))
        .collect();
    assert!(
        staging.is_empty(),
        "the prepared copies are disposable and were not disposed of: {staging:?}"
    );
    assert_eq!(
        std::fs::read_to_string(source.join("model")).unwrap(),
        "source",
        "the source is untouched, which is what makes the copies disposable"
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("model")).unwrap(),
        "previous target"
    );
    assert!(!target.join("retired-resources.json").exists());
}

/// A file held at the **destination** stops the move before anything is exchanged.
///
/// The locked-file case tested elsewhere holds a file in the old copy, during cleanup. This is the
/// other side: something at the live location that Windows will not let go of — an inference
/// session, an antivirus scanner — while the move is trying to put the new tree there. Nothing may
/// be exchanged, and the reason has to be one somebody can act on.
#[cfg(windows)]
#[test]
fn a_locked_file_at_the_destination_refuses_the_move_and_changes_nothing() {
    use std::os::windows::fs::OpenOptionsExt;

    let _order = serialised();
    let root = root("locked-target");
    let source = root.join("source");
    let target = root.join("target");
    let destination = target.join("data");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(source.join("model"), "source").unwrap();
    std::fs::write(destination.join("model"), "in use").unwrap();

    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(destination.join("model"))
        .unwrap();

    let error = relocate(
        &owned(),
        &target,
        &[(source.clone(), destination.clone())],
        true,
        |_| panic!("activation must not be reached: nothing could be moved aside"),
    )
    .expect_err("the destination cannot be moved aside while it is held");
    assert!(
        !error.is_empty(),
        "a refusal with no sentence in it is not a report"
    );

    assert_eq!(
        std::fs::read_to_string(source.join("model")).unwrap(),
        "source",
        "the source is exactly where it was"
    );
    assert!(
        !target.join("retired-resources.json").exists(),
        "a move that never happened must not leave consent to delete anything"
    );

    // Released before reading it back: this handle is the lock, and it refuses this process too.
    drop(locked);
    assert_eq!(
        std::fs::read_to_string(destination.join("model")).unwrap(),
        "in use",
        "and what was already live is still there, unexchanged"
    );
}
