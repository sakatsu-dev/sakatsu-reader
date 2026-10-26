//! A line break in a capture is a sentence boundary, unless the line was wrapped.
//!
//! # Why this file exists
//!
//! `model::ends_sentence` has always listed `'\n'` among its terminators, and it has never once
//! fired. Neither analyzer puts a newline into a token:
//!
//! - Lindera cuts its lattice at every `\n` *before* Viterbi, then drops the whitespace token —
//!   `Segmenter::new` leaves `keep_whitespace` false and the embedded dictionary classifies
//!   `0x000A` as whitespace.
//! - Ichiran returns the break as its own unanalyzed segment, and
//!   `analyzer::ichiran::convert` skips whitespace-only segments.
//!
//! So the rule was dead, and a punctuation-free capture — lyrics, subtitles, most game script —
//! arrived as **one** sentence however many lines it had. That is what put a whole passage on one
//! Anki card, welded the aligned translation into a single run-on row, and let a comparison
//! selection span six lines.
//!
//! # The other half, which is why this is not simply "a newline ends a sentence"
//!
//! PP-OCR emits one line per detected box, and for vertical Japanese a *column* is a box. One
//! sentence down three columns of a manga bubble arrives as three lines. Splitting on every break
//! would shatter it — worse than the bug being fixed. `lines::mark_line_breaks` asks the parser
//! which kind of break it is.
//!
//! ```text
//! cargo test -p sakatsu-core --test sentence_lines -- --nocapture
//! ```

use sakatsu_core::analyzer::{Analyzer, IchiranAnalyzer, LinderaAnalyzer};
use sakatsu_core::model::{sentence_ranges, Analysis};

/// Two complete utterances with no punctuation between them — the shape a lyric or subtitle
/// capture has, and the case that used to collapse into one sentence.
const TWO_LINES: &str = "優等生だね君って\n心配性だね君って";

/// One sentence broken mid-word, the way a panel's edge or a vertical column breaks it. The join
/// falls inside `宿泊させて欲しい`, so the parser reads straight through it.
const WRAPPED: &str = "６人宿泊させ\nて欲しい";

/// Nothing carries the break itself. Asserted rather than assumed, because every rule downstream
/// depends on it and it is invisible from the outside.
fn no_token_holds_the_break(analysis: &Analysis, what: &str) {
    for token in &analysis.tokens {
        assert!(
            !token.surface.contains('\n') && !token.surface.contains('\r'),
            "{what}: a token carries the break itself ({:?}) — the whole design assumes none does",
            token.surface
        );
    }
}

/// Windows clipboard text is CRLF, and the former IPADIC path did not classify `0x000D` as
/// whitespace, so a lone carriage return could become a token of its own. The normalization is
/// retained as an analyzer-independent input guarantee.
///
/// `lines::normalise_breaks` folds it onto a plain newline before anything is tokenized, which
/// this asserts — the surfaces are still printed, because the fix is only believable next to
/// what it removed.
fn report_crlf(analyzer: &dyn Analyzer, what: &str) {
    let text = "\u{512a}\u{7b49}\u{751f}\u{3060}\u{306d}\u{541b}\u{3063}\u{3066}\r\n\u{5fc3}\u{914d}\u{6027}\u{3060}\u{306d}\u{541b}\u{3063}\u{3066}";
    let analysis = analyzer.analyze(text).expect("CRLF analyzes");
    let surfaces: Vec<&str> = analysis.tokens.iter().map(|t| t.surface.as_str()).collect();
    println!("{what} CRLF surfaces: {surfaces:?}");
    println!(
        "{what} CRLF ranges:   {}",
        sentence_ranges(&analysis.tokens).len()
    );

    assert!(
        surfaces.iter().all(|surface| !surface.contains('\r'),),
        "{what}: a bare carriage return reached the token stream"
    );
    assert_eq!(
        sentence_ranges(&analysis.tokens).len(),
        2,
        "{what}: CRLF separates two sentences just as LF does"
    );
}

fn check(analyzer: &dyn Analyzer, what: &str) {
    let separate = analyzer.analyze(TWO_LINES).expect("two lines analyze");
    no_token_holds_the_break(&separate, what);
    assert_eq!(
        sentence_ranges(&separate.tokens).len(),
        2,
        "{what}: two unpunctuated lines are two sentences, not one"
    );

    let wrapped = analyzer.analyze(WRAPPED).expect("a wrapped line analyzes");
    no_token_holds_the_break(&wrapped, what);
    assert_eq!(
        sentence_ranges(&wrapped.tokens).len(),
        1,
        "{what}: a sentence broken mid-word is still one sentence"
    );
}

#[test]
fn lindera_reads_a_capture_s_line_breaks() {
    let lindera = LinderaAnalyzer::new().expect("the embedded UniDic loads");
    check(&lindera, "lindera");
    report_crlf(&lindera, "lindera");
}

#[test]
fn ichiran_reads_a_capture_s_line_breaks() {
    let analyzer = IchiranAnalyzer::new();
    if analyzer.analyze("水").is_err() {
        println!("SKIP: ichiran-cli is not answering");
        return;
    }
    check(&analyzer, "ichiran");
}
