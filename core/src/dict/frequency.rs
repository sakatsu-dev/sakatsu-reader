//! How common a word actually is, from a corpus that counted.
//!
//! # Why this is not in the dictionary
//!
//! Jitendex cannot order synonyms and does not claim to. Measured against the installed
//! copy: 22,553 of its 433,885 entries share the maximum `score` — 食べる and 喫する and
//! 食う among them — and the format carries no frequency list and no priority tags to break
//! that tie. There is a commonness mark (`★`, on 29,746 entries) but it is a flag, not an
//! order. So `dict::index` says an English search answers with *several words that mean
//! this*, and it is right to.
//!
//! Frequency is a separate dictionary, in Yomitan's `term_meta_bank_*.json` format — a file
//! `yomitan.rs` has always named and never read.
//!
//! # There is no single answer, so the corpus is named
//!
//! BCCWJ counts newspapers, white papers, textbooks and blogs. JPDB counts anime, visual
//! novels and novels. Innocent Corpus counts about five thousand light novels. For someone
//! reading manga those three disagree, and none of them is wrong — they counted different
//! Japanese. So whatever ranks the results is named where the ranking is shown, rather than
//! presented as how common a word *is*.
//!
//! # Rank or count, and why it is declared rather than guessed
//!
//! A **rank** list gives 1 to the most common word: smaller is commoner. A **count** list
//! gives the largest number to the most common: larger is commoner. Nothing about a bare
//! integer says which it is, and reading one as the other produces a confident ordering that
//! is exactly backwards — 食べる last, some hapax first, with every part of the machinery
//! working perfectly.
//!
//! So [`Direction`] is declared by whoever installs the list, not inferred. The three the
//! app offers declare it in the catalog; a list imported by hand is assumed to be a rank,
//! which is what nearly all of them are, and the interface says that is what it assumed.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Which way the numbers run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// 1 is the most common word. Nearly every published list is one of these.
    #[default]
    Rank,
    /// The largest number is the most common — raw occurrences in the corpus.
    Count,
}

/// The direction a list **the app fetches itself** is known to use.
///
/// `None` for anything else, where the reader's setting is the only information there is.
///
/// This exists because the setting is global and the consequence of it being wrong is not
/// subtle: both offered lists are ranks, so reading one as counts negates every value and
/// orders the results exactly backwards - a search for "feeling" led with the rarest word in
/// the dictionary instead of the commonest. The reader has no way to know which kind a list is,
/// and for these two nobody needs to: the app chose them, downloaded them and unpacked them.
///
/// Keep in step with the `frequency-*` entries in [`crate::setup::catalog`]; a test holds the
/// two together so a third list cannot be offered without a decision being made about it.
pub fn shipped_direction(source: &str) -> Option<Direction> {
    match source {
        "jpdb" | "bccwj" => Some(Direction::Rank),
        _ => None,
    }
}

/// One corpus's answer about how common each word is.
#[derive(Debug, Clone, Default)]
pub struct Frequencies {
    /// What to call the corpus when the interface says where an order came from.
    pub name: String,
    pub direction: Direction,
    /// Keyed by written form. A reading-specific row also lands under `term\u{1}reading`,
    /// so a lookup that knows the reading can be exact and one that does not still works.
    values: HashMap<String, f64>,
}

/// Joins a term to a reading in the key. A control character, because it cannot occur in
/// either half — a plain separator would be ambiguous the first time a term contained it.
const WITH_READING: char = '\u{1}';

impl Frequencies {
    pub fn new(name: impl Into<String>, direction: Direction) -> Self {
        Frequencies {
            name: name.into(),
            direction,
            values: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Record one row, keeping the commoner value when a term appears twice.
    ///
    /// Duplicates are normal: a list may carry a term once bare and again per reading, and
    /// some carry the same word from several sub-corpora. Keeping the commoner reading of
    /// the two is the answer that matches what the reader is most likely to have met.
    pub fn insert(&mut self, term: &str, reading: Option<&str>, value: f64) {
        let key = match reading {
            Some(reading) if !reading.is_empty() => format!("{term}{WITH_READING}{reading}"),
            _ => term.to_string(),
        };
        self.keep_commoner(key, value);
        // Also under the bare term, so a lookup without a reading finds something.
        if reading.is_some() {
            self.keep_commoner(term.to_string(), value);
        }
    }

    fn keep_commoner(&mut self, key: String, value: f64) {
        let better = match self.values.get(&key) {
            Some(existing) => match self.direction {
                Direction::Rank => value < *existing,
                Direction::Count => value > *existing,
            },
            None => true,
        };
        if better {
            self.values.insert(key, value);
        }
    }

    /// Where this word sorts. Smaller is commoner, whichever way the source counted.
    ///
    /// A count is negated rather than reciprocated so the two directions share one
    /// comparison and one ordering, and so nothing has to divide by a zero count.
    ///
    /// `None` means the corpus does not have the word, which is not the same as rare — a
    /// list that stops at 30,000 words says nothing at all about the 30,001st. Callers sort
    /// those last, together, in whatever order they already had.
    pub fn rank(&self, term: &str, reading: &str) -> Option<f64> {
        let exact = (!reading.is_empty())
            .then(|| self.values.get(&format!("{term}{WITH_READING}{reading}")))
            .flatten();
        let value = exact.or_else(|| self.values.get(term))?;
        Some(match self.direction {
            Direction::Rank => *value,
            Direction::Count => -*value,
        })
    }
}

/// Unpack a Yomitan frequency zip into a directory [`Frequencies::load`] can read.
///
/// Only the two files that matter — `index.json` for the corpus's own name, and the
/// `term_meta_bank_*.json` banks. A frequency dictionary may also carry `term_bank` files
/// it inherited from whatever it was built from, and those are a word dictionary's job;
/// importing them here would put a second copy of the same definitions in front of the
/// reader under a name that means "how common", which is not what was asked for.
///
/// Paths from the archive are taken by file name only. A zip can name an entry
/// `../../anything`, and joining that onto a destination writes wherever it likes.
pub fn unpack(zip_path: &std::path::Path, into: &std::path::Path) -> std::io::Result<usize> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::create_dir_all(into)?;

    let mut written = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let Some(name) = entry
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            continue;
        };
        let wanted =
            name == "index.json" || (name.starts_with("term_meta_bank") && name.ends_with(".json"));
        if !wanted || entry.is_dir() {
            continue;
        }
        let mut out = std::fs::File::create(into.join(&name))?;
        std::io::copy(&mut entry, &mut out)?;
        written += 1;
    }
    Ok(written)
}

/// Whether an unpacked frequency list is sitting in `dir`.
///
/// **The same test [`Frequencies::load`] applies**, and deliberately so. The setup screen has to
/// answer "is this already installed" without paying to parse thousands of entries, and when it
/// answered with its own rule the two disagreed: the frequency lists were simply missing from the
/// setup screen's match arm, so they reported not-installed for ever and offered the same download
/// again however many times it had already succeeded.
///
/// A `term_meta_bank` is the file that carries the counts. `index.json` alone is not enough: it is
/// only the list's name, and a directory holding nothing else ranks nothing.
pub fn is_present(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("term_meta_bank") && name.ends_with(".json"))
        })
    })
}

impl Frequencies {
    /// Load every `term_meta_bank_*.json` in an extracted dictionary directory.
    ///
    /// A frequency dictionary is a Yomitan zip like any other, so nothing new is needed to
    /// get it onto disk — the fetch script unpacks it and this reads what is there. A bank
    /// that will not parse is skipped with a warning rather than taking the rest down: a
    /// list is thousands of words and losing one file of it is much better than losing all
    /// of it, and the caveat still names the corpus honestly either way.
    ///
    /// The name comes from the dictionary's own `index.json` when it has one, because that
    /// is what the corpus calls itself, and the interface has to print something a reader
    /// can recognise.
    pub fn load(dir: &std::path::Path, direction: Direction) -> Frequencies {
        let name = std::fs::read_to_string(dir.join("index.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|index| index.get("title")?.as_str().map(str::to_string))
            .unwrap_or_else(|| {
                dir.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("frequency list")
                    .to_string()
            });

        let mut into = Frequencies::new(name, direction);
        let Ok(entries) = std::fs::read_dir(dir) else {
            return into;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_bank = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("term_meta_bank") && n.ends_with(".json"));
            if !is_bank {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    if let Err(error) = read_meta_bank(&text, &mut into) {
                        crate::log_warn!("dict", "{}: {error}", path.display());
                    }
                }
                Err(error) => crate::log_warn!("dict", "{}: {error}", path.display()),
            }
        }
        into
    }
}

/// Read one `term_meta_bank_N.json` into `into`.
///
/// Rows are `[term, mode, data]`. Only `freq` is of interest — a list may also carry
/// `pitch` and `ipa`, which are somebody else's feature — and a row whose shape is not
/// recognised is skipped rather than guessed at.
///
/// Yomitan is permissive about `data`, so all of these appear in the wild:
///
/// ```text
/// ["食べる", "freq", 384]
/// ["食べる", "freq", "384"]
/// ["食べる", "freq", {"value": 384, "displayValue": "384ゕ"}]
/// ["食べる", "freq", {"reading": "たべる", "frequency": 384}]
/// ["食べる", "freq", {"reading": "たべる", "frequency": {"value": 384}}]
/// ```
pub fn read_meta_bank(json: &str, into: &mut Frequencies) -> Result<usize, serde_json::Error> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(json)?;
    let mut taken = 0;

    for row in rows {
        let Some(fields) = row.as_array() else {
            continue;
        };
        let (Some(term), Some(mode)) = (
            fields.first().and_then(|v| v.as_str()),
            fields.get(1).and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        if mode != "freq" || term.is_empty() {
            continue;
        }
        let Some(data) = fields.get(2) else { continue };

        // `{reading, frequency}` names a reading; everything else is about the term alone.
        let (reading, payload) = match data.get("reading").and_then(|r| r.as_str()) {
            Some(reading) => (Some(reading), data.get("frequency").unwrap_or(data)),
            None => (None, data),
        };

        if let Some(value) = number_in(payload) {
            into.insert(term, reading, value);
            taken += 1;
        }
    }

    Ok(taken)
}

/// The number inside whichever wrapper this source chose.
fn number_in(value: &serde_json::Value) -> Option<f64> {
    if let Some(number) = value.as_f64() {
        return Some(number);
    }
    if let Some(text) = value.as_str() {
        // "384" and "384ゕ" both occur — the mark is a corpus badge, not part of the
        // number, so the digits are taken from the front and the rest ignored.
        let digits: String = text
            .trim()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        return digits.parse().ok();
    }
    // `{value, displayValue}`, and the nested `{frequency: {value}}` case.
    value
        .get("value")
        .and_then(number_in)
        .or_else(|| value.get("frequency").and_then(number_in))
}

#[cfg(test)]
mod tests;
