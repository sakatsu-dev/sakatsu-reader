//! Where a capture's lines end, and whether each break ends a sentence.
//!
//! # The break is not in the token stream
//!
//! Neither analyzer reports a line break. Lindera cuts its lattice at every `\n` before Viterbi
//! and then drops the whitespace token; Ichiran returns the break as its own segment and
//! `convert` skips whitespace-only ones. `ui/src/lib/passage.js` records the same fact from the
//! other side: "the break lives in the gap *between* two tokens, which no token covers".
//!
//! So [`crate::model::ends_sentence`] listed `'\n'` among its terminators and never once matched.
//! A punctuation-free capture — lyrics, subtitles, most game script — arrived as **one** sentence
//! however many lines it had, which put a whole passage on every Anki card made from it and welded
//! the aligned translation into a single run-on row.
//!
//! This module puts the fact back, once, against the token it follows.
//!
//! # Why every break is not simply a boundary
//!
//! PP-OCR emits one line per detected box, and for vertical Japanese a *column* is a box. One
//! sentence down three columns of a bubble arrives as three lines. Splitting there would shatter
//! it — measurably worse than the bug being fixed. So each break is classified, and the guard is
//! the one measured in `core/tests/fugumt_levers.rs`.

use crate::analyzer::assist::{Advisor, Assistance, AssistanceMode, Stage};
use crate::model::{Analysis, LineBreak};

/// Particles that link a phrase to what follows, so a line ending in one is unfinished.
///
/// Deliberately **not** the ones that can close an utterance — か, ね, よ, な, さ, の, わ — and
/// deliberately not the te-form, which ends plenty of complete lines: `制服だって上手く着崩して`
/// is a whole lyric, and treating a trailing て as unfinished would weld it to the next one.
///
/// **One list for every caller**: the reader's sentence ranges ([`mark`]) and the translation of a
/// whole passage ([`looks_wrapped`]). They were two copies, and two copies of a tuned list drift.
const HANGING: [&str; 10] = [
    "を", "が", "に", "へ", "と", "で", "から", "より", "まで", "ば",
];

/// What already ends a sentence on its own, so no probe is needed — [`crate::model::sentence_bounds`]'s
/// list, minus the newline, which is the thing being decided about.
pub(crate) const ENDS: [char; 5] = ['。', '！', '？', '!', '?'];

/// Asks whether a parser reads straight through the join of two lines.
///
/// A closure rather than an `&dyn Analyzer`, and that is not a style choice: the Lindera adapter
/// calls this from inside its own `analyze`, and calling back into `analyze` would deadlock on the
/// tokenizer's `Mutex`, which is not reentrant. Passing the probe lets the adapter answer from the
/// lock it is already holding — or drop it first, as it does.
pub(crate) type Straddles<'a> = &'a dyn Fn(&str, &str) -> bool;

/// Record where the source breaks between tokens, and what each break means.
///
/// Idempotent, and safe to call on a single-line passage, where it does nothing.
pub(crate) fn mark(analysis: &mut Analysis, straddles: Straddles<'_>) {
    let text = analysis.text.clone();
    if !text.contains('\n') {
        return;
    }

    // Every break, as a byte range covering the run of whitespace around it. A run holding two or
    // more newlines is a paragraph gap, which is never a wrap.
    let mut breaks: Vec<(usize, usize, bool)> = Vec::new();
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\n' || bytes[at] == b'\r' {
            let start = at;
            let mut newlines = 0;
            while at < bytes.len() && (bytes[at] as char).is_whitespace() {
                if bytes[at] == b'\n' {
                    newlines += 1;
                }
                at += 1;
            }
            if newlines > 0 {
                breaks.push((start, at, newlines > 1));
            }
            continue;
        }
        at += 1;
    }

    for (start, end, paragraph) in breaks {
        // The token the break follows: the last one that ends at or before it. A break with no
        // token before it — a passage opening on a newline — belongs to nothing.
        let Some(index) = analysis
            .tokens
            .iter()
            .rposition(|token| token.end.is_some_and(|finish| finish <= start))
        else {
            continue;
        };
        // And nothing after it means nothing was separated.
        if !analysis
            .tokens
            .iter()
            .any(|token| token.start.is_some_and(|begin| begin >= end))
        {
            continue;
        }

        let decided = if paragraph {
            // A blank line between two lines is a paragraph gap. Nothing wraps across one.
            LineBreak::Break
        } else if analysis.tokens[index].surface.ends_with(ENDS) {
            LineBreak::Break
        } else if HANGING.contains(&analysis.tokens[index].surface.as_str()) {
            // `映画を` cannot end an utterance. This signal needs no probe, because the token that
            // carries it is the one being marked.
            LineBreak::Wrap
        } else {
            let before = line_before(&text, start);
            let after = line_after(&text, end);
            if straddles(before, after) {
                LineBreak::Wrap
            } else {
                LineBreak::Break
            }
        };
        analysis.tokens[index].line_break = decided;
    }
}

/// The line ending at `at`, back to the previous break or the start.
fn line_before(text: &str, at: usize) -> &str {
    let from = text[..at].rfind('\n').map_or(0, |found| found + 1);
    text[from..at].trim()
}

/// The line beginning at `at`, forward to the next break or the end.
fn line_after(text: &str, at: usize) -> &str {
    let to = text[at..].find('\n').map_or(text.len(), |found| at + found);
    text[at..to].trim()
}

/// Fold every flavour of line ending onto `\n`, before anything is tokenized.
///
/// **Normalize before asking any analyzer.** The former IPADIC path did not classify `0x000D`
/// as whitespace, so a CRLF passage could gain a bare carriage-return token between its lines.
/// Keeping one canonical newline also prevents analyzer-specific line-ending behavior from
/// leaking into sentence ranges. `core/tests/sentence_lines.rs` prints the measured surfaces.
///
/// That token is not merely untidy. It falls inside a sentence range, so it reaches the string a
/// comparison is built from and the sentence written onto an Anki card. Windows clipboard text is
/// CRLF, so pasting a multi-line passage was enough to produce one.
///
/// Borrowed when there is nothing to do, which is every capture that came from OCR.
pub fn normalise_breaks(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('\r') {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Mark a passage using the shared embedded parser.
///
/// **Always Lindera, whichever analyzer produced the tokens**, and that is deliberate:
///
/// - the sentence partition now decides what goes on an Anki card and what a comparison may span,
///   so it must not change because WSL happened to be up;
/// - Ichiran is a subprocess per call, and a probe per break would be a spawn per line;
/// - "did the parser read through this join" is a segmentation question, which is the one row of
///   `lindera/mod.rs`'s own comparison table where the two engines are equals;
/// - Ichiran fuses particles into larger units, so it is measurably worse at the second signal;
/// - and the guard's numbers in `core/tests/fugumt_levers.rs` are Lindera's.
///
/// The embedded dictionary is loaded lazily and only for a passage that actually has a break
/// neither cheap signal settles, so a reader working one line at a time never pays for it.
pub fn mark_with_shared_probe(analysis: &mut Analysis) {
    mark_in(analysis, &Assistance::new(AssistanceMode::Legacy), None);
}

/// [`mark`], with each break neither cheap signal settles put to the request as a
/// [`Stage::JoinedLines`] question.
///
/// **Who answers is the caller's to say.** The Ichiran conversion passes `None`, and the request's
/// provider — the shared embedded parser — answers, as the probe always did. The built-in analyzer
/// marking its own passage passes itself, so it never loads a second parser for its own breaks.
///
/// **Asked inside the closure, not before it**, and that is what keeps the provider lazy: `mark`
/// returns immediately for a passage with no newline and asks only for a break neither cheap
/// signal settles, so a reader working one line at a time never loads the parser for this. A
/// question that is not answered — no parser, or one that failed — leaves the break a boundary:
/// separate lines are the common case, and merging them is the bug this exists to fix.
pub(crate) fn mark_in(
    analysis: &mut Analysis,
    assistance: &Assistance<'_>,
    advisor: Option<&dyn Advisor>,
) {
    mark(analysis, &|before, after| {
        reads_through(assistance, advisor, before, after)
    });
}

/// Whether the parser reads straight through the join of two lines, asked through the request as
/// a [`Stage::JoinedLines`] question. An unanswered question is no.
fn reads_through(
    assistance: &Assistance<'_>,
    advisor: Option<&dyn Advisor>,
    before: &str,
    after: &str,
) -> bool {
    joined_reading(assistance, advisor, before, after, |read| {
        straddled(
            before.len(),
            read.tokens
                .iter()
                .filter_map(|token| Some((token.start?, token.end?))),
        )
    })
}

/// The parser's reading of two lines joined, asked through the request as a [`Stage::JoinedLines`]
/// question, put to `test`. An unanswered question is no.
fn joined_reading(
    assistance: &Assistance<'_>,
    advisor: Option<&dyn Advisor>,
    before: &str,
    after: &str,
    test: impl FnOnce(&Analysis) -> bool,
) -> bool {
    // Nothing on one side means nothing was joined, which needs no parser to see.
    if before.is_empty() || after.is_empty() {
        return false;
    }
    let joined = format!("{before}{after}");
    let advice = match advisor {
        Some(advisor) => assistance.ask_of(Stage::JoinedLines, &joined, advisor),
        None => assistance.ask(Stage::JoinedLines, &joined),
    };
    advice.answered().is_some_and(test)
}

/// Whether the join at `join` cuts a word of `tokens`: a token crosses it, and so does the part of
/// that token the join falls in.
///
/// **Parts, not only tokens.** The built-in analyzer groups words into larger tokens, and a group
/// can cross a join while every word in it is whole: ご来館 at the end of one line and 心より at the
/// start of the next were grouped as ご来館心, and 徘徊 and する as 徘徊する. That is enough to call
/// the break a wrap ([`mark`]), but taking it out would hand the reader ご来館心. Only a word that
/// is itself cut — 払 | 拭, 見 | つけ — says the text box broke it.
fn cuts_a_word(join: usize, tokens: &[crate::model::Token]) -> bool {
    let crosses = |token: &crate::model::Token| matches!((token.start, token.end), (Some(start), Some(end)) if start < join && join < end);
    tokens.iter().any(|token| {
        crosses(token) && (token.components.is_empty() || token.components.iter().any(crosses))
    })
}

/// The passage with every line break a word runs across taken out, before any analyzer reads it.
///
/// **Why.** [`mark`] decides what a break means, but only after the analyzers have read the
/// passage, and both cut at a newline: the built-in one cuts its lattice there and Ichiran returns
/// the break as its own segment. A game's text box wraps where its width runs out, mid-word as
/// often as not, so 払拭 wrapped as 払 / 拭 was read as two pieces, and neither is the word the
/// reader wanted to look up. PaddleOCR reports one line per box it detects, so
/// the break is really there; only the reader can tell that it means nothing.
///
/// **Which breaks.** Only one that cuts a word: the parser reads the two lines joined, and a word
/// crosses the join ([`cuts_a_word`]). A line hanging on a linking particle wraps too, but its
/// break falls between words, splits nothing, and stays for [`mark`] to classify. A line ending in
/// a terminator, a blank line, and a break with anything but Japanese on either side are never
/// asked about: the parser reads `gives` and `us` on two lines as one unknown run, and joining
/// them would weld two English words.
///
/// **What it costs.** The break is gone from the passage, so "keep the capture's line breaks" runs
/// those two lines on. A break inside a word was the text box's, not the writer's.
pub(crate) fn rejoin_split_words<'t>(
    text: &'t str,
    straddles: Straddles<'_>,
) -> std::borrow::Cow<'t, str> {
    let text = normalise_breaks(text);
    let cuts = split_word_breaks(&text, straddles);
    if cuts.is_empty() {
        return text;
    }
    let mut joined = String::with_capacity(text.len());
    let mut from = 0;
    for (start, end) in cuts {
        joined.push_str(&text[from..start]);
        from = end;
    }
    joined.push_str(&text[from..]);
    std::borrow::Cow::Owned(joined)
}

/// [`rejoin_split_words`], with each straddle question asked through the request — of its provider,
/// the shared embedded parser, as [`mark_in`] asks for the Ichiran conversion.
pub(crate) fn rejoin_split_words_in<'t>(
    text: &'t str,
    assistance: &Assistance<'_>,
) -> std::borrow::Cow<'t, str> {
    rejoin_split_words(text, &|before, after| {
        joined_reading(assistance, None, before, after, |read| {
            cuts_a_word(before.len(), &read.tokens)
        })
    })
}

/// The byte range of each break a word straddles: the newline, with the spaces either side of it.
fn split_word_breaks(text: &str, straddles: Straddles<'_>) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut cuts = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'\n' {
            at += 1;
            continue;
        }
        let mut start = at;
        while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
            start -= 1;
        }
        let mut end = at;
        let mut newlines = 0;
        while end < bytes.len() && bytes[end].is_ascii_whitespace() {
            if bytes[end] == b'\n' {
                newlines += 1;
            }
            end += 1;
        }
        at = end;
        // A blank line between two lines is a paragraph gap, which no word runs across.
        if newlines > 1 {
            continue;
        }
        let before = line_before(text, start);
        let after = line_after(text, end);
        let japanese = |c: Option<char>| c.is_some_and(crate::kana::is_japanese);
        if !japanese(before.chars().last()) || !japanese(after.chars().next()) {
            continue;
        }
        if straddles(before, after) {
            cuts.push((start, end));
        }
    }
    cuts
}

/// Whether the break between two lines looks like a **wrap**, for a caller that has only the text.
///
/// Translation is that caller: `translate::mark_boundaries` writes the boundaries the app believes
/// in before a passage reaches the model, and has no analysis of its own to read them from. It asks
/// the same two signals [`mark`] does, **of the same parser**, whichever analyzer the reader is
/// using — the guard's numbers in `core/tests/fugumt_levers.rs` are this parser's, and the
/// preferred analyzer was a sidecar call per line join:
///
/// - **a token straddles the join** — almost never wrong: the parser read straight through the
///   break, which happens only inside a word;
/// - **the line hangs on a linking particle** — `映画を` cannot end an utterance, and a break
///   landing on that token boundary is invisible to the first signal.
///
/// Measured together at 56% of wraps caught for a 1% false alarm on genuine sentence breaks. Both
/// errors cost something — a false wrap welds two utterances, a missed one shatters one — so the
/// next person tuning [`HANGING`] is trading, not tightening.
///
/// A parser that cannot be loaded leaves every break a boundary, as [`mark_in`] does.
pub(crate) fn looks_wrapped(before: &str, after: &str) -> bool {
    let Some(parser) = crate::analyzer::lindera::shared() else {
        return false;
    };
    if parser.straddles(before, after) {
        return true;
    }
    let Ok(analysis) = crate::analyzer::Analyzer::analyze(parser, before) else {
        return false;
    };
    analysis
        .tokens
        .iter()
        .rev()
        .find(|token| !token.surface.trim().is_empty())
        .is_some_and(|token| HANGING.contains(&token.surface.as_str()))
}

/// The straddle half of the guard, given a parser's tokens for the two lines joined.
///
/// **Joined with nothing between them**, which is the only way to ask the question: with the break
/// still in place Lindera cuts its lattice there and could never read through it.
pub(crate) fn straddled(join: usize, spans: impl Iterator<Item = (usize, usize)>) -> bool {
    spans
        .into_iter()
        .any(|(start, end)| start < join && join < end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Token;

    /// A probe that never finds a straddle, so only the cheap signals decide.
    fn never(_: &str, _: &str) -> bool {
        false
    }

    /// A probe that always finds one.
    fn always(_: &str, _: &str) -> bool {
        true
    }

    fn passage(text: &str, spans: &[(usize, usize)]) -> Analysis {
        Analysis {
            execution: None,
            text: text.to_string(),
            tokens: spans
                .iter()
                .map(|&(start, end)| Token {
                    surface: text[start..end].to_string(),
                    start: Some(start),
                    end: Some(end),
                    ..Default::default()
                })
                .collect(),
        }
    }

    #[test]
    fn a_single_line_is_left_alone() {
        let mut analysis = passage("水を飲む", &[(0, "水を飲む".len())]);
        mark(&mut analysis, &never);
        assert_eq!(analysis.tokens[0].line_break, LineBreak::None);
    }

    #[test]
    fn a_break_the_parser_reads_through_is_a_wrap() {
        let text = "６人宿泊させ\nて欲しい";
        let split = "６人宿泊させ".len();
        let mut analysis = passage(text, &[(0, split), (split + 1, text.len())]);
        mark(&mut analysis, &always);
        assert_eq!(analysis.tokens[0].line_break, LineBreak::Wrap);
    }

    #[test]
    fn a_break_it_does_not_read_through_ends_the_sentence() {
        let text = "優等生\n心配性";
        let split = "優等生".len();
        let mut analysis = passage(text, &[(0, split), (split + 1, text.len())]);
        mark(&mut analysis, &never);
        assert_eq!(analysis.tokens[0].line_break, LineBreak::Break);
    }

    #[test]
    fn a_blank_line_is_a_paragraph_gap_and_never_a_wrap() {
        // Asserted with the probe that always claims a straddle, so the paragraph rule is what is
        // being tested rather than the probe agreeing by luck.
        let text = "優等生\n\n心配性";
        let split = "優等生".len();
        let mut analysis = passage(text, &[(0, split), (split + 2, text.len())]);
        mark(&mut analysis, &always);
        assert_eq!(analysis.tokens[0].line_break, LineBreak::Break);
    }

    #[test]
    fn a_line_already_ending_in_a_terminator_needs_no_probe() {
        let text = "水を飲む。\nパン";
        let split = "水を飲む。".len();
        let mut analysis = passage(text, &[(0, split), (split + 1, text.len())]);
        mark(&mut analysis, &always);
        assert_eq!(analysis.tokens[0].line_break, LineBreak::Break);
    }

    #[test]
    fn a_line_hanging_on_a_linking_particle_is_a_wrap_without_a_probe() {
        // `映画を` cannot end an utterance, and the break lands on a token boundary where the
        // straddle signal is blind — which is the whole reason this second signal exists.
        let text = "映画を\n見た";
        let split = "映画を".len();
        let mut analysis = Analysis {
            execution: None,
            text: text.to_string(),
            tokens: vec![
                Token {
                    surface: "映画".to_string(),
                    start: Some(0),
                    end: Some("映画".len()),
                    ..Default::default()
                },
                // Split off, because Lindera splits it off — and the whole point of this signal is
                // that it reads the line's *last* token, which is the bare particle.
                Token {
                    surface: "を".to_string(),
                    start: Some("映画".len()),
                    end: Some(split),
                    ..Default::default()
                },
                Token {
                    surface: "見た".to_string(),
                    start: Some(split + 1),
                    end: Some(text.len()),
                    ..Default::default()
                },
            ],
        };
        mark(&mut analysis, &never);
        assert_eq!(analysis.tokens[1].line_break, LineBreak::Wrap);
    }

    #[test]
    fn an_ungrounded_token_leaves_the_break_undecided() {
        let text = "優等生\n心配性";
        let split = "優等生".len();
        let mut analysis = passage(text, &[(0, split), (split + 1, text.len())]);
        analysis.tokens[0].end = None;
        mark(&mut analysis, &never);
        assert_eq!(analysis.tokens[0].line_break, LineBreak::None);
    }

    #[test]
    fn a_break_a_word_runs_across_is_taken_out_with_its_spaces() {
        assert_eq!(
            rejoin_split_words("６人宿泊させ\nて欲しい", &always),
            "６人宿泊させて欲しい"
        );
        assert_eq!(
            rejoin_split_words("６人宿泊させ \n て欲しい", &always),
            "６人宿泊させて欲しい"
        );
    }

    #[test]
    fn a_break_no_word_runs_across_is_kept() {
        let kept = rejoin_split_words("優等生\n心配性", &never);
        assert!(
            matches!(kept, std::borrow::Cow::Borrowed(_)),
            "nothing to do, nothing copied"
        );
        assert_eq!(kept, "優等生\n心配性");
    }

    #[test]
    fn a_paragraph_gap_or_a_finished_line_is_never_rejoined() {
        // The probe that always claims a straddle, so the rules are what is tested.
        assert_eq!(
            rejoin_split_words("優等生\n\n心配性", &always),
            "優等生\n\n心配性"
        );
        assert_eq!(
            rejoin_split_words("水を飲む。\nパン", &always),
            "水を飲む。\nパン"
        );
    }

    #[test]
    fn rejoining_folds_carriage_returns_as_the_analyzers_do() {
        assert_eq!(
            rejoin_split_words("優等生\r\n心配性", &never),
            "優等生\n心配性"
        );
    }

    #[test]
    fn a_break_with_anything_but_japanese_beside_it_is_never_asked_about() {
        assert_eq!(
            rejoin_split_words("best\nexercise", &always),
            "best\nexercise"
        );
        assert_eq!(rejoin_split_words("優等生\n2人", &always), "優等生\n2人");
    }

    fn token(start: usize, end: usize, parts: &[(usize, usize)]) -> Token {
        Token {
            start: Some(start),
            end: Some(end),
            components: parts.iter().map(|&(s, e)| token(s, e, &[])).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_word_is_cut_only_where_the_join_falls_inside_it() {
        // One word across the join.
        assert!(cuts_a_word(3, &[token(0, 6, &[])]));
        // A group whose part crosses the join.
        assert!(cuts_a_word(3, &[token(0, 9, &[(0, 6), (6, 9)])]));
        // A group whose parts meet at the join: each word is whole on its own line.
        assert!(!cuts_a_word(6, &[token(0, 9, &[(0, 6), (6, 9)])]));
        // Nothing crosses it.
        assert!(!cuts_a_word(6, &[token(0, 6, &[]), token(6, 9, &[])]));
    }
}
