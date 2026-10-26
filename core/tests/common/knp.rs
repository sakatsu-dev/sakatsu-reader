//! Reading KNP-annotated Japanese, shared between harnesses.
//!
//! Extracted from `kwdlc_gold.rs` when a second evaluation needed the same annotations — the same
//! move `common/gold.rs` made for the treebank. The parsing is unchanged; **what changed is that
//! nothing is lost silently any more.**
//!
//! # Why the faults are the point
//!
//! The original reader answered three different situations with the same empty result:
//!
//! - a file that could not be read at all,
//! - a morpheme line with too few fields,
//! - morphemes left over at end of file with no `EOS` to close them.
//!
//! A caller then counted sentences and reported a denominator, with no way to tell a genuinely
//! small corpus from a corpus it had failed to read. That is the difference between "the analyzer
//! agreed on 80% of what we checked" and "the analyzer agreed on 80% of what we managed to load",
//! and only one of those is a measurement.
//!
//! So every one of them is now a [`Fault`] the caller receives beside the sentences and must
//! account for. The sentences themselves are identical for well-formed input, which is what lets
//! `kwdlc_gold.rs` adopt this without its published numbers moving.
//!
//! # Licence
//!
//! KWDLC declares none, so `project notes` §8 treats it as **consult, never import**. This module
//! reads it to measure. **No surface, reading or boundary from it may be persisted** into a
//! fixture, a pack, a dictionary or an artifact — a `Fault` names a path and a line number for
//! exactly that reason, and deliberately does not carry the annotation text of a well-formed line.

// Every integration test binary compiles `common/` whole, so a harness that does not
// touch this module leaves all of it unread. Same reason `context_retrieval.rs` carries
// this.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// One annotated morpheme, with the two phrase levels it begins.
#[derive(Debug)]
pub struct Morpheme {
    pub reading: String,
    /// 活用形 — 基本形, タ形, 未然形 and the rest. Empty (`*`) for uninflected words.
    pub conjugation_form: String,
    pub start: usize,
    pub end: usize,
    pub begins_kihonku: bool,
    pub begins_bunsetsu: bool,
}

#[derive(Debug)]
pub struct Sentence {
    pub text: String,
    pub morphemes: Vec<Morpheme>,
    /// The document this came from, by file stem.
    ///
    /// **Identity, not decoration.** A workload keyed on position — "the 41st sentence of the
    /// stride" — names a different sentence the moment the corpus, the stride or the reservation
    /// changes, and a resume keyed on that would reuse one measurement for another.
    pub document: String,
    /// Which sentence of that document, from zero.
    pub ordinal: usize,
}

impl Sentence {
    /// `document:ordinal` — stable under any sampling, and carrying no corpus text.
    pub fn id(&self) -> String {
        format!("{}:{}", self.document, self.ordinal)
    }
}

/// Something the corpus could not supply, kept so a denominator can be believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The directory could not be listed. Every file under it is missing, not absent.
    UnreadableDirectory { path: PathBuf, error: String },
    /// The file could not be read. **Not the same as a file with no sentences in it.**
    UnreadableFile { path: PathBuf, error: String },
    /// A directory entry could not be read.
    ///
    /// Was `filter_map(Result::ok)`: a file the walker could not stat simply did not exist as far
    /// as the corpus was concerned.
    UnreadableEntry { path: PathBuf, error: String },
    /// A morpheme line had too few fields to be one.
    ///
    /// The line's own text is **not** carried — see the licence note above. Its number is enough
    /// to find it, and finding it is a human's job with the corpus in hand.
    MalformedMorpheme {
        path: PathBuf,
        line_number: usize,
        fields: usize,
    },
    /// Morphemes were still open at end of file with no `EOS` to close them.
    ///
    /// They are dropped, as they always were — but now the loss is counted.
    UnterminatedSentence { path: PathBuf, morphemes: usize },
}

/// What one read produced, and what it could not.
#[derive(Default)]
pub struct Read {
    pub sentences: Vec<Sentence>,
    pub faults: Vec<Fault>,
}

impl Read {
    /// Fold another read into this one.
    pub fn absorb(&mut self, other: Read) {
        self.sentences.extend(other.sentences);
        self.faults.extend(other.faults);
    }
}

/// Read one `.knp` file.
///
/// The format is positional and, in this corpus, free of the alternative-analysis lines KNP can
/// otherwise emit: `#` opens a sentence, `*` and `+` open a 文節 and a 基本句, `EOS` closes, and
/// everything else is a morpheme whose first three fields are surface, reading and lemma. Some
/// lines carry a trailing `NIL` and some do not, which is why nothing reads from the end.
pub fn read_knp(path: &Path) -> Read {
    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) => {
            return Read {
                sentences: Vec::new(),
                faults: vec![Fault::UnreadableFile {
                    path: path.to_path_buf(),
                    error: error.to_string(),
                }],
            }
        }
    };

    let document = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut read = Read::default();
    let mut morphemes: Vec<Morpheme> = Vec::new();
    let mut text = String::new();
    let mut pending_kihonku = false;
    let mut pending_bunsetsu = false;

    for (index, line) in body.lines().enumerate() {
        if line == "EOS" {
            if !morphemes.is_empty() {
                let ordinal = read.sentences.len();
                read.sentences.push(Sentence {
                    text: std::mem::take(&mut text),
                    morphemes: std::mem::take(&mut morphemes),
                    document: document.clone(),
                    ordinal,
                });
            }
            text.clear();
            morphemes.clear();
            pending_kihonku = false;
            pending_bunsetsu = false;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with("* ") || line == "*" {
            pending_bunsetsu = true;
            pending_kihonku = true;
            continue;
        }
        if line.starts_with("+ ") || line == "+" {
            pending_kihonku = true;
            continue;
        }

        let fields: Vec<&str> = line.split(' ').collect();
        if fields.len() < 11 {
            // Was a silent `continue`. The morpheme is still skipped — changing that would change
            // the sentences — but the skip is now on the record.
            read.faults.push(Fault::MalformedMorpheme {
                path: path.to_path_buf(),
                line_number: index + 1,
                fields: fields.len(),
            });
            continue;
        }
        let surface = fields[0];
        let start = text.len();
        text.push_str(surface);
        morphemes.push(Morpheme {
            reading: fields[1].to_string(),
            conjugation_form: match fields[9] {
                "*" => String::new(),
                form => form.to_string(),
            },
            start,
            end: text.len(),
            begins_kihonku: std::mem::take(&mut pending_kihonku),
            begins_bunsetsu: std::mem::take(&mut pending_bunsetsu),
        });
    }

    if !morphemes.is_empty() {
        read.faults.push(Fault::UnterminatedSentence {
            path: path.to_path_buf(),
            morphemes: morphemes.len(),
        });
    }

    read
}

/// Every `.knp` file under `dir`, and what could not be listed.
pub fn collect(dir: &Path) -> (Vec<PathBuf>, Vec<Fault>) {
    let mut files = Vec::new();
    let mut faults = Vec::new();
    walk(dir, &mut files, &mut faults);
    files.sort();
    (files, faults)
}

fn walk(dir: &Path, into: &mut Vec<PathBuf>, faults: &mut Vec<Fault>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            faults.push(Fault::UnreadableDirectory {
                path: dir.to_path_buf(),
                error: error.to_string(),
            });
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                faults.push(Fault::UnreadableEntry {
                    path: dir.to_path_buf(),
                    error: error.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            walk(&path, into, faults);
        } else if path.extension().is_some_and(|ext| ext == "knp") {
            into.push(path);
        }
    }
}

/// Every sentence under `dir`, **or a refusal**.
///
/// # Why a fault is not simply reported
///
/// The parser can say what went wrong, but only a caller can decide what that means — and the
/// tempting decision is to print the fault and score what loaded. That is exactly the failure this
/// exists to prevent: a malformed morpheme is *skipped*, so the sentence containing it is scored as
/// **shortened text**, with every later offset in that sentence shifted. The analyzer is then
/// compared against gold spans for a sentence nobody analysed. The run finishes, the denominator is
/// a little smaller, and the report is green.
///
/// So loading is all-or-nothing. A caller that wants the faults instead can use [`read_knp`]
/// directly and say what it is doing.
pub fn load(dir: &Path) -> Result<Vec<Sentence>, Damaged> {
    let (files, faults) = collect(dir);
    load_files(&files, faults)
}

/// Load an **explicit** list of files, all-or-nothing.
///
/// For a caller holding back part of a corpus. Reserving documents means excluding them **before
/// their annotations are loaded**, not filtering results afterwards: a pool that was read and then
/// dropped has still been read, and only the caller can honestly say which files it intends to
/// touch.
pub fn load_files(files: &[PathBuf], mut faults: Vec<Fault>) -> Result<Vec<Sentence>, Damaged> {
    let mut sentences = Vec::new();
    for path in files {
        let read = read_knp(path);
        sentences.extend(read.sentences);
        faults.extend(read.faults);
    }
    if faults.is_empty() {
        Ok(sentences)
    } else {
        Err(Damaged {
            files: files.len(),
            sentences_loaded: sentences.len(),
            faults,
        })
    }
}

/// A corpus that could not be loaded intact.
#[derive(Debug)]
pub struct Damaged {
    pub files: usize,
    /// How many sentences *did* load — the size of the smaller denominator that was on offer.
    pub sentences_loaded: usize,
    pub faults: Vec<Fault>,
}

impl std::fmt::Display for Damaged {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for fault in &self.faults {
            *kinds
                .entry(match fault {
                    Fault::UnreadableDirectory { .. } => "unreadable directory",
                    Fault::UnreadableEntry { .. } => "unreadable directory entry",
                    Fault::UnreadableFile { .. } => "unreadable file",
                    Fault::MalformedMorpheme { .. } => "malformed morpheme line",
                    Fault::UnterminatedSentence { .. } => "unterminated sentence",
                })
                .or_default() += 1;
        }
        let summary: Vec<String> = kinds
            .into_iter()
            .map(|(kind, count)| format!("{count} {kind}"))
            .collect();
        write!(
            formatter,
            "the corpus did not load intact ({}); {} of {} files yielded {} sentences, and scoring              that smaller set would report a measurement of text nobody analysed",
            summary.join(", "),
            self.files,
            self.files,
            self.sentences_loaded
        )
    }
}

/// Fold morphemes into the spans of whichever phrase level `begins` marks.
pub fn spans_of(morphemes: &[Morpheme], begins: impl Fn(&Morpheme) -> bool) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for morpheme in morphemes {
        match spans.last_mut() {
            Some(last) if !begins(morpheme) => last.1 = morpheme.end,
            _ => spans.push((morpheme.start, morpheme.end)),
        }
    }
    spans
}

/// The three granularities, as predicates over a morpheme.
pub mod level {
    use super::Morpheme;

    /// JUMAN's own units, the finest level. Every morpheme begins one.
    pub fn morpheme(_: &Morpheme) -> bool {
        true
    }

    /// 基本句 — a content word plus its particles.
    pub fn kihonku(morpheme: &Morpheme) -> bool {
        morpheme.begins_kihonku
    }

    /// 文節 — the phrase a reader chunks by.
    pub fn bunsetsu(morpheme: &Morpheme) -> bool {
        morpheme.begins_bunsetsu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny well-formed document in KNP's shape. Invented for the test, not corpus text — the
    /// licence forbids carrying KWDLC's own lines into anything that stays here.
    fn document() -> String {
        [
            "# S-ID:test-1",
            "* 1D",
            "+ 1D",
            "本 ほん 本 名詞 6 普通名詞 1 * 0 * 0 NIL",
            "を を を 助詞 9 格助詞 1 * 0 * 0 NIL",
            "* -1D",
            "+ -1D",
            "読んで よんで 読む 動詞 2 * 0 子音動詞マ行 9 タ系連用テ形 14 NIL",
            "いる いる いる 接尾辞 14 動詞性接尾辞 7 母音動詞 1 基本形 2 NIL",
            "EOS",
        ]
        .join("\n")
    }

    fn write(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("sakatsu-knp-tests");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write");
        path
    }

    #[test]
    fn a_well_formed_document_reads_with_no_faults() {
        let read = read_knp(&write("well-formed.knp", &document()));
        assert!(read.faults.is_empty(), "{:?}", read.faults);
        assert_eq!(read.sentences.len(), 1);
        let sentence = &read.sentences[0];
        assert_eq!(sentence.text, "本を読んでいる");
        assert_eq!(sentence.morphemes.len(), 4);

        // The three granularities, exactly as the module header's table describes them.
        assert_eq!(spans_of(&sentence.morphemes, level::morpheme).len(), 4);
        assert_eq!(
            spans_of(&sentence.morphemes, level::kihonku),
            vec![(0, 6), (6, 21)]
        );
        assert_eq!(
            spans_of(&sentence.morphemes, level::bunsetsu),
            vec![(0, 6), (6, 21)]
        );
    }

    /// **An unreadable file is not an empty one.**
    ///
    /// This is the hole that mattered most: a corpus that failed to load produced a smaller
    /// denominator and no complaint, so a measurement could quietly describe a fraction of what it
    /// claimed to cover.
    #[test]
    fn an_unreadable_file_is_a_fault_not_an_absence() {
        let missing = std::env::temp_dir().join("sakatsu-knp-tests/does-not-exist.knp");
        let _ = std::fs::remove_file(&missing);
        let read = read_knp(&missing);
        assert!(read.sentences.is_empty());
        assert!(
            matches!(read.faults.as_slice(), [Fault::UnreadableFile { .. }]),
            "{:?}",
            read.faults
        );

        // And a file that really is empty is **not** a fault — the two must stay distinguishable.
        let empty = read_knp(&write("empty.knp", ""));
        assert!(empty.sentences.is_empty());
        assert!(empty.faults.is_empty(), "an empty file is not a failure");
    }

    /// A morpheme line too short to be one is skipped, as before — and counted, which is new.
    #[test]
    fn a_malformed_morpheme_is_counted_rather_than_dropped() {
        let body = document().replace("を を を 助詞 9 格助詞 1 * 0 * 0 NIL", "を を を 助詞");
        let read = read_knp(&write("malformed.knp", &body));
        assert_eq!(read.sentences.len(), 1);
        assert_eq!(
            read.sentences[0].morphemes.len(),
            3,
            "the short line is still skipped, so sentences do not change"
        );
        assert!(
            matches!(
                read.faults.as_slice(),
                [Fault::MalformedMorpheme {
                    line_number: 5,
                    fields: 4,
                    ..
                }]
            ),
            "{:?}",
            read.faults
        );
    }

    /// Morphemes left open at end of file are dropped, as before — and counted.
    #[test]
    fn an_unterminated_sentence_is_counted() {
        let body = document().replace("\nEOS", "");
        let read = read_knp(&write("unterminated.knp", &body));
        assert!(read.sentences.is_empty(), "no EOS, so no sentence");
        assert!(
            matches!(
                read.faults.as_slice(),
                [Fault::UnterminatedSentence { morphemes: 4, .. }]
            ),
            "{:?}",
            read.faults
        );
    }

    /// A directory that cannot be listed is a fault, not an empty corpus.
    #[test]
    fn an_unlistable_directory_is_a_fault() {
        let missing = std::env::temp_dir().join("sakatsu-knp-tests/no-such-directory");
        let _ = std::fs::remove_dir_all(&missing);
        let (files, faults) = collect(&missing);
        assert!(files.is_empty());
        assert!(
            matches!(faults.as_slice(), [Fault::UnreadableDirectory { .. }]),
            "{faults:?}"
        );
    }

    /// **The loading path a measurement uses refuses a damaged corpus**, rather than handing back
    /// a smaller one.
    ///
    /// Asserted through `load` — the function `kwdlc_gold` and `hybrid_reference` actually call —
    /// not through `read_knp`. A parser that reports a fault its caller then prints and scores past
    /// has not prevented anything, and testing only the parser would have looked exactly the same.
    #[test]
    fn the_loading_path_refuses_a_damaged_corpus() {
        let dir = std::env::temp_dir().join("sakatsu-knp-tests/damaged-corpus");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("corpus dir");
        std::fs::write(dir.join("clean.knp"), document()).expect("write");

        // Intact corpus: loads, and the sentence count is what a measurement would use.
        let loaded = load(&dir).expect("a clean corpus loads");
        assert_eq!(loaded.len(), 1);

        // Now damage one file. The other file still parses, so the tempting behaviour is to score
        // what loaded — which is precisely what must not happen.
        let damaged = document().replace("を を を 助詞 9 格助詞 1 * 0 * 0 NIL", "を を を 助詞");
        std::fs::write(dir.join("damaged.knp"), damaged).expect("write");

        let refusal = load(&dir).expect_err("a damaged corpus must refuse");
        assert_eq!(refusal.files, 2);
        assert_eq!(
            refusal.sentences_loaded, 2,
            "two sentences were parseable — that is the smaller denominator on offer"
        );
        let said = refusal.to_string();
        assert!(said.contains("malformed morpheme line"), "{said}");
        assert!(
            said.contains("nobody analysed"),
            "the refusal says why a smaller denominator is not an option: {said}"
        );
    }

    /// An absent corpus is a refusal too, and a different one from a damaged corpus.
    #[test]
    fn the_loading_path_distinguishes_absent_from_damaged() {
        let missing = std::env::temp_dir().join("sakatsu-knp-tests/not-a-corpus");
        let _ = std::fs::remove_dir_all(&missing);
        let refusal = load(&missing).expect_err("an unlistable directory refuses");
        assert!(
            refusal.to_string().contains("unreadable directory"),
            "{refusal}"
        );
        assert_eq!(refusal.sentences_loaded, 0);
    }

    /// The faults carry no annotation text, because the licence does not allow keeping it.
    #[test]
    fn a_fault_names_a_place_and_not_the_corpus_text() {
        let body = document().replace("を を を 助詞 9 格助詞 1 * 0 * 0 NIL", "を を を 助詞");
        let read = read_knp(&write("licence.knp", &body));
        let rendered = format!("{:?}", read.faults);
        assert!(
            !rendered.contains('を') && !rendered.contains("助詞"),
            "a fault must not carry the corpus line: {rendered}"
        );
    }
}
