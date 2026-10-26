//! The application finds its dictionaries in either on-disk layout.
//!
//! # Why this is a shell test and not a core one
//!
//! `core` proved that a published generation opens, that a held reader keeps its files, and that a
//! failed publish changes nothing. None of that was reachable from the application, because
//! discovery decided what a dictionary *was* by looking for `meta.json` directly inside the
//! directory — the pre-generation layout. Five call sites did that, and every one of them silently
//! stopped seeing a dictionary the moment it was published as a generation: it vanished from
//! discovery, the setup panel called the component missing, and the backfill enumerated nothing.
//!
//! Every passing test in `dict::index` missed it, because the storage contract was
//! correct and nothing connected it to the callers. So these tests go through
//! `discover_dictionaries` — the function the app actually starts with — and then look a word up
//! through the reader it produced.

mod common;

use std::path::Path;

use sakatsu_core::dict::index::Dictionary;
use serde_json::json;

fn scratch(name: &str) -> common::Scratch {
    common::Scratch::new("dictionary-discovery", name)
}

/// The smallest Yomitan archive that imports.
fn write_archive(path: &Path) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let members = [
        (
            "index.json",
            json!({ "title": "Discovery Test", "format": 3 }).to_string(),
        ),
        (
            "term_bank_1.json",
            json!([["\u{732b}", "\u{306d}\u{3053}", "n", "", 10, ["cat"], 1, ""]]).to_string(),
        ),
    ];
    for (name, contents) in members {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .expect("member");
        std::io::Write::write_all(&mut writer, contents.as_bytes()).expect("write");
    }
    writer.finish().expect("finish");
}

/// Everything the app does at startup, and the question it exists to answer.
fn discovered_can_answer(dir: &Path) -> bool {
    let state = sakatsu_desktop_lib::discover_dictionaries(dir);
    let held = state.0.now();
    held.dictionaries.len() == 1
        && held.dictionaries[0]
            .lookup("\u{732b}")
            .map(|hits| !hits.is_empty())
            .unwrap_or(false)
}

/// Move a published generation back up into the directory, as a pre-generation install.
fn flatten_to_legacy(dir: &Path) {
    let generation = Dictionary::open(dir).expect("open").root().to_path_buf();
    assert_ne!(generation, dir, "there is a generation to flatten");
    for entry in std::fs::read_dir(&generation).expect("read") {
        let entry = entry.expect("entry");
        std::fs::rename(entry.path(), dir.join(entry.file_name())).expect("move up");
    }
    std::fs::remove_dir_all(&generation).expect("drop the generation");
    std::fs::remove_file(dir.join("current")).expect("drop the pointer");
}

/// A dictionary imported into an empty directory is found and answers.
///
/// This is the case that was broken: a fresh install has no `meta.json` at the top level, only a
/// `current` file and a generation beside it.
#[test]
fn a_freshly_imported_dictionary_is_discovered_and_answers() {
    let root = scratch("fresh");
    let dictionaries = root.join("dictionaries");
    std::fs::create_dir_all(&dictionaries).expect("create");
    let archive = root.join("d.zip");
    write_archive(&archive);

    Dictionary::import(&archive, &dictionaries.join("testdict")).expect("import");
    assert!(
        !dictionaries.join("testdict").join("meta.json").is_file(),
        "the fixture really is the generation layout, not the legacy one"
    );
    assert!(
        discovered_can_answer(&dictionaries),
        "a generation-published dictionary must be discovered by the application"
    );
}

/// A legacy dictionary is still found, and stays found across the migration that reclaims it.
///
/// The installed-dictionary gate runs cover the first half of this and only the first half: they
/// read a legacy layout, they never import, so they never perform the transition. This does.
#[test]
fn a_migrated_dictionary_is_still_discovered_after_its_legacy_files_go() {
    let root = scratch("migrate");
    let dictionaries = root.join("dictionaries");
    std::fs::create_dir_all(&dictionaries).expect("create");
    let archive = root.join("d.zip");
    write_archive(&archive);
    let dir = dictionaries.join("testdict");

    Dictionary::import(&archive, &dir).expect("import");
    flatten_to_legacy(&dir);
    assert!(dir.join("meta.json").is_file(), "the fixture is legacy");
    assert!(
        discovered_can_answer(&dictionaries),
        "a legacy dictionary must still be discovered"
    );

    // Two publishes: the first migrates to generations, the second reclaims the legacy files now
    // that nothing holds them.
    Dictionary::import(&archive, &dir).expect("migrating import");
    Dictionary::import(&archive, &dir).expect("second import");
    assert!(
        !dir.join("meta.json").is_file(),
        "the legacy files were reclaimed, which is the state that used to lose the dictionary"
    );
    assert!(
        discovered_can_answer(&dictionaries),
        "and it must still be discovered afterwards"
    );
}
