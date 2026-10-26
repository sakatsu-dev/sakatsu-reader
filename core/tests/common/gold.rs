//! Reading UD_Japanese-GSD, shared by the harnesses that score against it.
//!
//! Moved out of `ud_gold.rs` when a second harness needed the same treebank. Rust integration
//! tests are separate binaries and can only share through `mod common`, so this is the only place
//! it could go that does not mean two copies of a CoNLL-U reader drifting apart.
//!
//! Fetched by `scripts/fetch-ud-japanese.ps1`. CC BY-SA 4.0 — see `project notes` §8.

use std::path::PathBuf;

/// One hand-annotated token.
#[allow(dead_code)]
pub struct GoldToken {
    pub lemma: String,
    pub xpos: String,
    /// `LUWBILabel=B` — this token begins a new long unit.
    pub begins_long_unit: bool,
    /// Byte offsets into the sentence, to match `Token::start` / `Token::end`.
    pub start: usize,
    pub end: usize,
}

#[allow(dead_code)]
pub struct GoldSentence {
    /// The treebank's own `# sent_id`, e.g. `dev-s234`.
    ///
    /// **Carried so a case can be named by the corpus rather than by its position.** A run diff
    /// that says "case 417 moved" is a diff about this harness; one that says "dev-s234 moved" is
    /// a diff about the Japanese, and the id is stable across a corpus that grows. It is also what
    /// lets a pinned exception be matched by identity instead of by comparing whole sentences.
    pub id: String,
    pub text: String,
    pub tokens: Vec<GoldToken>,
}

/// Where the treebank was fetched to. `SAKATSU_UD_GSD` overrides.
#[allow(dead_code)]
pub fn gold_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SAKATSU_UD_GSD") {
        return PathBuf::from(dir);
    }
    super::dictionaries_dir()
        .parent()
        .map(|parent| parent.join("corpora").join("ud-japanese-gsd"))
        .unwrap_or_default()
}

/// Read one CoNLL-U file.
///
/// **Offsets are rebuilt by concatenating the forms**, not taken from the file, because CoNLL-U
/// does not record them. Japanese GSD marks `SpaceAfter=No` throughout, so the forms run together
/// into exactly the `# text` line — and where they do not, the sentence is dropped rather than
/// aligned against offsets that would silently be wrong by a character from there on.
#[allow(dead_code)]
pub fn read_conllu(path: &std::path::Path) -> Vec<GoldSentence> {
    let Ok(body) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut sentences = Vec::new();
    let mut id: Option<String> = None;
    let mut text: Option<String> = None;
    let mut tokens: Vec<GoldToken> = Vec::new();
    let mut cursor = 0usize;
    let mut misaligned = 0usize;

    for line in body.lines() {
        if line.is_empty() {
            if let Some(text) = text.take() {
                if cursor == text.len() && !tokens.is_empty() {
                    sentences.push(GoldSentence {
                        // A sentence with no `# sent_id` is named by its position, so a case id
                        // always exists — an unnamed case cannot be reconciled against a manifest.
                        id: id
                            .take()
                            .unwrap_or_else(|| format!("sentence-{}", sentences.len())),
                        text,
                        tokens: std::mem::take(&mut tokens),
                    });
                } else {
                    misaligned += 1;
                }
            }
            tokens.clear();
            id = None;
            cursor = 0;
            continue;
        }

        if let Some(rest) = line.strip_prefix("# sent_id = ") {
            id = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("# text = ") {
            text = Some(rest.to_string());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 10 {
            continue;
        }
        // A range id (`8-9`) is a contraction the treebank also spells out token by token, and an
        // empty id (`8.1`) is an elided node with no surface at all. Neither occupies text.
        if fields[0].contains('-') || fields[0].contains('.') {
            continue;
        }

        let form = fields[1];
        let start = cursor;
        cursor += form.len();
        tokens.push(GoldToken {
            lemma: fields[2].to_string(),
            xpos: fields[4].to_string(),
            begins_long_unit: fields[9].contains("LUWBILabel=B"),
            start,
            end: cursor,
        });
    }

    if misaligned > 0 {
        println!("  {misaligned} sentences dropped — forms did not rebuild the text line");
    }
    sentences
}

/// The long units, as spans, by folding `B`-started runs together.
#[allow(dead_code)]
pub fn long_units(tokens: &[GoldToken]) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for token in tokens {
        match spans.last_mut() {
            Some(last) if !token.begins_long_unit => last.1 = token.end,
            _ => spans.push((token.start, token.end)),
        }
    }
    spans
}

/// Both splits this project fetches, as one list. Empty when the treebank is not installed.
#[allow(dead_code)]
pub fn test_and_dev() -> Vec<GoldSentence> {
    let dir = gold_dir();
    let mut sentences = Vec::new();
    for split in ["test", "dev"] {
        sentences.extend(read_conllu(&dir.join(format!("ja_gsd-ud-{split}.conllu"))));
    }
    sentences
}
