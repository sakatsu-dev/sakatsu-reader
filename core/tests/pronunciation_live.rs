//! Tests against the **real** pronunciation archive.
//!
//! Ignored by default: they need `scripts/fetch-pronunciation.ps1` to have downloaded
//! `tofugu-pronunciation-master.zip`, and CI has no recordings. Run them after touching the
//! importer:
//!
//! ```text
//! cargo test -p sakatsu-core --test pronunciation_live -- --ignored --nocapture
//! ```
//!
//! **This file exists because the hand-built fixture was wrong in a way no unit test could
//! notice.** Every one of the 6,355 recordings is named `word【reading】.mp3` with the *black*
//! lenticular brackets, U+3010 and U+3011. The importer looked for the *white* pair, U+3016 and
//! U+3017 — visually near-identical, and written into both the Rust parser and
//! `scripts/fetch-pronunciation.ps1`. The inline test used the white pair too, so it agreed with
//! the code and disagreed with reality: the suite was green while the very first entry the
//! importer read failed and the whole pack was rejected as malformed. Offline pronunciation had
//! never worked, by either route.
//!
//! The same reasoning as `jitendex_live.rs`, which says it plainly: hand-built fixtures agree with
//! whatever the author believed.

use std::path::{Path, PathBuf};

use sakatsu_core::setup::audio_pack;

/// Where the fetch puts the archive. Both roots are searched, because a device that installed
/// before the local-data root existed still has it under the roaming one.
fn archive() -> Option<PathBuf> {
    let relative = Path::new("com.sakatsu.app")
        .join("audio")
        .join("tofugu-vocabulary")
        .join("work")
        .join("tofugu-pronunciation-master.zip");
    ["LOCALAPPDATA", "APPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join(&relative))
        .find(|candidate| candidate.is_file())
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sakatsu-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "needs scripts/fetch-pronunciation.ps1 to have run"]
fn the_real_archive_imports_every_recording() {
    let Some(archive) = archive() else {
        println!("SKIP: no pronunciation archive — run scripts/fetch-pronunciation.ps1");
        return;
    };

    let scratch = Scratch::new("pronunciation-live");
    let destination = scratch.0.join("installed");
    let imported = audio_pack(&archive, &destination)
        .unwrap_or_else(|error| panic!("the real archive must import cleanly, got: {error}"));

    // Thousands, not a handful: a parser that recognised only some naming pattern would still
    // produce a plausible-looking pack, and a count in the hundreds is the shape that would take.
    assert!(
        imported > 6_000,
        "expected the whole archive, imported only {imported}"
    );

    let index = std::fs::read_to_string(destination.join("index.tsv")).expect("index.tsv");
    assert_eq!(
        index.lines().count(),
        imported,
        "one line per recording, and no line lost to a stray tab or newline in a filename"
    );

    // Three entries read out of the archive itself. `〜丁目` is the one the reported error named
    // and the first the importer ever reached — a suffix, so its `〜` has to survive as part of
    // the word rather than being trimmed away as decoration.
    for (surface, reading) in [("上手", "じょうず"), ("水", "みず"), ("〜丁目", "ちょうめ")]
    {
        assert!(
            index
                .lines()
                .any(|line| line.starts_with(&format!("{surface}\t{reading}\t"))),
            "{surface}【{reading}】 is in the archive and must reach the index"
        );
    }

    assert!(
        destination.join("LICENSE").is_file(),
        "CC BY-SA 4.0 travels with the recordings"
    );
    assert!(destination.join("meta.json").is_file());
}
