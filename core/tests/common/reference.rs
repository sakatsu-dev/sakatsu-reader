//! What more than one reference harness must agree on, kept in one place so it cannot drift.
//!
//! **The KWDLC reservation above all.** Two harnesses scoring the corpus with two copies of the rule
//! for what is held back would, the first time one copy changed, score the pool the other withholds
//! — and nothing would error.

#![allow(dead_code)]

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

/// The documents held back, read from the exposure audit rather than restated here.
///
/// **One place decides what is reserved.** A list copied into a harness is a list that drifts from
/// the audit that justified it, and the justification is the part that matters: these files are
/// *not* unseen, they are the `id/test.id` entries the default sampling does not score.
pub fn reserved_documents(evaluation: &std::path::Path) -> (BTreeSet<String>, String) {
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    if let Ok(entries) = std::fs::read_dir(evaluation) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !name.starts_with("kwdlc-exposure-audit-") || !name.ends_with(".json") {
                continue;
            }
            let when = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if newest.as_ref().is_none_or(|(latest, _)| when > *latest) {
                newest = Some((when, path));
            }
        }
    }
    let Some((_, path)) = newest else {
        return (BTreeSet::new(), "no exposure audit found".to_string());
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return (
            BTreeSet::new(),
            format!("{} could not be read", path.display()),
        );
    };
    let Ok(document) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return (BTreeSet::new(), format!("{} is not JSON", path.display()));
    };
    let files: BTreeSet<String> = document["reserved_pool"]["files"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    (
        files,
        format!(
            "{} ({})",
            path.file_name().unwrap_or_default().to_string_lossy(),
            sakatsu_core::hashing::hex(Sha256::digest(raw.as_bytes()))
        ),
    )
}

/// **Diagnostic only:** each segmentation Ichiran returns for `text` under `-f -l limit`, converted
/// by the adapter's own conversion — tie-break and line marking included.
///
/// Segmentation `k` takes the `k`-th candidate of every analysed segment, or its last where it
/// returned fewer. A span lies inside one segment, so every candidate that segment returned is
/// examined by some `k`.
///
/// **This establishes presence only.** Something found in one of these was among what Ichiran
/// returned; something not found was not observed within `limit`, which says nothing about whether
/// Ichiran could produce it. `Err` is a failed diagnostic, a third outcome.
pub fn segmentations(
    ichiran: &sakatsu_core::analyzer::IchiranAnalyzer,
    text: &str,
    limit: usize,
) -> Result<Vec<sakatsu_core::model::Analysis>, String> {
    use sakatsu_core::analyzer::ichiran::{self, raw};
    let json = ichiran
        .segmentations(text, limit)
        .map_err(|error| error.to_string())?;
    let segments = raw::parse(&json).map_err(|error| format!("unparseable: {error}"))?;
    let returned = segments
        .iter()
        .filter_map(|segment| match segment {
            raw::Segment::Candidates(candidates) => Some(candidates.len()),
            raw::Segment::Unanalyzed(_) => None,
        })
        .max()
        .unwrap_or(0);
    let source = sakatsu_core::analyzer::lines::normalise_breaks(text);
    let source = source.trim();
    Ok((0..returned)
        .map(|rank| {
            let chosen: Vec<raw::Segment> = segments
                .iter()
                .map(|segment| match segment {
                    raw::Segment::Candidates(candidates) if !candidates.is_empty() => {
                        raw::Segment::Candidates(vec![
                            candidates[rank.min(candidates.len() - 1)].clone()
                        ])
                    }
                    other => other.clone(),
                })
                .collect();
            ichiran::to_analysis(source, &chosen)
        })
        .collect())
}

/// The running test executable's **full** identity: its complete SHA-256 and its byte length.
///
/// `runner::executable_digest` is abbreviated for people reading a report — sixteen hex characters
/// and a rounded size — which identifies a build to a reader and is not an identity. An artifact
/// or a resume key needs this one.
pub fn executable_identity() -> String {
    let Ok(path) = std::env::current_exe() else {
        return "unknown (no current_exe)".to_string();
    };
    match std::fs::read(&path) {
        Ok(bytes) => format!(
            "{}:{}",
            sakatsu_core::hashing::hex(Sha256::digest(&bytes)),
            bytes.len()
        ),
        Err(error) => format!("unknown (unreadable: {error})"),
    }
}

/// Publish `contents` as `<stem>-<content digest>.<extension>` in `dir`, **atomically and without
/// ever destroying evidence**.
///
/// Written to a staging file and renamed, so an interrupted run cannot leave a half-written file
/// wearing a finished one's name. The name derives from the content, so an existing file of that
/// name should hold the same bytes; it is checked rather than trusted, and a mismatch is written
/// beside it under a longer digest instead of replacing it.
pub fn publish(
    dir: &std::path::Path,
    stem: &str,
    extension: &str,
    contents: &str,
) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("create {}: {error}", dir.display()))?;
    let digest = sakatsu_core::hashing::hex(Sha256::digest(contents.as_bytes()));
    let mut path = dir.join(format!("{stem}-{}.{extension}", &digest[..16]));
    if path.exists() {
        let existing = std::fs::read(&path)
            .map_err(|error| format!("read the existing {}: {error}", path.display()))?;
        if existing == contents.as_bytes() {
            return Ok(path);
        }
        path = dir.join(format!("{stem}-{digest}.{extension}"));
    }
    let staging = dir.join(format!(".{stem}-{}.partial", &digest[..16]));
    std::fs::write(&staging, contents)
        .map_err(|error| format!("write {}: {error}", staging.display()))?;
    std::fs::rename(&staging, &path)
        .map_err(|error| format!("publish {}: {error}", path.display()))?;
    Ok(path)
}

/// One word of **Ichiran's own output** — its best segmentation — before the adapter touches it.
///
/// Grounded by a forward search of the word's own `text` in the source, which is independent of
/// the adapter's grounding: the point is to see what the engine said, so attribution can separate
/// "the engine segmented it this way" from "the conversion made it this way". A word whose text is
/// not found verbatim (Ichiran rewrote its width or punctuation) has no span, and says so.
#[derive(Debug, Clone, PartialEq)]
pub struct RawWord {
    pub start: Option<usize>,
    pub end: Option<usize>,
    pub text: String,
    /// The surface reading Ichiran chose, where the word is not ambiguous.
    pub kana: String,
    pub score: f64,
    /// For an ambiguous surface, every competing entry's reading and score, as Ichiran ranked them.
    pub alternatives: Vec<(String, f64)>,
    /// The parts Ichiran names for a compound, as it spells them — which may be expanded forms.
    pub compound: Vec<String>,
}

impl RawWord {
    /// Whether this word's own span covers `start..end`.
    pub fn covers(&self, start: usize, end: usize) -> bool {
        matches!((self.start, self.end), (Some(from), Some(to)) if from <= start && end <= to)
    }

    /// The readings Ichiran scored highest for this word — more than one exactly when it tied.
    pub fn top_readings(&self) -> Vec<&str> {
        if self.alternatives.is_empty() {
            return vec![self.kana.as_str()];
        }
        let best = self
            .alternatives
            .iter()
            .map(|(_, score)| *score)
            .fold(f64::MIN, f64::max);
        self.alternatives
            .iter()
            .filter(|(_, score)| *score == best)
            .map(|(kana, _)| kana.as_str())
            .collect()
    }
}

/// Ichiran's best segmentation of `source`, word by word, from its raw JSON.
pub fn raw_words(json: &str, source: &str) -> Result<Vec<RawWord>, String> {
    use sakatsu_core::analyzer::ichiran::raw;
    let segments = raw::parse(json).map_err(|error| format!("unparseable: {error}"))?;
    let hiragana =
        |text: &str| sakatsu_core::kana::to_hiragana(text.trim_start_matches('\u{200c}'));
    let mut words = Vec::new();
    let mut cursor = 0usize;
    for segment in &segments {
        let raw::Segment::Candidates(candidates) = segment else {
            continue;
        };
        let Some(best) = candidates.first() else {
            continue;
        };
        for entry in &best.0 {
            let info = &entry.1;
            let (text, kana, score, alternatives) = if info.alternative.is_empty() {
                (
                    info.text.clone(),
                    hiragana(&info.kana),
                    info.score,
                    Vec::new(),
                )
            } else {
                let alternatives: Vec<(String, f64)> = info
                    .alternative
                    .iter()
                    .map(|alternative| (hiragana(&alternative.kana), alternative.score))
                    .collect();
                let best = alternatives
                    .iter()
                    .map(|(_, score)| *score)
                    .fold(f64::MIN, f64::max);
                (
                    info.alternative
                        .first()
                        .map(|alternative| alternative.text.clone())
                        .unwrap_or_default(),
                    String::new(),
                    best,
                    alternatives,
                )
            };
            let (start, end) = match source.get(cursor..).and_then(|rest| rest.find(&text)) {
                Some(offset) if !text.is_empty() => {
                    let start = cursor + offset;
                    cursor = start + text.len();
                    (Some(start), Some(start + text.len()))
                }
                _ => (None, None),
            };
            words.push(RawWord {
                start,
                end,
                text,
                kana,
                score,
                alternatives,
                compound: info.compound.clone(),
            });
        }
    }
    Ok(words)
}

/// How many parts, at any depth, the reader is **not** offered.
pub fn unselectable_parts(analysis: &sakatsu_core::model::Analysis) -> usize {
    fn count(parts: &[sakatsu_core::model::Token]) -> usize {
        parts
            .iter()
            .map(|part| usize::from(!part.selectable) + count(&part.components))
            .sum()
    }
    analysis
        .tokens
        .iter()
        .map(|token| count(&token.components))
        .sum()
}
