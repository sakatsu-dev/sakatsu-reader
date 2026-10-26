//! The states a desktop acceptance checkpoint needs to look at, planted in a scratch profile.
//!
//! # Why this is a module and not just a binary
//!
//! The rows this prepares are only worth observing if the preparation is right, and "right" here
//! includes things a person cannot see by looking at the panel: that four containers are genuinely
//! *different* dictionaries, and that the exact queries the checklist tells somebody to type
//! actually reach them. Written only as a binary, none of that could be checked before a launch was
//! spent on it — and the first draft got both wrong. `src-tauri/tests/checkpoint_fixture.rs` runs
//! these builders and then asks `core::search` the prescribed questions.
//!
//! # Everything is produced by running the real primitives
//!
//! An interrupted container is a real `Dictionary::prepare` + `activate` left unsettled; an
//! interrupted install is a real `publish_with_commit` whose commit refuses. A hand-written journal
//! is a shape that looks right, and this branch has already paid for one of those.
//!
//! # No path is resolved from the environment
//!
//! The device-data directory arrives as an argument. Nothing here can reach an installed resource,
//! because nothing here asks where one is.

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::dict::Dictionary;

/// One prepared container, and the two words that tell its generations apart.
///
/// **Distinct per container, and searchable.** The first draft gave all three the same two words,
/// so "restore this one and watch its old word come back" could not distinguish the container that
/// was restored from the two that were not — every one of them answered. These are one token each,
/// and they appear in the **gloss** as well as the headword because an ASCII query takes the English
/// path, which searches glosses. `checkpoint_fixture.rs` verifies that with the real search rather
/// than trusting this comment.
pub struct Fixture {
    pub container: &'static str,
    /// The word only the generation being restored *to* contains.
    pub previous: &'static str,
    /// The word only the generation that is live contains.
    pub current: &'static str,
}

/// The container the checkpoint restores.
pub const RESTORE: Fixture = Fixture {
    container: "checkpoint-restore",
    previous: "restorable",
    current: "restoreheld",
};
/// The container the checkpoint keeps.
pub const KEEP: Fixture = Fixture {
    container: "checkpoint-keep",
    previous: "keepable",
    current: "keepheld",
};
/// The container whose restoration fails until its obstruction is removed.
pub const BLOCKED: Fixture = Fixture {
    container: "checkpoint-blocked",
    previous: "blockable",
    current: "blockheld",
};
/// The container the stale-action row uses.
///
/// **Its own container, because the stale row needs one that is still outstanding.** Rows 2 and 3
/// both used `checkpoint-keep`, and row 2 resolves it — so following the checklist in the order it
/// is written left row 3 with nothing to act on.
pub const STALE: Fixture = Fixture {
    container: "checkpoint-stale",
    previous: "stalable",
    current: "staleheld",
};

pub const FIXTURES: [Fixture; 4] = [RESTORE, KEEP, BLOCKED, STALE];

fn dictionaries(device: &Path) -> PathBuf {
    device.join("dictionaries")
}

fn work(device: &Path) -> PathBuf {
    device.join(".checkpoint-work")
}

/// A one-word Yomitan archive whose token is both the headword and the gloss.
fn archive(path: &Path, token: &str) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    for (name, value) in [
        (
            "index.json",
            serde_json::json!({"title": format!("Checkpoint fixture ({token})"), "format": 3}),
        ),
        (
            "term_bank_1.json",
            // Headword and gloss, so the query finds it whichever path a search takes.
            serde_json::json!([[token, token, "n", "", 0, [token], 1, ""]]),
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .map_err(|error| error.to_string())?;
        zip.write_all(value.to_string().as_bytes())
            .map_err(|error| error.to_string())?;
    }
    zip.finish().map_err(|error| error.to_string())?;
    Ok(())
}

/// One container left mid-transaction: the pointer has moved and the record of how to go back is
/// still outstanding.
///
/// `std::mem::forget` is how a process arranges what dying between activating and settling leaves.
fn interrupted_container(work: &Path, container: &Path, fixture: &Fixture) -> Result<(), String> {
    let first = work.join(format!("{}-previous.zip", fixture.container));
    archive(&first, fixture.previous)?;
    drop(Dictionary::import(&first, container).map_err(|error| error.to_string())?);

    let second = work.join(format!("{}-current.zip", fixture.container));
    archive(&second, fixture.current)?;
    let mut prepared =
        Dictionary::prepare(&second, container).map_err(|error| error.to_string())?;
    prepared.activate().map_err(|error| error.to_string())?;
    std::mem::forget(prepared);
    Ok(())
}

/// Plant the four containers, and move the blocked one's previous generation aside.
pub fn plant_recovery(device: &Path) -> Result<(), String> {
    let dir = dictionaries(device);
    let work = work(device);
    std::fs::create_dir_all(&work).map_err(|error| error.to_string())?;
    for fixture in &FIXTURES {
        let container = dir.join(fixture.container);
        if container.exists() {
            std::fs::remove_dir_all(&container).map_err(|error| error.to_string())?;
        }
        let _ = std::fs::remove_file(dir.join(format!(".{}.recovery.json", fixture.container)));
        interrupted_container(&work, &container, fixture)?;
    }

    // **The obstruction: the generation to go back to is not there.**
    //
    // Moved aside rather than made unreadable, and the difference decides which row this is. An
    // unreadable *pointer* makes the whole record unreadable, and the panel then correctly offers
    // no actions at all — a different state, and not the one being observed. With the previous
    // generation moved out of the container the record still reads, both actions are still offered,
    // and it is the restoration that fails, with the reason the reader needs.
    let record: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.join(format!(".{}.recovery.json", BLOCKED.container)))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let previous = record["previous"]
        .as_str()
        .ok_or("the blocked container has no previous generation to hide")?
        .to_string();
    std::fs::write(work.join("blocked-generation.txt"), &previous)
        .map_err(|error| error.to_string())?;
    std::fs::rename(
        dir.join(BLOCKED.container).join(&previous),
        work.join("blocked-generation"),
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Put the blocked container's generation back, so the retry can be observed.
pub fn unblock(device: &Path) -> Result<PathBuf, String> {
    let work = work(device);
    let previous = std::fs::read_to_string(work.join("blocked-generation.txt"))
        .map_err(|error| format!("no obstruction was recorded here: {error}"))?;
    let back = dictionaries(device)
        .join(BLOCKED.container)
        .join(previous.trim());
    std::fs::rename(work.join("blocked-generation"), &back).map_err(|error| error.to_string())?;
    Ok(back)
}

/// Resolve one container's transaction from outside, so an action already on screen is stale.
///
/// The only way to reach that state deliberately: the panel has to have been drawn *before* the
/// record went away.
pub fn resolve(device: &Path, container: &str) -> Result<PathBuf, String> {
    let record = dictionaries(device).join(format!(".{container}.recovery.json"));
    if !record.exists() {
        return Err(format!(
            "{container} has nothing outstanding, so an action on it is already stale"
        ));
    }
    std::fs::remove_file(&record).map_err(|error| error.to_string())?;
    Ok(record)
}

/// An interrupted offline install, with components chosen for what their loading reports.
///
/// `kanji` with a readable table loads; the same component with an empty one is restored and
/// **fails** to load; an unselected frequency list needs no loading; `wordnet` reloads with no
/// emptiness to ask about, so its outcome is honestly unknown.
pub fn plant_loading(device: &Path, empty_kanji: bool) -> Result<PathBuf, String> {
    let root = device.join(".offline-setup-checkpoint-1");
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;

    let mut members = Vec::new();
    // **What is already installed is moved aside, not overwritten.** The destination has to be
    // absent for recovery to restore into it — one that exists is retained instead, which is a
    // different row — so the profile's own copy is what goes into the backup and comes back.
    let mut plant = |id: &str,
                     destination: PathBuf,
                     files: Vec<(&str, &str)>,
                     replace: bool|
     -> Result<(), String> {
        let staged = root.join(format!("staged-{id}"));
        std::fs::create_dir_all(&staged).map_err(|error| error.to_string())?;
        std::fs::write(staged.join("placeholder"), b"new").map_err(|error| error.to_string())?;
        let existed = destination.is_dir();
        if existed && replace {
            std::fs::remove_dir_all(&destination).map_err(|error| error.to_string())?;
        }
        std::fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
        if !existed || replace {
            for (name, contents) in files {
                std::fs::write(destination.join(name), contents)
                    .map_err(|error| error.to_string())?;
            }
        }
        members.push((id.to_string(), staged, destination));
        Ok(())
    };

    plant(
        "kanji",
        device.join("kanji"),
        vec![("kanjidic.tsv", if empty_kanji { "" } else { KANJI_ROW })],
        empty_kanji,
    )?;
    if !empty_kanji {
        // `destination("frequency-jpdb")` is `dictionaries/frequency/jpdb`: the id and the
        // directory deliberately differ, and the panel must still place it.
        plant(
            "frequency-jpdb",
            dictionaries(device).join("frequency").join("jpdb"),
            vec![("term_meta_bank_1.json", FREQUENCY_ROW)],
            false,
        )?;
        plant(
            "wordnet",
            device.join("wordnet"),
            vec![("categories.tsv", "")],
            false,
        )?;
    }

    // The publisher writes the journal before its first rename; a refused commit gives us that
    // journal and a rollback. Moving each destination into `backup/<id>` afterwards reproduces what
    // a process stopped between the two renames leaves.
    sakatsu_core::setup::publication::publish_with_commit(&root, &members, || {
        Err("stopped, deliberately".to_string())
    })
    .err()
    .ok_or("the commit was supposed to refuse")?;

    let backup = root.join("backup");
    std::fs::create_dir_all(&backup).map_err(|error| error.to_string())?;
    for (id, _, destination) in &members {
        std::fs::rename(destination, backup.join(id))
            .map_err(|error| format!("could not move {} aside: {error}", destination.display()))?;
    }
    if !root.join("publication.json").is_file() {
        return Err("the publisher left no journal, so this is not the real shape".into());
    }
    Ok(root)
}

/// One kanji row, for a profile with no table of its own. Real data is copied in by the launcher;
/// this is a stand-in so the row stays observable without it.
const KANJI_ROW: &str = "\u{8a9e}\t\u{30b4}\t\u{304b}\u{305f}-\u{308b}\tword\t14\t2\n";
/// One frequency entry, in the shape the ranking reads.
const FREQUENCY_ROW: &str = "[[\"\u{8a9e}\",\"freq\",1]]";

/// Change a resource in its old location, after a move, so deferred cleanup must keep it.
pub fn touch_old(device: &Path, name: &str) -> Result<PathBuf, String> {
    let old = device.join(name);
    if !old.is_dir() {
        return Err(format!(
            "{} is not here — run this against the profile's ORIGINAL device-data directory, \
             after the move and before the restart",
            old.display()
        ));
    }
    let marker = old.join("changed-since-the-move.txt");
    std::fs::write(&marker, b"a file that arrived after the move was agreed to")
        .map_err(|error| error.to_string())?;
    Ok(marker)
}

/// What is outstanding under this profile, for confirming a fixture before a launch is spent on it.
pub fn report(device: &Path) -> Vec<String> {
    let mut lines: Vec<String> = sakatsu_core::recovery::dictionaries(&dictionaries(device))
        .into_iter()
        .map(|entry| {
            format!(
                "  {:<20} {:?}{}",
                entry.name,
                entry.state,
                entry
                    .detail
                    .map(|why| format!(" — {why}"))
                    .unwrap_or_default()
            )
        })
        .collect();
    if let Ok(entries) = std::fs::read_dir(device) {
        for path in entries.flatten().map(|entry| entry.path()).filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".offline-setup-"))
        }) {
            lines.push(format!("  interrupted install {}", path.display()));
        }
    }
    lines
}
