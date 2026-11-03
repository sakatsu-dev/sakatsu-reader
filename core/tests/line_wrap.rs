//! **A word a text box wraps is one word to the reader.**
//!
//! PaddleOCR reports one line per box it detects, and a game's text box wraps where its width runs
//! out, inside a word as often as not. Both analyzers cut at a newline, so the reader was handed
//! the pieces: 払拭 wrapped as 払 / 拭 was read as two pieces, neither of them the word.
//! `lines::rejoin_split_words` takes such a break out before any analyzer reads the passage.
//!
//! These run the production path with the built-in analyzer, which needs nothing installed, and
//! with a recording engine that shows what every engine — Ichiran included — is handed.
//!
//! Sentences from Tatoeba (CC BY 2.0 FR), wrapped where a text box might wrap them.

use std::sync::Mutex;

use sakatsu_core::analyzer::{AnalyzeError, Analyzer, LinderaAnalyzer};
use sakatsu_core::model::{Analysis, AnalyzerPolicy};
use sakatsu_core::study::{self, AnalysisOptions, Plan};

/// #176705 "The crowd calmed down.", wrapped inside 群衆.
const INSIDE_A_WORD: &str = "群\n衆は静かになった。";
/// The same sentence, wrapped between two words.
const BETWEEN_WORDS: &str = "群衆は\n静かになった。";

fn read(text: &str) -> Analysis {
    let lindera = LinderaAnalyzer::new().expect("the embedded dictionary loads");
    study::analyse(text, &lindera, &[]).expect("analyses")
}

fn surfaces(analysis: &Analysis) -> Vec<&str> {
    analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect()
}

#[test]
fn a_word_wrapped_across_two_lines_is_read_as_one_word() {
    let analysis = read(INSIDE_A_WORD);
    assert_eq!(analysis.text, "群衆は静かになった。");
    assert!(
        surfaces(&analysis).contains(&"群衆"),
        "{:?}",
        surfaces(&analysis)
    );
}

/// #3555687 "I'll come back. I promise.", wrapped inside 約束: the line before the break is not
/// the passage's first.
#[test]
fn a_word_wrapped_in_a_later_sentence_is_rejoined_too() {
    let analysis = read("必ず戻る。約\n束する。");
    assert_eq!(analysis.text, "必ず戻る。約束する。");
    assert!(
        analysis
            .tokens
            .iter()
            .any(|token| token.surface.starts_with("約束")),
        "{:?}",
        surfaces(&analysis)
    );
}

/// The break splits nothing, so it stays for the line marking to classify and the reader to show.
#[test]
fn a_break_between_two_words_is_kept() {
    let analysis = read(BETWEEN_WORDS);
    assert_eq!(analysis.text, BETWEEN_WORDS);
}

/// #8844982 "Is this jasmine tea?" above #9178141 "I prefer going out to staying at home.", the
/// question mark dropped as a capture of unpunctuated text drops it. Joined, the built-in analyzer
/// groups ティー and 家 into one token, but each word is whole on its own line: two sentences, and
/// the break stays.
#[test]
fn two_whole_words_the_analyzer_would_group_are_not_welded() {
    let text = "これってジャスミンティー\n家にいるより、外に出かける方が好きなんだ。";
    assert_eq!(read(text).text, text);
}

/// Text that is not Japanese is never joined: the parser reads two English words on two lines as
/// one unknown run, and joining them would weld them.
#[test]
fn a_break_between_english_words_is_kept() {
    let text = "Walking is the best\nexercise.";
    assert_eq!(read(text).text, text);
}

/// An engine that records each passage it is handed, and reads it with the built-in analyzer.
struct Recording {
    name: &'static str,
    fails: bool,
    inner: LinderaAnalyzer,
    seen: Mutex<Vec<String>>,
}

impl Recording {
    fn new(name: &'static str, fails: bool) -> Self {
        Recording {
            name,
            fails,
            inner: LinderaAnalyzer::new().expect("the embedded dictionary loads"),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().expect("seen").clone()
    }
}

impl Analyzer for Recording {
    fn analyze(&self, text: &str) -> Result<Analysis, AnalyzeError> {
        self.seen.lock().expect("seen").push(text.to_string());
        if self.fails {
            return Err(AnalyzeError::EngineUnavailable("wsl is not running".into()));
        }
        self.inner.analyze(text)
    }

    fn engine(&self) -> &str {
        self.name
    }
}

#[test]
fn every_engine_is_handed_the_rejoined_passage() {
    let engine = Recording::new("recording", false);
    study::analyse(INSIDE_A_WORD, &engine, &[]).expect("analyses");
    assert_eq!(engine.seen(), ["群衆は静かになった。"]);
}

/// The reader's own request: the first attempt and the retry read the same, rejoined passage.
#[test]
fn the_retry_reads_the_same_rejoined_passage() {
    let primary = Recording::new("ichiran", true);
    let fallback = Recording::new("lindera", false);
    let plan = Plan {
        requested: AnalyzerPolicy::Automatic,
        primary: &primary,
        fallback: Some(&fallback),
        standing_in: None,
    };
    let done = study::analyse_resilient(
        INSIDE_A_WORD,
        &plan,
        &[],
        AnalysisOptions::PRODUCTION,
        &|| true,
    )
    .expect("the fallback answers");
    assert_eq!(primary.seen(), ["群衆は静かになった。"]);
    assert_eq!(fallback.seen(), ["群衆は静かになった。"]);
    assert_eq!(done.analysis.text, "群衆は静かになった。");
}
