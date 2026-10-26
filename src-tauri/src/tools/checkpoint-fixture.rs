//! Plant the states a desktop acceptance checkpoint needs to observe, inside a scratch profile.
//!
//! ```text
//! cargo run -p sakatsu-desktop --bin checkpoint-fixture -- recovery <device-data-dir>
//! ```
//!
//! A thin command line over [`sakatsu_desktop_lib::checkpoint`], which is where the fixtures are
//! built and where the reasoning for each of them lives. It is a module rather than a binary so
//! that `src-tauri/tests/checkpoint_fixture.rs` can build the same states and then ask
//! `core::search` the exact questions the checklist tells a person to type — a checklist whose
//! queries have never been run is a list of things to be surprised by.
//!
//! | Command | Plants |
//! |---|---|
//! | `recovery` | four dictionary containers left mid-transaction |
//! | `unblock` | puts the blocked one's generation back, so the retry can be observed |
//! | `resolve <name>` | clears one record from outside, making a panel already on screen stale |
//! | `loading [--empty-kanji]` | an interrupted install whose components load, need no loading, or fail |
//! | `touch-old <name>` | changes a moved-from resource, so deferred cleanup keeps it and says why |
//! | `report` | what is outstanding, to confirm a fixture before spending a launch on it |

use std::path::PathBuf;

use sakatsu_desktop_lib::checkpoint;

/// What is actually going to run, for the acceptance record.
///
/// **A commit and a dirty-path count do not identify this build.** 286 uncommitted paths is a
/// number, not a state; two trees with the same count can differ everywhere. So this reports the
/// bytes: the executable's own digest, and a content digest of the interface source, because
/// `tauri dev` serves the UI from source rather than from a built bundle.
fn identity(root: &std::path::Path, ui_only: bool) -> Vec<String> {
    use sha2::{Digest, Sha256};

    let mut said = Vec::new();
    if !ui_only {
        // **This is the file on disk now, which is not necessarily the one that will run.**
        // `cargo tauri dev` rebuilds, so a hash taken before it can identify yesterday's binary.
        // The launcher hashes the *running* executable after the window appears and treats this as
        // preliminary; `--ui-only` is how it asks for the half that is not about to change.
        let exe = root
            .join("target")
            .join("debug")
            .join("sakatsu-desktop.exe");
        match std::fs::read(&exe) {
            Ok(bytes) => said.push(format!(
                "executable           {} ({} bytes) -- PRELIMINARY, before any rebuild",
                sakatsu_core::hashing::hex(Sha256::digest(&bytes)),
                bytes.len()
            )),
            Err(error) => said.push(format!(
                "executable           NOT BUILT at {} ({error})",
                exe.display()
            )),
        }
    }
    // **Source identity, and labelled as such.** `tauri dev` serves the interface from source and
    // reloads it on a save, so this identifies what was on disk when it was taken, not necessarily
    // what stayed on screen. The launcher takes it twice and says if it moved.
    for (what, path) in [
        ("ui/src (source)", root.join("ui").join("src")),
        ("ui/tests (source)", root.join("ui").join("tests")),
    ] {
        match sakatsu_core::content::fingerprint(&path) {
            Ok(digest) => said.push(format!("{what:<20} {digest}")),
            Err(error) => said.push(format!("{what:<20} unreadable: {error}")),
        }
    }
    for file in ["ui/index.html", "ui/package-lock.json"] {
        let path = root.join(file);
        match std::fs::read(&path) {
            Ok(bytes) => said.push(format!(
                "{file:<20} {}",
                sakatsu_core::hashing::hex(Sha256::digest(&bytes))
            )),
            Err(error) => said.push(format!("{file:<20} unreadable: {error}")),
        }
    }
    said
}

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let device = PathBuf::from(args.next().unwrap_or_else(|| {
        eprintln!("usage: checkpoint-fixture <command> <device-data-dir> [argument]");
        std::process::exit(2);
    }));
    let rest: Vec<String> = args.collect();
    let first = rest.first().map(String::as_str);

    let outcome = match command.as_str() {
        "recovery" => checkpoint::plant_recovery(&device).map(|()| {
            let mut said = vec![format!(
                "planted {} containers under {}",
                checkpoint::FIXTURES.len(),
                device.join("dictionaries").display()
            )];
            for fixture in &checkpoint::FIXTURES {
                said.push(format!(
                    "  {:<20} previous: {:<12} current: {}",
                    fixture.container, fixture.previous, fixture.current
                ));
            }
            said.push(format!(
                "  {} cannot be restored until `unblock` runs",
                checkpoint::BLOCKED.container
            ));
            said.extend(checkpoint::report(&device));
            said
        }),
        "unblock" => checkpoint::unblock(&device).map(|back| {
            vec![
                format!("put {} back", back.display()),
                "the retry in the panel should now succeed".to_string(),
            ]
        }),
        "resolve" => {
            let container = first.unwrap_or(checkpoint::STALE.container);
            checkpoint::resolve(&device, container).map(|record| {
                vec![
                    format!("cleared {}", record.display()),
                    "the panel on screen is now describing a moment that has passed".to_string(),
                ]
            })
        }
        "loading" => {
            let empty = rest.iter().any(|flag| flag == "--empty-kanji");
            checkpoint::plant_loading(&device, empty).map(|root| {
                let mut said =
                    vec![format!("planted an interrupted install at {}", root.display())];
                if empty {
                    said.push(
                        "  kanji           restored with an empty table: expect a loading FAILURE"
                            .into(),
                    );
                    said.push(
                        "                  NOTE: this profile's kanji table is now empty on purpose."
                            .into(),
                    );
                    said.push(
                        "                  Use a fresh profile for any row needing kanji details."
                            .into(),
                    );
                } else {
                    said.push(
                        "  kanji           restored and readable: expect \"in use now\"".into(),
                    );
                    said.push(
                        "  frequency-jpdb  not the selected list: expect \"nothing else is needed\""
                            .into(),
                    );
                    said.push(
                        "  wordnet         nothing to measure: expect \"not established\"".into(),
                    );
                }
                said
            })
        }
        "touch-old" => checkpoint::touch_old(&device, first.unwrap_or("kanji")).map(|marker| {
            vec![
                format!("changed {}", marker.display()),
                "cleanup must now keep this folder and say why".to_string(),
            ]
        }),
        "report" => {
            let lines = checkpoint::report(&device);
            Ok(if lines.is_empty() {
                vec![format!("nothing outstanding under {}", device.display())]
            } else {
                lines
            })
        }
        // `identity` takes the repository root rather than a profile: what it reports is what will
        // be launched, which is not a property of any profile.
        "identity" => Ok(identity(
            &device,
            rest.iter().any(|flag| flag == "--ui-only"),
        )),
        other => Err(format!("no such fixture command: {other:?}")),
    };

    match outcome {
        Ok(lines) => lines.iter().for_each(|line| println!("{line}")),
        Err(error) => {
            eprintln!("checkpoint-fixture: {error}");
            std::process::exit(1);
        }
    }
}
