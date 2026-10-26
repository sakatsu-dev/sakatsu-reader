//! Two operations that both write the resource tree must not run at once — and neither may work
//! from a location it looked up before it owned anything.
//!
//! # What was missing, and how it was found
//!
//! `reserve_component_installs` gave one publisher per **component**, across both setup screens.
//! Nothing gave one publisher per **resource tree**: relocation and the gloss backfill took no
//! reservation at all. That was reproduced here before `core::setup::claim` existed — a move ran to
//! activation while an install held the dictionaries, and startup cleanup deleted an old copy with
//! an install in progress.
//!
//! **Claiming turned out not to be enough on its own.** Every one of these operations begins by
//! resolving a resource directory, and those answer from a location a move changes. Claim second
//! and this is reachable:
//!
//! ```text
//! resolve the old location → a move completes → take the claim → write to the abandoned location
//! ```
//!
//! No two operations overlap while holding claims, and the write still lands where nothing will
//! look again. So the writing operations take a `Claimed` **witness** instead of claiming for
//! themselves, and a caller cannot hold one without having claimed before it resolved anything.
//!
//! # What is here, and what is not
//!
//! This file is about the **mechanism**: what the witness requires, and what it costs when a write
//! reaches the tree without one. Whether the production commands actually take ownership — and take
//! it early enough — is a question about callers, and it is asked where the callers are, in
//! `src-tauri/tests/writer_ownership.rs`. Testing `claim()` here would establish nothing about them.
//!
//! The barrier is `relocate`'s own `activate` closure. It runs inside the publication, after every
//! tree is copied and verified and after the swap, which is exactly the window a second operation
//! does its damage in — and it is a closure the test supplies, so the interleaving is *chosen*
//! rather than waited for. No sleeps.

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::dict::Dictionary;
use sakatsu_core::setup::claim::{claim, Claim, Claimed};
use sakatsu_core::setup::resources::{reclaim_relocated, relocate};

/// The claim registry is process-global, which is what makes it a policy rather than a lock
/// somebody remembered to take. Two of these holding claims at once would report each other's
/// reservations, so they run one at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialised() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Ownership of the whole tree, as every caller takes it before resolving anything.
fn owned() -> Claimed {
    claim([Claim::Everything]).expect("nothing else is running")
}

/// A scratch root that is removed however the test ends.
///
/// **A guard rather than a call at the end.** Cleanup written as the last line of a test only runs
/// when the test passes — which is how 2,229 of these came to be sitting in the temporary
/// directory, and how the disk filled during this work.
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
        "sakatsu-coordination-{name}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    Scratch(root.canonicalize().unwrap())
}

/// A dictionary with two words, imported into `container`.
fn a_dictionary(root: &Path, container: &Path) -> Dictionary {
    let archive = root.join("fixture.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    for (name, value) in [
        (
            "index.json",
            serde_json::json!({"title":"Coordination fixture","format":3}),
        ),
        (
            "term_bank_1.json",
            serde_json::json!([
                ["dog", "dog", "n", "", 0, ["a dog"], 1, ""],
                ["bird", "bird", "n", "", 0, ["a bird"], 2, ""]
            ]),
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(value.to_string().as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    Dictionary::import(&archive, container).unwrap()
}

/// Remove the gloss index a fresh import builds, so a backfill has something to do.
fn without_a_gloss_index(container: &Path) {
    let generation = std::fs::read_to_string(container.join("current")).unwrap();
    let gloss = container.join(generation.trim()).join("gloss");
    if gloss.is_dir() {
        std::fs::remove_dir_all(gloss).unwrap();
    }
}

/// **What it costs when a publication lands inside a move.**
///
/// This drives the primitive directly, past the claim the orchestrators take, because that is the
/// only way to show what the claim is for. The finding is not "bytes are lost" — they are not — it
/// is that the two locations disagree and **nothing says so**: the new location has the
/// pre-backfill generation, the work is in a directory that is now only kept because its
/// fingerprint no longer matches, and English search comes back unavailable with no explanation.
#[test]
fn a_publication_that_lands_mid_move_leaves_the_new_location_stale_and_silent() {
    let _order = serialised();
    let root = root("stale");
    let source = root.join("dictionaries");
    let target = root.join("new");
    std::fs::create_dir_all(&target).unwrap();
    let container = source.join("fixture");
    let held = a_dictionary(&root, &container);
    without_a_gloss_index(&container);
    assert!(
        Dictionary::open(&container)
            .unwrap()
            .needs_secondary_index(),
        "the fixture starts without the index a backfill would build"
    );

    let destination = target.join("dictionaries");
    relocate(
        &owned(),
        &target,
        &[(source.clone(), destination.clone())],
        true,
        |_| {
            // The backfill, at the worst possible instant: after the copy was verified and swapped.
            held.index_secondary().unwrap();
            Ok(())
        },
    )
    .unwrap();

    assert!(
        Dictionary::open(&destination.join("fixture"))
            .unwrap()
            .needs_secondary_index(),
        "the new location carries the generation from before the backfill — this is the loss"
    );
    assert!(
        !Dictionary::open(&container)
            .unwrap()
            .needs_secondary_index(),
        "while the work itself sits in the old location"
    );

    // Nothing was destroyed, and that is the part worth being precise about: the retirement record
    // no longer matches what is on disk, so cleanup keeps the old copy and says so in a notice.
    drop(held);
    let notices = reclaim_relocated(&owned(), &target, &[]);
    assert!(
        notices.iter().any(|notice| notice.contains("changed")),
        "the old copy is kept because it changed since the move: {notices:?}"
    );
    assert!(
        source.exists(),
        "and it is still there to be recovered from"
    );
}

/// **And what it costs to look the location up before owning it.**
///
/// The interleaving the witness closes, written out as a writer that does it the wrong way round:
/// it resolves where the dictionaries are, a move completes, and only then does it write. It never
/// overlaps the move — it takes its claim afterwards, cleanly, and is refused by nothing — and its
/// work still lands in a directory the app will not look at again.
///
/// No seam inside the copier is needed for this: the interleaving is between two of the writer's
/// own steps, and the test performs them in that order.
#[test]
fn a_writer_that_resolves_before_it_owns_writes_to_the_abandoned_location() {
    let _order = serialised();
    let root = root("resolve-first");
    let source = root.join("dictionaries");
    let target = root.join("new");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(source.join("placeholder"), "resources").unwrap();

    // Where the app looks *now*. A writer that resolves at this point holds a path, not a promise.
    let live = || -> PathBuf {
        std::fs::read_to_string(root.join("location.txt"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| source.clone())
    };
    let resolved_too_early = live();

    // The move completes, activation and all.
    let destination = target.join("dictionaries");
    relocate(
        &owned(),
        &target,
        &[(source.clone(), destination.clone())],
        false,
        |_| {
            std::fs::write(root.join("location.txt"), destination.display().to_string())
                .map_err(|error| error.to_string())
        },
    )
    .unwrap();

    // Now the writer claims — successfully, because nothing else is running — and writes.
    let late = claim([Claim::Dictionaries]).expect("the move has finished; nothing is running");
    std::fs::write(resolved_too_early.join("imported"), "a dictionary").unwrap();
    drop(late);

    assert!(
        !destination.join("imported").exists(),
        "the write did not reach the location the app now reads"
    );
    assert!(
        resolved_too_early.join("imported").exists(),
        "it landed in the abandoned one instead — with no overlap, no refusal, and no error"
    );
    assert_ne!(
        live(),
        resolved_too_early,
        "and the two are genuinely different places, or this test proves nothing"
    );
}

/// The witness is what makes that unwriteable: neither operation runs without one.
#[test]
#[should_panic(expected = "Claim before resolving any path")]
fn a_move_without_ownership_refuses_to_run() {
    let _order = serialised();
    let root = root("no-ownership");
    let target = root.join("target");
    std::fs::create_dir_all(&target).unwrap();
    // A component claim is a real claim and still not this one.
    let wrong = claim([Claim::Component("manga-ocr")]).expect("nothing else is running");
    let _ = relocate(&wrong, &target, &[], false, |_| Ok(()));
}

/// The same for the backfill, which is the other operation that publishes without downloading.
#[test]
#[should_panic(expected = "Claim before resolving any path")]
fn a_backfill_without_ownership_refuses_to_run() {
    let _order = serialised();
    let root = root("no-ownership-backfill");
    let wrong = claim([Claim::Component("manga-ocr")]).expect("nothing else is running");
    let _ = sakatsu_core::setup::backfill::rebuild_gloss_indexes(&root, &wrong);
}

/// A claim that covers the dictionaries is enough for the backfill, and a move's is too.
#[test]
fn the_backfill_accepts_either_claim_that_covers_the_dictionaries() {
    let _order = serialised();
    let root = root("covers");
    let container = root.join("fixture");
    let held = a_dictionary(&root, &container);
    without_a_gloss_index(&container);
    drop(held);

    let dictionaries = claim([Claim::Dictionaries]).expect("nothing else is running");
    let built = sakatsu_core::setup::backfill::rebuild_gloss_indexes(&root, &dictionaries);
    assert_eq!(built.built.len(), 1, "{built:?}");
    assert!(built.failed.is_none(), "{built:?}");
    drop(dictionaries);

    // Nothing left to build, and a move's ownership is accepted for the same work.
    let everything = owned();
    let again = sakatsu_core::setup::backfill::rebuild_gloss_indexes(&root, &everything);
    assert!(again.built.is_empty(), "{again:?}");
}
