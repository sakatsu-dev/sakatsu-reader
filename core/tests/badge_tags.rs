//! Every grammatical tag a real dictionary puts on a badge can be explained.
//!
//! This exists because guessing is what produced the gap it guards. The glossary was
//! written from JMdict's documented codes — `v5`, `v1`, `vs`, `uk` — and Jitendex spells
//! its tags out instead: `5-dan`, `1-dan`, `suru`, `kana`. Where a spelled-out form
//! happened to match an alias it worked, and 137 of 160 distinct tags did not, which
//! nobody noticed because nothing was reading the dictionary to check.
//!
//! So the measurement is the test. It reads the installed dictionary rather than a
//! fixture, because a fixture would be another guess about what the tags are.
//!
//! Subject fields — `computing`, `golf`, `Buddhism` — are deliberately **not** required to
//! have entries. They name themselves, and the interface renders an unexplained tag as a
//! plain chip rather than offering an explanation it does not have.
//!
//! ```text
//! cargo test -p sakatsu-core --test badge_tags -- --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

mod common;

/// Tags that name a subject rather than a grammatical property.
///
/// Listed by shape rather than one by one: anything the glossary does not know and that is
/// not in the grammatical list below is treated as a field. The check is therefore about
/// the terms we *claim* to explain, and adding a new field to the dictionary cannot fail
/// the build.
const GRAMMATICAL: &[&str] = &[
    // Parts of speech and verb classes, as Jitendex writes them.
    "noun",
    "5-dan",
    "1-dan",
    "2-dan",
    "suru",
    "su-verb",
    "zuru",
    "adjective",
    "adjectival",
    "na-adj",
    "no-adj",
    "to-adverb",
    "taru",
    "ku-adj",
    "shiku",
    "adverb",
    "particle",
    "conjunction",
    "interjection",
    "pronoun",
    "counter",
    "prefix",
    "suffix",
    "numeric",
    "aux-verb",
    "aux-adj",
    "exp",
    "transitive",
    "intransitive",
    "unclass",
    // How the word is used.
    "kana",
    "abbr.",
    "forms",
    "rare",
    "formal",
    "polite",
    "honorific",
    "humble",
    "colloquial",
    "slang",
    "net slang",
    "derogatory",
    "vulgar",
    "sensitive",
    "euphemism",
    "jocular",
    "familiar",
    "childish",
    "masculine",
    "feminine",
    "poetical",
    "proverb",
    "quote",
    "idiom",
    "yoji",
    "mimetic",
    "archaic",
    "obsolete",
    "dated",
    "historical",
];

fn dictionary() -> Option<PathBuf> {
    let dir = common::dictionaries_dir()
        .join("jitendex")
        .join("terms.ndjson");
    dir.exists().then_some(dir)
}

#[test]
fn every_grammatical_tag_a_badge_can_show_is_explained() {
    let Some(path) = dictionary() else {
        eprintln!("SKIP: no jitendex installed — run scripts/fetch-dictionaries.ps1");
        return;
    };

    // What the dictionary actually emits, counted rather than assumed.
    let text = std::fs::read_to_string(&path).expect("read terms.ndjson");
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(definitions) = entry.get("definitions").and_then(|d| d.as_array()) else {
            continue;
        };
        for definition in definitions {
            let tags = definition.get("tags").and_then(|t| t.as_array());
            for tag in tags.into_iter().flatten().filter_map(|t| t.as_str()) {
                *seen.entry(tag.to_string()).or_default() += 1;
            }
        }
    }
    assert!(!seen.is_empty(), "no tags found in {}", path.display());

    let wanted: BTreeSet<&str> = GRAMMATICAL.iter().copied().collect();
    let mut missing: Vec<(usize, String)> = seen
        .iter()
        .filter(|(tag, _)| wanted.contains(tag.as_str()))
        .filter(|(tag, _)| sakatsu_core::terms::get(tag).is_none())
        .map(|(tag, count)| (*count, tag.clone()))
        .collect();
    missing.sort_unstable_by(|a, b| b.cmp(a));

    eprintln!(
        "{} distinct tags across {} entries; {} of them grammatical",
        seen.len(),
        text.lines().count(),
        seen.keys().filter(|t| wanted.contains(t.as_str())).count()
    );
    assert!(
        missing.is_empty(),
        "grammatical tags with no explanation, most frequent first:\n{}",
        missing
            .iter()
            .map(|(count, tag)| format!("  {count:6}  {tag}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The list above is only worth anything if the tags in it are really on the badges.
///
/// A typo, or a tag Jitendex renames, would otherwise quietly stop being checked — the
/// filter would simply match nothing and the test would pass on an empty set.
#[test]
fn the_grammatical_list_describes_tags_that_actually_appear() {
    let Some(path) = dictionary() else {
        eprintln!("SKIP: no jitendex installed");
        return;
    };
    let text = std::fs::read_to_string(&path).expect("read terms.ndjson");
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        for definition in entry
            .get("definitions")
            .and_then(|d| d.as_array())
            .into_iter()
            .flatten()
        {
            for tag in definition
                .get("tags")
                .and_then(|t| t.as_array())
                .into_iter()
                .flatten()
                .filter_map(|t| t.as_str())
            {
                seen.insert(tag.to_string());
            }
        }
    }

    let absent: Vec<&str> = GRAMMATICAL
        .iter()
        .copied()
        .filter(|tag| !seen.contains(*tag))
        .collect();
    assert!(
        absent.is_empty(),
        "listed as grammatical but never emitted — renamed, or a typo:\n  {}",
        absent.join("\n  ")
    );
}
