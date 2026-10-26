//! The capture corpus, enumerated once for everything that measures over it.
//!
//! # Why this is shared
//!
//! Two harnesses enumerated it independently and disagreed. One accounted for unreadable
//! directories, unreadable transcripts, empty files and an unparseable holdout list; the other
//! skipped each silently, keyed cases on a bare stem so two captures with the same name in
//! different folders would collide, and pooled development and holdout totals. A corpus that
//! half-failed to load would have reported a clean sweep over whatever survived.
//!
//! So the rule lives here: `scripts/capture-inventory.py` is the documented authority for which
//! files are captures and which tier each transcript is, and this is its executable counterpart.
//!
//! # What callers get
//!
//! Every discovered transcript with its **directory-plus-stem identity**, its tier, and its
//! partition — plus the faults, separately, so a required-failure gate can refuse to measure over
//! a corpus that did not fully load. Holdout captures are **labelled, never dropped**: they have
//! already been read once, and a run pretending otherwise would misrepresent the record.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// One capture transcript, chosen the way `scripts/capture-inventory.py` chooses.
pub struct Capture {
    /// `capture/<stem>` — the same id `colloquial_comparison.rs` gives the same file.
    pub id: String,
    pub text: String,
    /// `promoted` once a person reviewed the draft and renamed it; `draft` until then.
    pub tier: &'static str,
    /// `development`, or `holdout` for the seven stems `corpus/reading-gold.json` is built on.
    pub partition: &'static str,
}

/// Suffixes longest-first, because `.txt.mine` must be tried before `.txt` or every draft reads
/// as a reviewed transcript. `scripts/capture-inventory.py` is the documented authority for this
/// rule and for which files are not captures.
pub const TIERS: [&str; 8] = [
    ".txt.suggested",
    ".txt.mine",
    ".tok.mine",
    ".txt",
    ".png",
    ".jpg",
    ".jpeg",
    ".webp",
];
pub const IMAGE_TIERS: [&str; 4] = [".png", ".jpg", ".jpeg", ".webp"];
/// Preferred first: a reviewed transcript beats the draft it was promoted from.
pub const TRANSCRIPT_TIERS: [&str; 2] = [".txt", ".txt.mine"];

pub fn split_tier(name: &str) -> Option<(&str, &'static str)> {
    TIERS
        .iter()
        .find_map(|tier| name.strip_suffix(tier).map(|stem| (stem, *tier)))
}

/// The seven capture stems the existing reading holdout is built on.
///
/// Read so they can be **labelled, not quietly dropped** — every one has a promoted `.txt`, so a
/// sweep of transcripts picks all seven up whether or not it says so. `colloquial_comparison.rs`
/// reads the same file for the same reason.
pub fn reading_holdout_stems() -> Result<BTreeSet<String>, String> {
    let path = super::corpus_dir().join("reading-gold.json");
    // **Neither an unreadable file nor a well-formed empty one can mean "no holdout".** Returning
    // an empty set would relabel seven reserved captures as development material and report that
    // as fact — and `{}` is valid JSON, so parsing successfully proves nothing about structure.
    let raw = std::fs::read(&path)
        .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
    let document: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|error| format!("{} is not JSON: {error}", path.display()))?;
    let cases = document
        .get("cases")
        .ok_or_else(|| format!("{} has no `cases`", path.display()))?
        .as_array()
        .ok_or_else(|| format!("{}'s `cases` is not an array", path.display()))?;
    if cases.is_empty() {
        return Err(format!("{} lists no cases", path.display()));
    }

    let mut stems = BTreeSet::new();
    for (index, case) in cases.iter().enumerate() {
        // Every case must name an image, and that name must yield a stem. A case that does not is
        // a malformed record, not a case with no holdout — `filter_map` used to drop both alike.
        let image = case
            .get("source_image")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                format!(
                    "{}: case {index} has no string `source_image`",
                    path.display()
                )
            })?;
        let stem = std::path::Path::new(image)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .filter(|stem| !stem.is_empty())
            .ok_or_else(|| {
                format!(
                    "{}: case {index}'s `source_image` {image:?} has no file stem",
                    path.display()
                )
            })?;
        stems.insert(stem);
    }
    Ok(stems)
}

/// Every capture with a transcript, and separately the text-only stems that were not counted.
///
/// **A capture is a stem that has an image.** That is structural rather than a name list, so a
/// transcript nobody captured cannot drift into a reader-facing measurement. The stems it excludes
/// are returned rather than dropped, so the exclusion is visible in the report instead of silent.
pub fn captures() -> (Vec<Capture>, Vec<String>, Vec<String>) {
    let root = super::corpus_dir();
    let mut faults: Vec<String> = Vec::new();
    let holdout = match reading_holdout_stems() {
        Ok(stems) => stems,
        Err(reason) => {
            faults.push(format!("the reading holdout is unreadable: {reason}"));
            BTreeSet::new()
        }
    };
    // (directory, stem) -> the file found for each suffix.
    let mut found: BTreeMap<(PathBuf, String), BTreeMap<&'static str, PathBuf>> = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                faults.push(format!("{} could not be listed: {error}", dir.display()));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    faults.push(format!(
                        "an entry under {} is unreadable: {error}",
                        dir.display()
                    ));
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let Some((stem, tier)) = split_tier(&name) else {
                continue;
            };
            found
                .entry((dir.clone(), stem.to_string()))
                .or_default()
                .insert(tier, path);
        }
    }

    let mut captures = Vec::new();
    let mut text_only = Vec::new();
    for ((dir, stem), tiers) in found {
        let has_image = IMAGE_TIERS.iter().any(|tier| tiers.contains_key(tier));
        let Some((path, tier)) = TRANSCRIPT_TIERS
            .iter()
            .find_map(|tier| tiers.get(tier).map(|path| (path.clone(), *tier)))
        else {
            continue;
        };
        if !has_image {
            text_only.push(stem);
            continue;
        }
        // **Discovered before it is read.** Skipping an unreadable or empty transcript silently
        // shrank the denominator, so a corpus that half-failed to load reported a clean sweep over
        // whatever survived. Both are faults now, and both fail the run.
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                faults.push(format!("{} could not be read: {error}", path.display()));
                continue;
            }
        };
        if text.trim().is_empty() {
            faults.push(format!("{} is empty", path.display()));
            continue;
        }
        let partition = if holdout.contains(&stem) {
            "holdout"
        } else {
            "development"
        };
        // **The identity enumeration actually used.** Ids were the bare stem while the sweep
        // keyed on directory-plus-stem, so two captures with the same stem in different folders
        // would have collided into one case id and one of them gone unreported.
        let relative = dir
            .strip_prefix(&root)
            .unwrap_or(std::path::Path::new(""))
            .to_string_lossy()
            .replace('\\', "/");
        let id = if relative.is_empty() {
            format!("capture/{stem}")
        } else {
            format!("capture/{relative}/{stem}")
        };
        captures.push(Capture {
            id,
            text,
            tier: if tier == ".txt" { "promoted" } else { "draft" },
            partition,
        });
    }
    captures.sort_by(|a, b| a.id.cmp(&b.id));
    text_only.sort();
    (captures, text_only, faults)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Valid JSON is not valid structure.**
    ///
    /// `{}` parses. So does `{"cases": []}`, and so does a case with no `source_image`. Each used
    /// to arrive as "no holdout", which relabels seven reserved captures as development material
    /// and reports that as fact. The parse succeeding proves nothing about what was parsed.
    fn stems_from(json: &str) -> Result<BTreeSet<String>, String> {
        let document: serde_json::Value =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        let cases = document
            .get("cases")
            .ok_or_else(|| "no `cases`".to_string())?
            .as_array()
            .ok_or_else(|| "`cases` is not an array".to_string())?;
        if cases.is_empty() {
            return Err("lists no cases".into());
        }
        let mut stems = BTreeSet::new();
        for (index, case) in cases.iter().enumerate() {
            let image = case
                .get("source_image")
                .and_then(|value| value.as_str())
                .ok_or_else(|| format!("case {index} has no string `source_image`"))?;
            let stem = std::path::Path::new(image)
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .filter(|stem| !stem.is_empty())
                .ok_or_else(|| format!("case {index} has no file stem"))?;
            stems.insert(stem);
        }
        Ok(stems)
    }

    #[test]
    fn well_formed_json_with_the_wrong_shape_is_refused() {
        for (json, why) in [
            ("{}", "no `cases`"),
            (r#"{"cases": {}}"#, "not an array"),
            (r#"{"cases": []}"#, "lists no cases"),
            (r#"{"cases": [{"id": "a"}]}"#, "no string `source_image`"),
            (
                r#"{"cases": [{"source_image": 7}]}"#,
                "no string `source_image`",
            ),
            (r#"{"cases": [{"source_image": ""}]}"#, "no file stem"),
        ] {
            let outcome = stems_from(json).expect_err(&format!("{json} must be refused"));
            assert!(
                outcome.contains(why),
                "{json}: expected {why:?}, got {outcome:?}"
            );
        }
    }

    #[test]
    fn a_well_formed_record_yields_its_stems() {
        let stems = stems_from(
            r#"{"cases": [{"source_image": "firefox_abc.png"}, {"source_image": "x/firefox_def.png"}]}"#,
        )
        .expect("well formed");
        assert_eq!(
            stems,
            ["firefox_abc".to_string(), "firefox_def".to_string()]
                .into_iter()
                .collect()
        );
    }
}
