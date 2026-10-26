//! Every dictionary line a particle sense claims is still a line the dictionary prints.
//!
//! The reference page fuses the taught particle senses with the dictionary's own into one
//! list, and the join is by text: a taught sense names the exact line the entry draws — its
//! glosses joined with `"; "` — and the interface pairs them on that string.
//!
//! Text rather than sense number, because JMdict reorders and inserts senses between
//! releases. An index survives that silently and points at the wrong reading; a line does
//! not survive it at all, which is the point. This is what turns "does not survive" into a
//! message naming the particle, the sense and the string that no longer matches.
//!
//! It reads the installed dictionary rather than a fixture, for the reason `badge_tags.rs`
//! does: a fixture would be a second copy of the thing being checked, and would agree with
//! the claim while the real dictionary disagreed.
//!
//! ```text
//! cargo test -p sakatsu-core --test particle_glosses -- --nocapture
//! ```

use std::collections::BTreeSet;
use std::path::PathBuf;

use sakatsu_core::grammar::particles;

mod common;

fn dictionary() -> Option<PathBuf> {
    let path = common::dictionaries_dir()
        .join("jitendex")
        .join("terms.ndjson");
    path.exists().then_some(path)
}

/// Every line the dictionary prints for a headword, as the interface would draw it.
fn lines_for(path: &PathBuf, headword: &str) -> BTreeSet<String> {
    let file = std::fs::read_to_string(path).expect("read the installed dictionary");
    let mut lines = BTreeSet::new();
    for row in file.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(row) else {
            continue;
        };
        if entry["term"].as_str() != Some(headword) {
            continue;
        }
        let Some(definitions) = entry["definitions"].as_array() else {
            continue;
        };
        for definition in definitions {
            let Some(glosses) = definition["glosses"].as_array() else {
                continue;
            };
            let joined: Vec<&str> = glosses.iter().filter_map(|g| g.as_str()).collect();
            if !joined.is_empty() {
                lines.insert(joined.join("; "));
            }
        }
    }
    lines
}

#[test]
fn every_claimed_dictionary_line_is_one_the_dictionary_still_prints() {
    let Some(path) = dictionary() else {
        eprintln!("jitendex is not installed; nothing to check against");
        return;
    };

    let mut checked = 0;
    let mut problems = Vec::new();
    for entry in particles::all() {
        let available = lines_for(&path, &entry.surface);
        assert!(
            !available.is_empty(),
            "the dictionary has no entry for {} at all, so nothing here can be checked",
            entry.surface
        );
        for sense in &entry.senses {
            for claimed in &sense.dictionary {
                checked += 1;
                if !available.contains(claimed) {
                    problems.push(format!(
                        "  {} / {} claims {claimed:?}, which the dictionary no longer prints",
                        entry.surface, sense.id
                    ));
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "{} of {checked} claimed lines have gone stale:\n{}\n\nThe dictionary's wording \
         changed. Re-read the entry and update `dictionary` in particles.json — do not \
         delete the claim, because losing it silently unpairs the two lists rather than \
         fixing them.",
        problems.len(),
        problems.join("\n")
    );
    eprintln!("{checked} claimed dictionary lines, all still printed");
}

/// A claim has to point at *one* reading to be worth making.
///
/// Two taught senses may share a dictionary sense — に's "where something is" and "when it
/// happens" both answer to `at (place, time); in; on; during`, because that one sense
/// carries place and time together. That is the dictionary's carving and is fine. What is
/// not fine is one taught sense claiming several lines that are unrelated to each other,
/// which would mean the claim was never specific enough to pair on.
#[test]
fn a_sense_claims_at_most_the_handful_of_lines_it_can_justify() {
    for entry in particles::all() {
        for sense in &entry.senses {
            assert!(
                sense.dictionary.len() <= 2,
                "{} / {} claims {} dictionary lines; a reading that answers to that many \
                 is not being paired, it is being guessed at",
                entry.surface,
                sense.id,
                sense.dictionary.len()
            );
            let unique: BTreeSet<&String> = sense.dictionary.iter().collect();
            assert_eq!(
                unique.len(),
                sense.dictionary.len(),
                "{} / {} names the same line twice",
                entry.surface,
                sense.id
            );
        }
    }
}
