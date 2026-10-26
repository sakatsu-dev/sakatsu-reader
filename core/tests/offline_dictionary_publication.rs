//! An offline package must not take bytes a live reader is still using.
//!
//! # The defect, and why it was the bad kind
//!
//! `setup_manager::import_offline` copied the live dictionary container, imported into the copy,
//! and swapped the copy in as the new live root. Generation **leases are held against the original
//! paths**, so pruning inside the copy saw no lease on its copied old generation and removed it;
//! the swap then took away the bytes a held reader was still resolving to.
//!
//! The symptom was the worst available, because one of its two faces is silent. Where the swap
//! leaves a missing *directory*, a shard is cached as an empty map and the reader reports "no such
//! word" — indistinguishable from a dictionary that genuinely lacks it. Where it leaves a missing
//! *file*, the lookup errors instead. The original reproduction saw the silent face for an unwarmed
//! word; `the_whole_root_swap_is_still_the_mechanism_that_loses_a_held_reader` sees the loud one.
//! Both are the same destruction, and only the first is the reason it could have shipped unnoticed.
//!
//! # What these tests drive
//!
//! The **shared orchestration the shell uses**, not a sketch of it: `install::prepare_dictionaries`
//! prepares generations in the live container and `publication::publish_offline` activates them
//! inside the same rollback boundary as the directory swaps. `setup_manager::import_offline` calls
//! exactly these. What stays in the shell is the package's own reading, hashing and manifest
//! checking, which have nothing to do with publication.
//!
//! The control — an ordinary `Dictionary::import` into the stable root — is kept beside them. It
//! asserts the same required behaviour through the path that always had it, so a failure here can
//! be read as "the offline path is wrong" rather than "the fixture is".

use sakatsu_core::dict::Dictionary;
use sakatsu_core::setup::install::InstallHooks;
use sakatsu_core::setup::publication::publish_offline;
use sakatsu_core::setup::{catalog::After, Artifact, Component, Tier};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::{io::Write, path::Path, path::PathBuf};

#[path = "common/scratch.rs"]
mod scratch_guard;

fn archive(path: &Path, replacement: bool) {
    let rows = if replacement {
        serde_json::json!([[
            "cat",
            "cat",
            "n",
            "",
            0,
            ["new cat with longer contents"],
            3,
            ""
        ]])
    } else {
        serde_json::json!([
            ["dog", "dog", "n", "", 0, ["old dog"], 1, ""],
            ["bird", "bird", "n", "", 0, ["old bird"], 2, ""]
        ])
    };
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, value) in [
        (
            "index.json",
            serde_json::json!({"title":"Offline lease fixture", "format":3}),
        ),
        ("term_bank_1.json", rows),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(value.to_string().as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

fn scratch(name: &str) -> scratch_guard::Scratch {
    scratch_guard::Scratch::new("offline", name)
}

struct Quiet;
impl InstallHooks for Quiet {
    fn detail(&mut self, _id: &str, _detail: &str) {}
    fn phase(&mut self, _phase: sakatsu_core::setup::install::InstallPhase) {}
    fn verify_model(&mut self, _id: &str, _work: &Path) -> Result<(), String> {
        Ok(())
    }
    fn install_runtime(&mut self, _into: &Path) -> Result<String, String> {
        Ok(String::new())
    }
}

/// A component naming one archive, so `prepare_dictionaries` walks the same list it would for a
/// real one. Leaked deliberately: the production type is `&'static`, and a test fixture that lives
/// for the process is the honest way to satisfy that without weakening the signature.
fn component(file: &'static str) -> &'static Component {
    Box::leak(Box::new(Component {
        id: "dictionaries",
        label: "Dictionaries",
        about: "",
        bytes: 0,
        licence: "CC BY-SA 4.0",
        home: "",
        tier: Tier::Essential,
        script: None,
        after: After::ImportDictionary,
        artifacts: Box::leak(Box::new([Artifact {
            url: "",
            file,
            min_bytes: 0,
            expected_bytes: None,
            sha256: None,
        }])),
    }))
}

/// What a held reader must still be able to do after a replacement is published.
///
/// Both a warm and an **unwarmed** lookup, and the second is the one that mattered: the defect was
/// invisible to the first, because a shard already in memory keeps answering from bytes nobody can
/// take away.
fn assert_old_reader_intact(held: &Dictionary, warm: &serde_json::Value) {
    let bird = held
        .lookup("bird")
        .expect("a held reader must still read its own generation");
    assert_eq!(
        bird.len(),
        1,
        "the unwarmed lookup is the one the whole-root swap broke"
    );
    assert!(serde_json::to_string(&bird).unwrap().contains("old bird"));
    assert_eq!(
        serde_json::to_value(held.lookup("dog").unwrap()).unwrap(),
        *warm,
        "and the warm one is unchanged"
    );
}

/// The control: the path that always had this property.
#[test]
fn publishing_into_the_stable_dictionary_root_preserves_a_held_reader() {
    let root = scratch("control");
    let first = root.join("first.zip");
    let second = root.join("second.zip");
    archive(&first, false);
    archive(&second, true);
    let live = root.join("live");

    let held = Dictionary::import(&first, &live).unwrap();
    let warm = serde_json::to_value(held.lookup("dog").unwrap()).unwrap();
    assert_eq!(
        warm.as_array().unwrap().len(),
        1,
        "the control query exists"
    );

    drop(Dictionary::import(&second, &live).unwrap());

    let fresh = Dictionary::open(&live).unwrap();
    assert_eq!(fresh.lookup("cat").unwrap().len(), 1);
    assert!(fresh.lookup("dog").unwrap().is_empty());
    assert_old_reader_intact(&held, &warm);
}

/// The regression: the same requirement, through the offline orchestration.
#[test]
fn offline_publication_preserves_a_held_reader() {
    let root = scratch("offline");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    archive(&work.join("second.zip"), true);
    let dictionaries = root.join("live");

    // A reader opened before the package arrives, exactly as the app holds one while a setup runs.
    let held = {
        let mut prepared = sakatsu_core::setup::install::prepare_dictionaries(
            &mut Quiet,
            component("first.zip"),
            &dictionaries,
            &work,
            &Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
        .0;
        for one in prepared.iter_mut() {
            one.activate().unwrap();
        }
        for one in prepared {
            one.settle().unwrap();
        }
        Dictionary::open(&dictionaries.join("first")).unwrap()
    };
    let warm = serde_json::to_value(held.lookup("dog").unwrap()).unwrap();
    assert_eq!(warm.as_array().unwrap().len(), 1);

    // The package. Prepared into the live container, then activated through the shared publisher.
    let (prepared, described) = sakatsu_core::setup::install::prepare_dictionaries(
        &mut Quiet,
        component("second.zip"),
        &dictionaries,
        &work,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(described.contains("entries"), "the summary is unchanged");

    // Nothing may point at the new generation until it is activated.
    assert!(
        Dictionary::open(&dictionaries.join("second"))
            .map(|opened| opened.lookup("cat").unwrap().is_empty())
            .unwrap_or(true),
        "a prepared generation must not be readable before activation"
    );

    publish_offline(&root, &[], prepared).unwrap();

    let fresh = Dictionary::open(&dictionaries.join("second")).unwrap();
    assert_eq!(
        fresh.lookup("cat").unwrap().len(),
        1,
        "the new reader sees the new bytes"
    );
    assert_old_reader_intact(&held, &warm);
}

/// Replacing the **same** container, which is the ordinary upgrade and the exact shape that broke.
#[test]
fn offline_publication_over_an_existing_container_preserves_a_held_reader() {
    let root = scratch("offline-same");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");

    let mut prepared = sakatsu_core::setup::install::prepare_dictionaries(
        &mut Quiet,
        component("first.zip"),
        &dictionaries,
        &work,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
    .0;
    for one in prepared.iter_mut() {
        one.activate().unwrap();
    }
    for one in prepared {
        one.settle().unwrap();
    }
    let held = Dictionary::open(&dictionaries.join("first")).unwrap();
    let warm = serde_json::to_value(held.lookup("dog").unwrap()).unwrap();

    // The replacement lands in the same container, under the same name.
    archive(&work.join("first.zip"), true);
    let (prepared, _) = sakatsu_core::setup::install::prepare_dictionaries(
        &mut Quiet,
        component("first.zip"),
        &dictionaries,
        &work,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    publish_offline(&root, &[], prepared).unwrap();

    {
        let fresh = Dictionary::open(&dictionaries.join("first")).unwrap();
        assert_eq!(fresh.lookup("cat").unwrap().len(), 1);
        assert!(
            fresh.lookup("dog").unwrap().is_empty(),
            "the new generation replaced the old"
        );
        assert_old_reader_intact(&held, &warm);
        // Scoped, because this reader leases the generation it opened on. Holding it past the
        // next publish would keep that generation alive — which is the correct behaviour, and
        // would make the reclamation assertion below measure the wrong thing.
    }

    // And once the old reader is gone, a later publish reclaims what it was holding.
    let generations = || {
        std::fs::read_dir(dictionaries.join("first"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("g-"))
            .count()
    };
    assert!(
        generations() >= 2,
        "the generation the held reader is using is still on disk"
    );

    // Only now, with no reader on either, may a publish reclaim them.
    drop(held);
    archive(&work.join("first.zip"), false);
    let (prepared, _) = sakatsu_core::setup::install::prepare_dictionaries(
        &mut Quiet,
        component("first.zip"),
        &dictionaries,
        &work,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    publish_offline(&root, &[], prepared).unwrap();
    assert_eq!(
        generations(),
        1,
        "with no reader holding it, the superseded generation is reclaimed"
    );
}

/// Why the refusal above exists: swapping a container really does destroy a held reader.
///
/// **A diagnostic, and only that.** It says the mechanism is unsafe; it cannot say the shell picks
/// the safe one — `the_generic_publisher_refuses_a_dictionary_container` is what makes the unsafe
/// one unreachable, and this explains what that refusal is protecting.
///
/// The swap is done with raw renames rather than through `publish_prepared`, which now declines a
/// dictionary container outright. That is the point: the hazard is a property of replacing the
/// directory, whoever does it, so the demonstration must not depend on the publisher that has
/// since been taught to refuse.
///
/// If this ever stops failing, the reasoning behind `PreparedGeneration` needs re-reading rather
/// than trusting — that is a deliberate re-examination, not a green run.
#[test]
fn replacing_a_container_by_hand_still_loses_a_held_reader() {
    let root = scratch("unsafe-control");
    let first = root.join("first.zip");
    let second = root.join("second.zip");
    archive(&first, false);
    archive(&second, true);
    let live = root.join("live");

    let held = Dictionary::import(&first, &live).unwrap();
    assert_eq!(
        held.lookup("dog").unwrap().len(),
        1,
        "the reader answers before the swap, so what follows is a loss and not an empty fixture"
    );

    // Exactly what `import_offline` used to do, spelled out.
    let prepared = root.join("prepared");
    sakatsu_core::setup::resources::copy_tree(&live, &prepared).unwrap();
    drop(Dictionary::import(&second, &prepared).unwrap());
    let aside = root.join("aside");
    std::fs::rename(&live, &aside).unwrap();
    std::fs::rename(&prepared, &live).unwrap();

    // The reader has lost its own generation, and **either face counts**. Depending on whether the
    // swap left a missing file or a missing directory, a lookup surfaces as an I/O error or as a
    // cached-empty shard reporting "no such word" — the original reproduction of this defect saw
    // the silent empty for an unwarmed word, and this fixture sees the error. Both are the same
    // destruction, and pinning one spelling would make the guard fragile about the wrong thing.
    //
    // The silent one is why it went unnoticed: nothing in the app distinguishes "this dictionary
    // does not have that word" from "the bytes were deleted out from under you".
    let lost = |query: &str| match held.lookup(query) {
        Ok(found) => found.is_empty(),
        Err(_) => true,
    };
    assert!(
        lost("bird") && lost("dog"),
        "if a hand-rolled container swap now preserves a held reader, find out why before          reaching for one again"
    );
}

/// The identity an action must carry, read from the record it is about.
///
/// Tests name the outstanding transaction rather than hard-coding one, because the point of the
/// identity check is that it matches *what the caller was shown* — and a test that invented an
/// identity would be checking its own arithmetic. `a_stale_action_is_refused_and_leaves_the_record_alone`
/// in `src-tauri/tests/recovery_ipc.rs` is where a deliberately wrong one is exercised.
fn identity_of(container: &Path) -> sakatsu_core::dict::Identity {
    let record = sakatsu_core::dict::RecoveryRecord::read(container)
        .expect("readable")
        .expect("something is outstanding");
    sakatsu_core::dict::Identity {
        transaction: record.transaction,
        published: record.published,
    }
}

/// Prepare one dictionary in `dictionaries/<name>` from the archive already staged in `work`.
fn prepare_one(
    dictionaries: &Path,
    work: &Path,
    file: &'static str,
) -> sakatsu_core::dict::PreparedGeneration {
    let (mut prepared, _) = sakatsu_core::setup::install::prepare_dictionaries(
        &mut Quiet,
        component(file),
        dictionaries,
        work,
        &Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(prepared.len(), 1, "the fixture component names one archive");
    prepared.pop().unwrap()
}

/// Install one dictionary and settle it, leaving a container with a live generation.
fn install_one(dictionaries: &Path, work: &Path, file: &'static str) {
    let mut prepared = prepare_one(dictionaries, work, file);
    prepared.activate().unwrap();
    prepared.settle().unwrap();
}

/// A second activation that fails must undo the first one.
///
/// **The earlier version of this test proved nothing.** It failed a directory-swap member, and
/// `publish_with_commit` renames every swap *before* it calls the commit closure — so dictionary
/// activation never ran, and the unchanged pointer it asserted was unchanged because nothing had
/// touched it. Rollback was never reached.
///
/// So the failure is injected where activation actually happens: two dictionaries, the second of
/// which cannot activate because its container's `current` path is occupied by a directory. The
/// first has already switched by then, and what is asserted is that it switched back.
#[test]
fn a_failed_activation_rolls_back_the_dictionary_that_already_switched() {
    let root = scratch("rollback");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    archive(&work.join("second.zip"), false);
    let dictionaries = root.join("live");

    install_one(&dictionaries, &work, "first.zip");
    install_one(&dictionaries, &work, "second.zip");
    let before = std::fs::read_to_string(dictionaries.join("first").join("current")).unwrap();
    let untouched = std::fs::read_to_string(dictionaries.join("second").join("current")).unwrap();

    // Replacements for both containers.
    archive(&work.join("first.zip"), true);
    archive(&work.join("second.zip"), true);
    let first = prepare_one(&dictionaries, &work, "first.zip");
    let second = prepare_one(&dictionaries, &work, "second.zip");

    // The second container cannot take a new pointer: a directory sits where the file goes.
    let blocked = dictionaries.join("second").join("current");
    std::fs::remove_file(&blocked).unwrap();
    std::fs::create_dir(&blocked).unwrap();

    let error = publish_offline(&root, &[], vec![first, second])
        .expect_err("the second activation cannot write its pointer");
    assert!(
        error.contains("Could not activate a dictionary"),
        "the message must say what failed: {error}"
    );

    // **The first one activated and had to be put back.** This is the assertion the previous
    // version could not make, because activation never happened.
    assert_eq!(
        std::fs::read_to_string(dictionaries.join("first").join("current")).unwrap(),
        before,
        "the dictionary that already switched must be back on its old generation"
    );
    let rolled = Dictionary::open(&dictionaries.join("first")).unwrap();
    assert_eq!(
        rolled.lookup("dog").unwrap().len(),
        1,
        "and it answers with the old contents"
    );
    assert!(rolled.lookup("cat").unwrap().is_empty());

    // The blocked one never switched, so there was nothing to undo for it.
    std::fs::remove_dir(&blocked).unwrap();
    std::fs::write(&blocked, untouched.as_bytes()).unwrap();
    assert_eq!(
        Dictionary::open(&dictionaries.join("second"))
            .unwrap()
            .lookup("dog")
            .unwrap()
            .len(),
        1
    );
}

/// A roll-back that fails keeps what it needs to try again.
///
/// **The state used to be discarded before the fallible work.** `roll_back` began with
/// `self.previous.take()`, so a restoration that failed left nothing behind — and the next call
/// returned `Ok(())` having done nothing at all, reporting a roll-back that never happened. That
/// is the worst shape available: a caller that retries is told it succeeded.
#[test]
fn a_failed_roll_back_can_be_retried_and_does_not_report_success() {
    let root = scratch("rollback-retry");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let before = std::fs::read_to_string(container.join("current")).unwrap();

    archive(&work.join("first.zip"), true);
    let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
    prepared.activate().unwrap();
    let after = std::fs::read_to_string(container.join("current")).unwrap();
    assert_ne!(after, before, "the activation moved the pointer");

    // Block the restoration: a directory where the pointer file must be renamed into place.
    let pointer = container.join("current");
    std::fs::remove_file(&pointer).unwrap();
    std::fs::create_dir(&pointer).unwrap();

    prepared
        .roll_back()
        .expect_err("a restoration that cannot write must not report success");

    // Unblock, and retry. The recovery state has to have survived the failure.
    std::fs::remove_dir(&pointer).unwrap();
    std::fs::write(&pointer, after.as_bytes()).unwrap();
    prepared.roll_back().expect("the retry must restore");

    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        before,
        "the retry put the old generation back, which the discarded state made impossible"
    );
    assert_eq!(
        Dictionary::open(&container)
            .unwrap()
            .lookup("dog")
            .unwrap()
            .len(),
        1
    );
}

/// The generic publisher refuses to own a dictionary container at all.
///
/// **This is the part a diagnostic control cannot do.** Asserting that the whole-root swap is still
/// broken says the mechanism is unsafe; it does not stop the next caller reaching for it. Refusing
/// the member does, and it refuses on what is on disk rather than on the id, so a container reached
/// under any name is caught.
#[test]
fn the_generic_publisher_refuses_a_dictionary_container() {
    let root = scratch("refusal");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let staged = root.join("staged");
    std::fs::create_dir_all(&staged).unwrap();
    let error = sakatsu_core::setup::publication::publish_prepared(
        &root,
        &[("dictionary".into(), staged, dictionaries.join("first"))],
    )
    .expect_err("a dictionary container is not a directory-swap member");
    assert!(
        error.contains("immutable generations") && error.contains("Dictionary::prepare"),
        "the refusal must say what to do instead: {error}"
    );

    // And the container is untouched by the refusal.
    assert_eq!(
        Dictionary::open(&dictionaries.join("first"))
            .unwrap()
            .lookup("dog")
            .unwrap()
            .len(),
        1
    );
}

/// Another publisher's work is never silently undone.
///
/// **The earlier version of this test asserted the wrong outcome.** It published a third generation
/// and then expected the older transaction's roll-back to replace it with the original — which
/// establishes that the bytes were retained while accepting a **lost update**, and an invisible one:
/// the container still names a valid generation and still answers, so nothing looks wrong.
///
/// Three distinct generations, so "went back to the original" and "left the newcomer alone" are
/// different answers rather than the same one. Retention is still asserted — the roll-back target
/// must survive — but the roll-back itself must decline.
#[test]
fn a_roll_back_refuses_when_another_publisher_has_moved_on() {
    let root = scratch("rollback-conflict");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let original = std::fs::read_to_string(container.join("current")).unwrap();

    // Generation two: this transaction's.
    archive(&work.join("first.zip"), true);
    let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
    prepared.activate().unwrap();
    let ours = std::fs::read_to_string(container.join("current")).unwrap();
    assert_ne!(ours, original);

    // Generation three: somebody else's, landing after ours.
    let third = work.join("third.zip");
    archive(&third, false);
    drop(Dictionary::import(&third, &container).unwrap());
    let theirs = std::fs::read_to_string(container.join("current")).unwrap();
    assert_ne!(theirs, ours, "three distinct generations");
    assert_ne!(theirs, original);

    // The roll-back must decline rather than restore over them.
    prepared
        .roll_back()
        .expect_err("undoing this transaction would discard a newer publication");
    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        theirs,
        "the newer publication is still live"
    );

    // And the target is still there, because declining is not the same as giving up: a person can
    // still choose to put it back.
    assert!(
        container.join(&original).is_dir(),
        "the generation the roll-back was for must survive the refusal"
    );
    assert_eq!(
        Dictionary::open(&container)
            .unwrap()
            .lookup("dog")
            .unwrap()
            .len(),
        1,
        "and the live dictionary answers"
    );
}

/// The guard refuses the destination the defect actually used.
///
/// **This is the shape the first version of the guard missed.** `destination("dictionaries")` is the
/// parent — `dictionaries/`, holding `jitendex/` and `jmnedict/` — and the parent has no `current`
/// and no `g-` directory of its own. A guard that looked only at the destination's own markers
/// would have let the original defect straight through, which is exactly what it did.
#[test]
fn the_guard_refuses_the_parent_that_holds_the_dictionaries() {
    let root = scratch("guard-parent");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    archive(&work.join("second.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");
    install_one(&dictionaries, &work, "second.zip");

    // The parent itself carries no generation markers.
    assert!(!dictionaries.join("current").exists());

    let staged = root.join("staged");
    std::fs::create_dir_all(&staged).unwrap();
    let error = sakatsu_core::setup::publication::publish_prepared(
        &root,
        &[("dictionaries".into(), staged, dictionaries.clone())],
    )
    .expect_err("replacing the parent replaces every dictionary under it");
    assert!(error.contains("immutable generations"), "{error}");

    for name in ["first", "second"] {
        assert_eq!(
            Dictionary::open(&dictionaries.join(name))
                .unwrap()
                .lookup("dog")
                .unwrap()
                .len(),
            1,
            "{name} is untouched"
        );
    }
}

/// And a legacy dictionary, which has no generation markers at all.
///
/// A directory imported before generations existed *is* its own generation — `meta.json` and
/// `terms.ndjson`, no `current`, no `g-`. Readers lease it by path exactly the same way, so
/// replacing it destroys bytes exactly the same way. It is also the install least likely to have
/// been re-imported recently, which makes it the more likely one to still be sitting there.
#[test]
fn the_guard_refuses_a_legacy_dictionary_with_no_generation_markers() {
    let root = scratch("guard-legacy");
    let legacy = root.join("live").join("jitendex");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("meta.json"), b"{}").unwrap();
    std::fs::write(legacy.join("terms.ndjson"), b"").unwrap();

    let staged = root.join("staged");
    std::fs::create_dir_all(&staged).unwrap();

    for destination in [legacy.clone(), root.join("live")] {
        let error = sakatsu_core::setup::publication::publish_prepared(
            &root,
            &[("dictionaries".into(), staged.clone(), destination.clone())],
        )
        .expect_err("a legacy dictionary is still a dictionary");
        assert!(
            error.contains("immutable generations"),
            "{destination:?}: {error}"
        );
        // `publish_prepared` creates `root/backup` and refuses to clear a stale one, so each
        // attempt needs its own root to fail for the reason under test rather than on the backup.
        let _ = std::fs::remove_dir_all(root.join("backup"));
    }
}

/// Recovery survives its owner being dropped, and a later publication after that.
///
/// **A lease dies with its owner.** When a transaction neither settles nor rolls back — it failed
/// and `publish_offline` returned, or the roll-back itself failed — the container is left on the
/// new generation with the old one still needed, and the `PreparedGeneration` holding the lease is
/// gone. The protection lasted exactly as long as the function that failed, so the next publication
/// would reclaim the bytes a person still needs. The direct retry test cannot see this, because it
/// keeps the owner alive.
///
/// So the retention is written into the generation directory and outranks leases in
/// `prune_generations`. Dropped without completing is the case under test, and it is the one that
/// matters: settling and rolling back both have an owner present to clean up after themselves.
#[test]
fn a_recovery_point_survives_its_owner_and_a_later_publication() {
    let root = scratch("recovery-survives");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let recovery_point = std::fs::read_to_string(container.join("current")).unwrap();

    // Activated, then abandoned: neither settled nor rolled back, which is what a transaction that
    // failed after this container had switched leaves behind.
    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }
    assert_ne!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        recovery_point,
        "the container is left on the new generation, which is why the old one is needed"
    );

    // An unrelated publication into the same container, which prunes as its last leg.
    let third = work.join("third.zip");
    archive(&third, false);
    drop(Dictionary::import(&third, &container).unwrap());

    assert!(
        container.join(&recovery_point).is_dir(),
        "the recovery point must survive the transaction that needed it"
    );
    let bytes = container.join(&recovery_point).join("terms.ndjson");
    assert!(
        bytes.is_file() && std::fs::metadata(&bytes).unwrap().len() > 0,
        "and survive with its contents, not as an empty directory"
    );
}

/// A transaction that *did* clean up leaves nothing retained.
///
/// The counterpart, and it is not decoration: a retention that is never released is a disk leak
/// that grows one generation per failed install. A successful roll-back consumes its own recovery
/// point — the old generation is live again, and what protects it is that it is current — so a
/// later publication may supersede and reclaim it like any other.
#[test]
fn a_completed_roll_back_leaves_nothing_retained() {
    let root = scratch("recovery-released");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    archive(&work.join("second.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");
    install_one(&dictionaries, &work, "second.zip");

    let container = dictionaries.join("first");
    let original = std::fs::read_to_string(container.join("current")).unwrap();

    // A package that activates the first container and then fails on the second, so the first is
    // rolled back successfully.
    archive(&work.join("first.zip"), true);
    archive(&work.join("second.zip"), true);
    let first = prepare_one(&dictionaries, &work, "first.zip");
    let second = prepare_one(&dictionaries, &work, "second.zip");
    let blocked = dictionaries.join("second").join("current");
    std::fs::remove_file(&blocked).unwrap();
    std::fs::create_dir(&blocked).unwrap();

    publish_offline(&root, &[], vec![first, second]).expect_err("the second cannot activate");
    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        original,
        "the first was rolled back"
    );

    // Superseded by an ordinary import, and reclaimed like any other unprotected generation.
    let third = work.join("third.zip");
    archive(&third, false);
    drop(Dictionary::import(&third, &container).unwrap());
    assert!(
        !container.join(&original).exists(),
        "a consumed recovery point must not keep a generation alive forever"
    );
}

/// An activation that fails must not have switched.
///
/// **The hole this closes was between the switch and the record.** `activate` used to learn which
/// generation it had published by reading `current` back *after* switching — a fallible call in the
/// one place where failure is unrecoverable, because returning an error for a container that had in
/// fact activated means `publish_offline` rolls back every dictionary except that one. The name is
/// now reported by the publication helper, so nothing fallible runs between the switch and the
/// recovery state being recorded.
///
/// What that leaves is the property asserted here: a failed activation changed nothing.
#[test]
fn an_activation_that_fails_leaves_the_container_and_no_record() {
    let root = scratch("activate-fails");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let before = std::fs::read_to_string(container.join("current")).unwrap();

    archive(&work.join("first.zip"), true);
    let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
    // Take the built generation away, so the publish cannot rename it into place.
    std::fs::remove_dir_all(prepared.staging_path()).unwrap();

    prepared
        .activate()
        .expect_err("there is nothing to publish");
    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        before,
        "a failed activation must not have moved the pointer"
    );
    assert!(
        sakatsu_core::dict::RecoveryRecord::read(&container)
            .unwrap()
            .is_none(),
        "and must not leave a record describing a transaction that never happened"
    );
}

/// A legacy dictionary is protected by the record too.
///
/// **The marker scheme could not express this at all.** A legacy layout *is* the container
/// directory, so there was no generation to put a marker in — `activate` skipped writing one in
/// exactly that case, and the legacy cleanup in `prune_generations` consulted leases only. So
/// abandoning a legacy-to-generation transaction dropped its only protection, and the next
/// publication deleted the very bytes it existed to recover from.
///
/// The record says `previous: None` for that shape, which is a thing a file inside a generation
/// could never have said.
#[test]
fn an_abandoned_legacy_transaction_keeps_the_legacy_bytes() {
    let root = scratch("legacy-recovery");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    let container = dictionaries.join("first");

    // A legacy install: imported, then flattened into the pre-generation layout.
    install_one(&dictionaries, &work, "first.zip");
    let generation = container.join(std::fs::read_to_string(container.join("current")).unwrap());
    for name in ["meta.json", "terms.ndjson", "tags.json"] {
        let from = generation.join(name);
        if from.exists() {
            std::fs::copy(&from, container.join(name)).unwrap();
        }
    }
    for bucket in ["spelling", "reading"] {
        let from = generation.join(bucket);
        if from.exists() {
            sakatsu_core::setup::resources::copy_tree(&from, &container.join(bucket)).unwrap();
        }
    }
    std::fs::remove_file(container.join("current")).unwrap();
    std::fs::remove_dir_all(&generation).unwrap();
    assert_eq!(
        Dictionary::open(&container)
            .unwrap()
            .lookup("dog")
            .unwrap()
            .len(),
        1,
        "the legacy layout reads"
    );

    // A transaction that activates over it and is then abandoned.
    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }
    let record = sakatsu_core::dict::RecoveryRecord::read(&container)
        .unwrap()
        .expect("an abandoned transaction leaves a record");
    assert!(
        record.previous.is_none(),
        "a legacy layout is recorded as having had no pointer"
    );

    // A later publication must not reclaim the legacy files.
    let third = work.join("third.zip");
    archive(&third, false);
    drop(Dictionary::import(&third, &container).unwrap());
    assert!(
        container.join("terms.ndjson").is_file(),
        "the legacy bytes are the recovery point and must survive"
    );
}

/// Transaction metadata must not change a pinned dictionary's identity.
///
/// **The first design wrote its marker inside the superseded generation**, which
/// `content_fingerprint` hashes in full — so the same dictionary, whose bytes had not changed,
/// reported a different identity for as long as an install was in flight. Resource identity is used
/// to say which release a measurement ran against; a value that moves because something *else* is
/// happening is worse than no value.
#[test]
fn a_transaction_does_not_change_a_dictionarys_content_fingerprint() {
    let root = scratch("fingerprint");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let held = Dictionary::open(&container).unwrap();
    let before = held.content_fingerprint().unwrap();

    // A transaction over the same container, left outstanding.
    archive(&work.join("first.zip"), true);
    let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
    prepared.activate().unwrap();

    assert!(
        sakatsu_core::dict::RecoveryRecord::read(&container)
            .unwrap()
            .is_some(),
        "the transaction really is outstanding, so this is not vacuous"
    );
    assert_eq!(
        held.content_fingerprint().unwrap(),
        before,
        "the pinned generation's identity must not move because a transaction is in flight"
    );
}

/// The recovery record survives a process ending, not merely a value being dropped.
///
/// A lease is memory and a `Drop` is still this process. What the review asked for is the boundary
/// neither can show: the writing process **exits**, and a later one still finds the recovery point
/// protected. So the first half runs in a child, which is killed by returning, and the assertions
/// run here afterwards — the same re-exec idiom `performance.rs` uses for cold starts.
#[test]
fn a_recovery_record_survives_the_process_that_wrote_it() {
    if std::env::var_os(ABANDON_IN_CHILD).is_some() {
        return abandon_a_transaction_for_the_parent();
    }
    let root = scratch("restart");
    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let output = std::process::Command::new(&exe)
        .args([
            "a_recovery_record_survives_the_process_that_wrote_it",
            "--exact",
            "--nocapture",
        ])
        .env(ABANDON_IN_CHILD, &*root)
        .output()
        .expect("spawn a fresh process");
    assert!(
        output.status.success(),
        "the child could not set up the transaction: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The process that wrote the record is gone. Nothing in this one has ever held a lease on that
    // generation, so only what is on disk can protect it.
    let dictionaries = root.join("live");
    let container = dictionaries.join("first");
    let record = sakatsu_core::dict::RecoveryRecord::read(&container)
        .unwrap()
        .expect("the record outlived its writer");
    let recovery_point = record
        .previous
        .clone()
        .expect("the child had a generation to go back to");

    // A publication from this process, which prunes as its last leg.
    let work = root.join("payload");
    let third = work.join("third.zip");
    archive(&third, false);
    drop(Dictionary::import(&third, &container).unwrap());

    assert!(
        container.join(&recovery_point).is_dir(),
        "a recovery point must survive a restart, not merely a scope"
    );
    let bytes = container.join(&recovery_point).join("terms.ndjson");
    assert!(
        bytes.is_file() && std::fs::metadata(&bytes).unwrap().len() > 0,
        "with its contents"
    );

    // **And the unrelated publication left the record alone.** It is not that transaction, so it
    // has no business resolving it — clearing somebody else's recovery point to tidy up after
    // itself is precisely how the protection would be lost a second time, by a different route.
    let after = sakatsu_core::dict::RecoveryRecord::read(&container)
        .unwrap()
        .expect("an unrelated publish must not resolve an outstanding record");
    assert_eq!(
        after.transaction, record.transaction,
        "the record still belongs to the transaction that wrote it"
    );
    assert_eq!(after.state, sakatsu_core::dict::RecoveryState::Activating);
}

const ABANDON_IN_CHILD: &str = "SAKATSU_ABANDON_TRANSACTION_IN";

/// The child half: install, activate, and exit without settling or rolling back.
fn abandon_a_transaction_for_the_parent() {
    let root = PathBuf::from(std::env::var_os(ABANDON_IN_CHILD).expect("the parent names a root"));
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    archive(&work.join("first.zip"), true);
    let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
    prepared.activate().unwrap();
    // Returning from here ends the process with the transaction outstanding, which is exactly the
    // state a crash or a killed installer leaves behind.
    std::mem::forget(prepared);
}

/// A second transaction may not start over an unresolved one.
///
/// **Checking identity when *clearing* cannot protect a record already overwritten.** With one
/// record per container, a second `activate` would replace the only description of what the first
/// did — and with it the protection on the generation the first still needs. The bytes would then
/// be reclaimed by the next prune, having survived every earlier hazard.
#[test]
fn a_second_transaction_is_refused_while_a_record_is_outstanding() {
    let root = scratch("second-transaction");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let recovery_point = std::fs::read_to_string(container.join("current")).unwrap();

    // One transaction, activated and abandoned.
    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }
    let original = sakatsu_core::dict::RecoveryRecord::read(&container)
        .unwrap()
        .expect("outstanding");

    // A second one over the top of it.
    let mut second = prepare_one(&dictionaries, &work, "first.zip");
    let error = second
        .activate()
        .expect_err("an unresolved record must not be overwritten");
    assert!(
        error.to_string().contains("not finished"),
        "the refusal must say why: {error}"
    );

    let kept = sakatsu_core::dict::RecoveryRecord::read(&container)
        .unwrap()
        .expect("the original record survives the refusal");
    assert_eq!(
        kept.transaction, original.transaction,
        "the first transaction still owns the record"
    );
    assert!(
        container.join(&recovery_point).is_dir(),
        "and its protected bytes are still there"
    );
}

/// An unreadable record refuses too.
///
/// It means a transaction was interrupted and its state cannot be established, which is the case
/// where guessing is least affordable — so it is the one case that must not be treated as "no
/// record".
#[test]
fn an_unreadable_record_refuses_a_new_transaction_and_protects_everything() {
    let root = scratch("unreadable-record");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let before: Vec<_> = std::fs::read_dir(&container)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();

    // A torn write, which is what an interrupted record looks like on disk.
    let record = dictionaries.join(".first.recovery.json");
    std::fs::write(&record, b"{\"transaction\": \"half").unwrap();

    let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
    let error = prepared
        .activate()
        .expect_err("an unreadable record is not an absent one");
    assert!(error.to_string().contains("could not be read"), "{error}");

    // And nothing is reclaimed while it stands, including by an ordinary import elsewhere.
    let after: Vec<_> = std::fs::read_dir(&container)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(
        before, after,
        "no generation may be reclaimed under an unreadable record"
    );
}

/// The record says what was attempted; the pointer says what happened.
///
/// **`Activated` was written before the switch**, so an interruption in between left a record
/// asserting an activation that had not occurred. The state is now `Activating` — an intent — and
/// `observe` reconciles it against `current` to say what actually holds.
#[test]
fn an_interrupted_activation_is_observed_as_never_switched() {
    let root = scratch("observe-never");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let live = std::fs::read_to_string(container.join("current")).unwrap();

    // Exactly what an interruption between writing the record and switching leaves behind.
    let record = sakatsu_core::dict::RecoveryRecord {
        transaction: "interrupted".into(),
        previous: Some(live.clone()),
        published: "g-000000000099".into(),
        state: sakatsu_core::dict::RecoveryState::Activating,
    };
    std::fs::write(
        dictionaries.join(".first.recovery.json"),
        serde_json::to_string(&record).unwrap(),
    )
    .unwrap();

    match sakatsu_core::dict::RecoveryRecord::observe(&container)
        .unwrap()
        .expect("something is outstanding")
    {
        sakatsu_core::dict::Outstanding::CleanupOnly { record } => {
            assert_eq!(record.transaction, "interrupted");
        }
        other => panic!("the switch never landed, so nothing needs undoing: {other:?}"),
    }
}

/// And a switch that *did* land, with something newer over it, needs a person.
#[test]
fn a_newer_publication_is_observed_as_a_conflict() {
    let root = scratch("observe-conflict");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");
    let container = dictionaries.join("first");
    let original = std::fs::read_to_string(container.join("current")).unwrap();

    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }
    let ours = std::fs::read_to_string(container.join("current")).unwrap();

    // Somebody else publishes over it. The record is still the abandoned transaction's.
    let third = work.join("third.zip");
    archive(&third, false);
    drop(Dictionary::import(&third, &container).unwrap());

    let observed = sakatsu_core::dict::RecoveryRecord::observe(&container)
        .unwrap()
        .expect("still outstanding");
    match &observed {
        sakatsu_core::dict::Outstanding::ConflictingPublication { record, live } => {
            assert_eq!(record.previous.as_deref(), Some(original.as_str()));
            assert_eq!(record.published, ours);
            assert_ne!(
                live.as_deref(),
                Some(ours.as_str()),
                "something newer is live"
            );
        }
        other => panic!("expected a conflict: {other:?}"),
    }
    assert!(
        observed.needs_a_decision(),
        "undoing this would discard the newer publication, which is not a routine's call"
    );
}

/// A roll-back whose bookkeeping fails can be finished later.
///
/// Restoration and record cleanup are two steps. The second can fail on its own, and a retry then
/// arrives to find the pointer already back where it wanted it — which used to report `Superseded`,
/// telling the caller somebody else had moved on when in fact this transaction had succeeded.
#[test]
fn a_roll_back_whose_cleanup_failed_finishes_on_a_retry() {
    let root = scratch("cleanup-retry");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    let original = std::fs::read_to_string(container.join("current")).unwrap();

    archive(&work.join("first.zip"), true);
    let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
    prepared.activate().unwrap();

    // Block only the *cleanup*: a directory where the record file must be removed.
    let record = dictionaries.join(".first.recovery.json");
    std::fs::remove_file(&record).unwrap();
    std::fs::create_dir(&record).unwrap();
    std::fs::write(record.join("occupied"), b"x").unwrap();

    prepared
        .roll_back()
        .expect_err("the record could not be cleared");
    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        original,
        "but the restoration itself succeeded"
    );

    // Unblock and retry: this must finish, not report a conflict.
    std::fs::remove_dir_all(&record).unwrap();
    prepared
        .roll_back()
        .expect("already restored is not somebody else having moved on");
    assert!(
        sakatsu_core::dict::RecoveryRecord::read(&container)
            .unwrap()
            .is_none(),
        "and the record is finally cleared"
    );
}

/// A failed roll-back says so in the record, for whoever looks next.
#[test]
fn a_failed_roll_back_is_recorded_as_having_been_tried() {
    let root = scratch("rollback-noted");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");

    let container = dictionaries.join("first");
    archive(&work.join("first.zip"), true);
    let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
    prepared.activate().unwrap();

    // Block the restoration itself.
    let pointer = container.join("current");
    std::fs::remove_file(&pointer).unwrap();
    std::fs::create_dir(&pointer).unwrap();

    prepared
        .roll_back()
        .expect_err("the pointer cannot be written");

    let record = sakatsu_core::dict::RecoveryRecord::read(&container)
        .unwrap()
        .expect("still outstanding");
    assert_eq!(
        record.state,
        sakatsu_core::dict::RecoveryState::RollBackFailed,
        "a repair that has already been tried must say so, rather than looking untouched"
    );
}

/// Keeping the current publication resolves an unfinished install that in fact worked.
#[test]
fn keeping_the_current_publication_clears_the_record_and_frees_the_old_bytes() {
    let root = scratch("keep-current");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");
    let container = dictionaries.join("first");
    let superseded = std::fs::read_to_string(container.join("current")).unwrap();

    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }

    sakatsu_core::dict::keep_current(&container, &identity_of(&container))
        .expect("the newer generation is already live");
    assert!(
        sakatsu_core::dict::RecoveryRecord::read(&container)
            .unwrap()
            .is_none(),
        "the record is resolved"
    );

    // The dictionary is the new one, and the old generation is now reclaimable.
    assert_eq!(
        Dictionary::open(&container)
            .unwrap()
            .lookup("cat")
            .unwrap()
            .len(),
        1
    );
    let third = work.join("third.zip");
    archive(&third, false);
    drop(Dictionary::import(&third, &container).unwrap());
    assert!(
        !container.join(&superseded).exists(),
        "nothing protects it any more, so an ordinary publish reclaims it"
    );
}

/// Restoring puts the previous generation back and resolves the record.
#[test]
fn restoring_the_previous_generation_resolves_an_unfinished_install() {
    let root = scratch("restore-previous");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");
    let container = dictionaries.join("first");
    let original = std::fs::read_to_string(container.join("current")).unwrap();

    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }

    sakatsu_core::dict::restore_previous(&container, &identity_of(&container))
        .expect("the previous generation is on disk");
    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        original
    );
    assert!(sakatsu_core::dict::RecoveryRecord::read(&container)
        .unwrap()
        .is_none());
    let back = Dictionary::open(&container).unwrap();
    assert_eq!(
        back.lookup("dog").unwrap().len(),
        1,
        "the old contents answer"
    );
    assert!(back.lookup("cat").unwrap().is_empty());
}

/// A conflict has a supported way out, and it is keeping what is live.
///
/// **Only one of the two actions discards anything.** Restoring would replace a newer publication
/// with an older generation, so it refuses. Keeping discards nothing at all.
///
/// An earlier version refused *both*, which left a state with no action that could resolve it: the
/// report told a person about a conflict and then offered them nothing to do about it.
#[test]
fn a_conflict_is_resolved_by_keeping_what_is_live() {
    let root = scratch("conflict-actions");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");
    let container = dictionaries.join("first");

    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }
    let third = work.join("third.zip");
    archive(&third, false);
    drop(Dictionary::import(&third, &container).unwrap());
    let live = std::fs::read_to_string(container.join("current")).unwrap();

    sakatsu_core::dict::restore_previous(&container, &identity_of(&container))
        .expect_err("restoring would discard the newer publication");

    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        live,
        "a refused action changes nothing"
    );
    assert!(
        sakatsu_core::dict::RecoveryRecord::read(&container)
            .unwrap()
            .is_some(),
        "and leaves the record outstanding"
    );

    // Keeping is supported, because it takes nothing away: the newer install is already the one
    // being served, and this only stops protecting two superseded generations nothing points at.
    sakatsu_core::dict::keep_current(&container, &identity_of(&container))
        .expect("keeping what is live discards nothing");
    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        live,
        "the newer installation is still the one being served"
    );
    assert!(
        sakatsu_core::dict::RecoveryRecord::read(&container)
            .unwrap()
            .is_none(),
        "and the conflict is resolved"
    );
    assert_eq!(
        Dictionary::open(&container)
            .unwrap()
            .lookup("dog")
            .unwrap()
            .len(),
        1,
        "the live dictionary still answers"
    );
}

/// Restoring refuses when the bytes it would name are gone.
///
/// The record is what kept them, so they should be there — but somebody may have deleted them by
/// hand between the failure and the repair, and pointing `current` at a generation that is not
/// there would turn a recoverable state into a broken one.
#[test]
fn restoring_refuses_when_the_previous_generation_has_been_deleted() {
    let root = scratch("restore-missing");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    let dictionaries = root.join("live");
    install_one(&dictionaries, &work, "first.zip");
    let container = dictionaries.join("first");
    let original = std::fs::read_to_string(container.join("current")).unwrap();

    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }
    let live = std::fs::read_to_string(container.join("current")).unwrap();
    std::fs::remove_dir_all(container.join(&original)).unwrap();

    let error = sakatsu_core::dict::restore_previous(&container, &identity_of(&container))
        .expect_err("there is nothing to restore to");
    assert!(
        matches!(
            &error,
            sakatsu_core::dict::DictError::RecoveryTargetUnavailable(_)
        ),
        "the recovery action needs a different diagnosis from a publisher finding an outstanding transaction"
    );
    assert!(error.to_string().contains("no longer on disk"), "{error}");
    assert_eq!(
        std::fs::read_to_string(container.join("current")).unwrap(),
        live,
        "and the container is left working rather than pointed at nothing"
    );
}

/// The sweep reports every container, and a malformed record does not hide the others.
#[test]
fn the_startup_sweep_reports_each_container_including_unreadable_ones() {
    let root = scratch("sweep");
    let work = root.join("payload");
    std::fs::create_dir_all(&work).unwrap();
    archive(&work.join("first.zip"), false);
    archive(&work.join("second.zip"), false);
    archive(&work.join("third.zip"), false);
    let dictionaries = root.join("live");
    for name in ["first.zip", "second.zip", "third.zip"] {
        install_one(&dictionaries, &work, name);
    }

    // One outstanding, one malformed, one clean.
    archive(&work.join("first.zip"), true);
    {
        let mut prepared = prepare_one(&dictionaries, &work, "first.zip");
        prepared.activate().unwrap();
    }
    std::fs::write(dictionaries.join(".second.recovery.json"), b"{ not json").unwrap();

    let found = sakatsu_core::dict::outstanding_recoveries(&dictionaries);
    assert_eq!(
        found.len(),
        2,
        "the clean container is not reported: {found:?}"
    );
    assert!(
        found[0].0.ends_with("first") && found[0].1.is_ok(),
        "the outstanding one is described: {found:?}"
    );
    assert!(
        found[1].0.ends_with("second") && found[1].1.is_err(),
        "and the malformed one arrives as an entry with its reason, not as an absence: {found:?}"
    );
}
