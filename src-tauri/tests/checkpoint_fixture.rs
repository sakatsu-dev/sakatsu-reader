//! The checkpoint's fixtures, and the exact queries its checklist tells a person to type.
//!
//! # Why a checklist needs tests
//!
//! A row that fails because its fixture was wrong costs a launch, and worse, it costs trust in the
//! row: somebody has to work out whether the app is broken or the preparation is. Both of the
//! defects this file now guards against were in the first draft.
//!
//! - **All three containers held the same two words.** "Restore this one and watch its old word
//!   come back" could not distinguish the container that was restored from the two that were not,
//!   because every one of them answered.
//! - **The prescribed queries were ASCII**, which takes the **English gloss** path — and the words
//!   were headwords. Nothing in the fixture's glosses matched, so the queries would have found
//!   nothing whatever the app did.
//!
//! So this builds the fixtures with the production code and then asks `core::search` the questions
//! the checklist asks, before anybody is asked to look.

use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::LinderaAnalyzer;
use sakatsu_core::dict::{Dictionary, KanjiDic};
use sakatsu_core::search::{search, Resources};
use sakatsu_desktop_lib::checkpoint;

mod common;
use common::Scratch;

fn profile(name: &str) -> Scratch {
    let scratch = Scratch::new("checkpoint-fixture", name);
    std::fs::create_dir_all(scratch.join("dictionaries")).expect("device data");
    scratch
}

/// Every container in the profile, opened the way a session opens them.
fn opened(device: &Path) -> Vec<Dictionary> {
    Dictionary::discover(&device.join("dictionaries"))
        .into_iter()
        .map(|path| Dictionary::open(&path).expect("the fixture container opens"))
        .collect()
}

/// Which containers answer `query`, by the title their generation carries.
///
/// Titles rather than paths, because the title is what the generation *is*: a container that has
/// been restored carries its previous generation's title, which is exactly the distinction the
/// checklist asks a person to see.
fn answers(
    dictionaries: &[Dictionary],
    kanji: &KanjiDic,
    analyzer: &LinderaAnalyzer,
    query: &str,
) -> Vec<String> {
    let found = search(
        query,
        Resources {
            preferred: analyzer,
            built_in: analyzer,
            dictionaries,
            kanji,
            frequency: None,
        },
    )
    .expect("the search runs");
    found
        .words
        .iter()
        .map(|hit| hit.word.headword.clone())
        .collect()
}

fn kanji_table(device: &Path) -> KanjiDic {
    let path = device.join("kanjidic.tsv");
    std::fs::write(&path, b"").expect("an empty table is enough for these rows");
    KanjiDic::load(&path)
}

/// **The prescribed queries reach exactly one container each.**
///
/// This is the check the first draft could not have passed: one token per container, present as
/// both headword and gloss so an ASCII query finds it on the English path.
#[test]
fn every_container_answers_only_its_own_query() {
    let device = profile("distinct");
    checkpoint::plant_recovery(&device).expect("plant the containers");

    let analyzer = LinderaAnalyzer::new().expect("the built-in analyzer");
    let kanji = kanji_table(&device);
    let dictionaries = opened(&device);
    assert_eq!(
        dictionaries.len(),
        checkpoint::FIXTURES.len(),
        "every fixture container is discoverable"
    );

    for fixture in &checkpoint::FIXTURES {
        // The live generation's word answers; the one it would be restored to does not.
        let current = answers(&dictionaries, &kanji, &analyzer, fixture.current);
        assert_eq!(
            current,
            vec![fixture.current.to_string()],
            "{}'s live word must be findable, and by nothing else: {current:?}",
            fixture.container
        );
        let previous = answers(&dictionaries, &kanji, &analyzer, fixture.previous);
        assert!(
            previous.is_empty(),
            "{}'s previous word must not answer until it is restored: {previous:?}",
            fixture.container
        );
    }

    // And no two fixtures share a word, which is what makes "only its own" mean anything.
    let mut words: Vec<&str> = checkpoint::FIXTURES
        .iter()
        .flat_map(|fixture| [fixture.previous, fixture.current])
        .collect();
    let planted = words.len();
    words.sort_unstable();
    words.dedup();
    assert_eq!(words.len(), planted, "two fixtures share a word: {words:?}");
}

/// Restoring one container changes that one and leaves the rest exactly as they were.
///
/// The row the checklist asks about, run end to end: restore, then ask what answers.
#[test]
fn restoring_one_container_leaves_the_others_alone() {
    let device = profile("restore");
    checkpoint::plant_recovery(&device).expect("plant the containers");

    let container = device
        .join("dictionaries")
        .join(checkpoint::RESTORE.container);
    let outstanding = sakatsu_core::dict::outstanding_recoveries(&device.join("dictionaries"));
    let (_, outcome) = outstanding
        .iter()
        .find(|(path, _)| path == &container)
        .expect("the container is outstanding");
    let record = outcome.as_ref().expect("its record reads").record().clone();

    sakatsu_core::dict::restore_previous(
        &container,
        &sakatsu_core::dict::Identity {
            transaction: record.transaction.clone(),
            published: record.published.clone(),
        },
    )
    .expect("the restoration succeeds");

    let analyzer = LinderaAnalyzer::new().expect("the built-in analyzer");
    let kanji = kanji_table(&device);
    let dictionaries = opened(&device);

    assert_eq!(
        answers(
            &dictionaries,
            &kanji,
            &analyzer,
            checkpoint::RESTORE.previous
        ),
        vec![checkpoint::RESTORE.previous.to_string()],
        "the restored word answers"
    );
    assert!(
        answers(
            &dictionaries,
            &kanji,
            &analyzer,
            checkpoint::RESTORE.current
        )
        .is_empty(),
        "and the one that was undone stops answering"
    );
    for other in [checkpoint::KEEP, checkpoint::BLOCKED, checkpoint::STALE] {
        assert_eq!(
            answers(&dictionaries, &kanji, &analyzer, other.current),
            vec![other.current.to_string()],
            "{} was not restored and must be untouched",
            other.container
        );
    }
}

/// The keep row and the stale row have containers of their own.
///
/// Both used `checkpoint-keep`, and the keep row resolves it — so following the checklist in the
/// order it is written left the stale row with nothing outstanding to act on.
#[test]
fn resolving_the_keep_container_leaves_the_stale_one_outstanding() {
    let device = profile("stale");
    checkpoint::plant_recovery(&device).expect("plant the containers");
    assert_ne!(
        checkpoint::KEEP.container,
        checkpoint::STALE.container,
        "the two rows must not share a transaction"
    );

    checkpoint::resolve(&device, checkpoint::KEEP.container).expect("resolve the keep container");
    let still: Vec<String> = sakatsu_core::recovery::dictionaries(&device.join("dictionaries"))
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert!(
        still.contains(&checkpoint::STALE.container.to_string()),
        "the stale row still has something to act on: {still:?}"
    );
    assert!(
        !still.contains(&checkpoint::KEEP.container.to_string()),
        "and the keep row's is gone, which is what makes it stale: {still:?}"
    );
}

/// The obstruction leaves the record readable, so both actions are still offered.
///
/// The first obstruction made `current` a directory, which broke reading the record as well as
/// writing it: the panel then showed `Unreadable` and correctly offered nothing — a different row.
#[test]
fn the_blocked_container_still_offers_both_actions_and_fails_to_restore() {
    let device = profile("blocked");
    checkpoint::plant_recovery(&device).expect("plant the containers");
    let dictionaries_dir = device.join("dictionaries");

    let entry = sakatsu_core::recovery::dictionaries(&dictionaries_dir)
        .into_iter()
        .find(|entry| entry.name == checkpoint::BLOCKED.container)
        .expect("the blocked container is listed");
    assert_eq!(
        entry.state,
        sakatsu_core::recovery::DictionaryRecoveryState::Unresolved,
        "unresolved is the state that offers both buttons: {entry:?}"
    );

    let container = dictionaries_dir.join(checkpoint::BLOCKED.container);
    let identity = sakatsu_core::dict::Identity {
        transaction: entry.transaction.clone().expect("a transaction"),
        published: entry.published.clone().expect("a generation"),
    };
    let refused = sakatsu_core::dict::restore_previous(&container, &identity)
        .expect_err("the generation to go back to is not there");
    let reason = refused.to_string();
    assert!(
        reason.contains("no longer on disk"),
        "the reason has to name what is missing: {reason}"
    );

    // And the retry succeeds once the obstruction is removed, which is the second half of the row.
    checkpoint::unblock(&device).expect("put the generation back");
    sakatsu_core::dict::restore_previous(&container, &identity).expect("the retry succeeds");
}

/// The loading fixture keeps whatever the profile already had.
#[test]
fn the_loading_fixture_moves_the_profiles_own_files_aside_rather_than_replacing_them() {
    let device = profile("loading");
    let kanji = device.join("kanji");
    std::fs::create_dir_all(&kanji).expect("a table the profile already has");
    std::fs::write(kanji.join("kanjidic.tsv"), b"THE PROFILE'S OWN TABLE").expect("write");

    let root = checkpoint::plant_loading(&device, false).expect("plant the interrupted install");
    assert_eq!(
        std::fs::read_to_string(root.join("backup").join("kanji").join("kanjidic.tsv"))
            .expect("the backup holds it"),
        "THE PROFILE'S OWN TABLE",
        "the profile's copy is what was moved aside"
    );
    assert!(
        !kanji.exists(),
        "and the destination is absent, which is what recovery needs to restore into"
    );

    // The empty variant is the one that replaces it, deliberately.
    let device = profile("loading-empty");
    let kanji = device.join("kanji");
    std::fs::create_dir_all(&kanji).expect("a table the profile already has");
    std::fs::write(kanji.join("kanjidic.tsv"), b"THE PROFILE'S OWN TABLE").expect("write");
    let root = checkpoint::plant_loading(&device, true).expect("plant the interrupted install");
    assert_eq!(
        std::fs::read_to_string(root.join("backup").join("kanji").join("kanjidic.tsv"))
            .expect("the backup holds it"),
        "",
        "the empty variant is the loading-failure row, and says so in the launcher"
    );
}

/// Nothing the fixtures write leaves the profile they were given.
///
/// Asserted rather than argued: the module resolves no path from the environment, and this is what
/// says so. Every file created by every command has to be under the directory handed in.
#[test]
fn every_fixture_writes_only_inside_the_profile() {
    let device = profile("contained");
    let before = tree(&device);

    checkpoint::plant_recovery(&device).expect("plant");
    checkpoint::plant_loading(&device, false).expect("plant");
    checkpoint::unblock(&device).expect("unblock");
    checkpoint::resolve(&device, checkpoint::STALE.container).expect("resolve");
    std::fs::create_dir_all(device.join("kanji")).expect("a folder to change");
    checkpoint::touch_old(&device, "kanji").expect("touch");

    let after = tree(&device);
    assert!(
        after.len() > before.len(),
        "the fixtures wrote something, or this test is checking nothing"
    );
    for path in &after {
        assert!(
            path.starts_with(&*device),
            "{} is outside the profile",
            path.display()
        );
    }
}

fn tree(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            found.push(path);
        }
    }
    found
}
