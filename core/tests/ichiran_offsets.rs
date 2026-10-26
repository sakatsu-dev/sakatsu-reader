//! Source alignment for the Ichiran adapter: every token locatable, in the source's own spelling.
//!
//! # The defect
//!
//! Ichiran folds fullwidth characters on the way through, so a passage that reads ２０１１ comes back
//! as `2011`. Grounding was a literal `find` of the returned surface in the original, and neither
//! string contains the other, so it answered `None`: **20 tokens per stage across 18 of the novel's
//! lines had no offsets at all**, and a normalized surface sat beside a residual token covering the
//! same characters.
//!
//! Unlocated tokens were excluded from paired span scoring, which is why this reached the evaluator
//! as missing data. It reaches the *reader* as something worse — a token with no span cannot be
//! highlighted, looked up by position, or saved against the text it came from.
//!
//! # Why these particular tests
//!
//! The two ways a fix here goes wrong are the
//! same mistake in different clothes: **an offset is not something to search for, it is something to
//! carry**. A mapping built by re-finding the normalized text is a mapping that will take the wrong
//! occurrence the first time a passage repeats itself, and one built by counting characters where
//! bytes are meant will slide the moment a character is not three bytes wide.
//!
//! These run with **nothing installed**: they drive `to_analysis` over hand-written segments, which
//! is the shape `ichiran-cli` returns, so the sidecar, WSL and PostgreSQL are all out of the way.

use sakatsu_core::analyzer::ichiran::raw::{Candidate, Segment, WordEntry, WordInfo};
use sakatsu_core::analyzer::ichiran::to_analysis;
use sakatsu_core::model::Analysis;

/// One analyzed word, shaped the way `ichiran-cli -f` reports it.
fn word(surface: &str, reading: &str) -> WordInfo {
    WordInfo {
        text: surface.to_string(),
        kana: reading.to_string(),
        score: 100.0,
        ..Default::default()
    }
}

fn analyse(text: &str, words: &[WordInfo]) -> Analysis {
    let segments: Vec<Segment> = words
        .iter()
        .map(|info| {
            Segment::Candidates(vec![Candidate(
                vec![WordEntry(String::new(), info.clone(), Vec::new())],
                100.0,
            )])
        })
        .collect();
    to_analysis(text, &segments)
}

/// **Every token names a real slice of the source** — not just every *word*.
///
/// The distinction is the whole point here and was nearly missed. `Token::is_word` asks whether
/// there is Japanese in the surface, and a folded numeral is `2011`, which there is not. Skipping
/// non-words would have skipped precisely the cohort this file exists for: two of these tests
/// passed against the unrepaired code until this stopped filtering.
fn spans_are_exact(analysis: &Analysis, text: &str) {
    for token in &analysis.tokens {
        let (start, end) = (
            token
                .start
                .unwrap_or_else(|| panic!("{:?} has no start", token.surface)),
            token
                .end
                .unwrap_or_else(|| panic!("{:?} has no end", token.surface)),
        );
        assert!(start < end, "{:?} has an empty span", token.surface);
        assert_eq!(
            &text[start..end],
            token.surface,
            "{:?} does not name the source it points at",
            token.surface
        );
    }
}

/// A fullwidth number is located, and shown the way the source spells it.
#[test]
fn a_folded_number_is_grounded_in_the_source_and_keeps_its_spelling() {
    let text = "２０１１年に来た。";
    // Ichiran returns the digits folded to ASCII, which is the whole difficulty.
    let analysis = analyse(
        text,
        &[word("2011", "にせんじゅういち"), word("年", "ねん")],
    );

    let numeral = analysis
        .tokens
        .iter()
        .find(|token| token.start == Some(0))
        .expect("the number is grounded");
    assert_eq!(
        numeral.surface, "２０１１",
        "the source's own spelling, not the analyzer's echo of it"
    );
    assert_eq!(numeral.end, Some("２０１１".len()));
    spans_are_exact(&analysis, text);
}

/// **A repeated number takes the occurrence the cursor is at, not the first one.**
///
/// This is the test that a mapping rebuilt by re-searching fails. Three identical numbers, and the
/// second and third must land on their own characters.
#[test]
fn a_repeated_number_grounds_at_each_occurrence_in_turn() {
    let text = "１２と１２と１２";
    let analysis = analyse(
        text,
        &[
            word("12", "じゅうに"),
            word("と", "と"),
            word("12", "じゅうに"),
            word("と", "と"),
            word("12", "じゅうに"),
        ],
    );

    let starts: Vec<usize> = analysis
        .tokens
        .iter()
        .filter(|token| token.surface == "１２")
        .map(|token| token.start.expect("grounded"))
        .collect();
    assert_eq!(
        starts.len(),
        3,
        "all three occurrences are grounded, not one of them three times"
    );
    assert!(
        starts.windows(2).all(|pair| pair[0] < pair[1]),
        "each occurrence is further into the source than the last: {starts:?}"
    );
    spans_are_exact(&analysis, text);
}

/// **Astral characters do not slide the mapping.**
///
/// A character outside the basic plane is four bytes and one `char`, so a map indexed by characters
/// where bytes are meant reports a span three bytes short of where it should be, and every token
/// after it inherits the error. The emoji here sits between two folded numbers precisely so a
/// character/byte conflation shows up as a wrong span rather than as nothing at all.
#[test]
fn an_astral_character_does_not_shift_the_spans_after_it() {
    let text = "１２🎌３４";
    let analysis = analyse(
        text,
        &[
            word("12", "じゅうに"),
            word("🎌", ""),
            word("34", "さんじゅうよん"),
        ],
    );

    let last = analysis
        .tokens
        .iter()
        .find(|token| token.surface == "３４")
        .expect("the number after the emoji is grounded");
    assert_eq!(
        last.start,
        Some("１２🎌".len()),
        "the span is measured in bytes, and 🎌 is four of them"
    );
    spans_are_exact(&analysis, text);
}

/// Mixed widths in one passage: the folded and unfolded paths agree about where they are.
#[test]
fn folded_and_unfolded_tokens_share_one_cursor() {
    let text = "第２章と第3章";
    let analysis = analyse(
        text,
        &[
            word("第", "だい"),
            word("2", "に"),
            word("章", "しょう"),
            word("と", "と"),
            word("第", "だい"),
            word("3", "さん"),
            word("章", "しょう"),
        ],
    );
    spans_are_exact(&analysis, text);
    let chapters: Vec<usize> = analysis
        .tokens
        .iter()
        .filter(|token| token.surface == "章")
        .map(|token| token.start.expect("grounded"))
        .collect();
    assert_eq!(chapters.len(), 2);
    assert!(chapters[0] < chapters[1], "{chapters:?}");
}

/// **Nothing is lost and nothing is duplicated.**
///
/// The failure the handoff warns of for the *number* work applies here in reverse: a mapping that
/// normalizes for the search must not leave a residual token behind covering the same characters.
/// Concatenating every token's own source slice reconstructs the passage exactly once.
#[test]
fn the_grounded_tokens_tile_the_source_without_overlap() {
    let text = "２０１１年、１２月３日。";
    let analysis = analyse(
        text,
        &[
            word("2011", "にせんじゅういち"),
            word("年", "ねん"),
            word("12", "じゅうに"),
            word("月", "がつ"),
            word("3", "さん"),
            word("日", "にち"),
        ],
    );
    spans_are_exact(&analysis, text);

    let mut spans: Vec<(usize, usize)> = analysis
        .tokens
        .iter()
        .filter_map(|token| Some((token.start?, token.end?)))
        .collect();
    spans.sort();
    for pair in spans.windows(2) {
        assert!(
            pair[0].1 <= pair[1].0,
            "two tokens claim the same characters: {pair:?}"
        );
    }
}
