//! Telling the model where a captured passage's sentences end.
//!
//! # The question this answers
//!
//! A capture arrives as lines, and a line break means one of two opposite things:
//!
//! - **a wrap** — one sentence too wide for the bubble, broken by the panel's edge;
//! - **a boundary** — one line, one utterance, the way lyrics, subtitles and game script come.
//!
//! [`FuguMt::for_the_model`](crate::translate::FuguMt) welds every line together before the
//! encoder sees them, which is right for the first and wrong for the second. `project notes` §7
//! records why it was written that way — a break dropped mid-sentence cost more than the entire
//! gap to a 7B instruct model — but that was measured on single sentences with a break inserted
//! synthetically, because "no sentence in this corpus contains a newline".
//!
//! Welding is the only rule the whole-passage path had, and on a passage that is genuinely several
//! sentences it runs them together. The reported symptom was one word: `schoolhousethat`.
//!
//! # What was measured
//!
//! `core/tests/fugumt_levers.rs`, embedding cosine against the human translations, 75 passages of
//! four lines and 300 wrapped sentences:
//!
//! | | punctuated | punctuation absent | wrapped |
//! |---|---|---|---|
//! | welded, as it shipped | 0.870 | 0.819 | 0.862 |
//! | every boundary marked | 0.870 | **0.870** | 0.812 — 47 wins, 72 ties, **181 losses** |
//! | **marked, wrap-guarded** | 0.870 | **0.865** — 59/9/7, p < 0.001 | **0.847** — 19/**208**/73 |
//! | **the aligned view, as it now ships** | 0.878 | **0.852** — 49/12/14 | 0.819 — 14/**164**/122 |
//!
//! Marking every boundary recovers punctuation-free passages *exactly* — 0.870 is what punctuated
//! text already scored — and does equal and opposite damage to a wrapped one. The guard keeps
//! nearly all of the first and avoids most of the second: 208 of the 300 wrapped sentences come
//! out untouched.
//!
//! **The last row was measured wrong, and the correction is worth keeping.** It was computed by
//! splitting the lines by hand, on the belief that the aligned view did the same because
//! [`sentence_ranges`](crate::model::sentence_ranges) treated a newline as a terminator. It did
//! not — that arm of `ends_sentence` was dead, since no analyzer emits a token carrying a break
//! — so 0.757 was the number for a path the app never took. The aligned view *welded* instead,
//! which is the same defect this module was written for, one layer down. Both are fixed now:
//! `analyzer::lines` records the break against the token it follows, and the levers file
//! measures the shipped aligned path rather than a model of it.

use crate::analyzer::lines::{self, ENDS};

/// A captured passage with its sentence boundaries written where the model can see them.
///
/// **Nothing is rewritten.** A terminator is added only between two lines, only where the first
/// does not already end in one, and only where the parser does not think the line was wrapped —
/// so this states a boundary the app already believes in rather than inventing one. Blank lines
/// are dropped, as they were before this existed.
///
/// **Asked of the shared embedded parser** (`analyzer::lines::looks_wrapped`), whichever analyzer
/// the reader is using: the guard and its measurements are that parser's, and asking the preferred
/// analyzer meant a sidecar call for every join.
///
/// A single-line passage comes back unchanged, which is every caller except the whole-passage
/// one. `translate_sentences` and `translate_variants` build their input by concatenating token
/// surfaces over a range, and no token carries a newline — so the raw text reaches a model
/// through this command and nowhere else.
pub fn mark_boundaries(text: &str) -> String {
    let lines: Vec<&str> = text
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() < 2 {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len() + lines.len());
    for (index, line) in lines.iter().enumerate() {
        out.push_str(line);
        let Some(next) = lines.get(index + 1) else {
            continue;
        };
        if line.ends_with(ENDS) || lines::looks_wrapped(line, next) {
            continue;
        }
        out.push('。');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_is_returned_exactly_as_it_came() {
        assert_eq!(mark_boundaries("水を飲む"), "水を飲む");
        assert_eq!(mark_boundaries(""), "");
    }

    #[test]
    fn separate_lines_get_the_boundary_the_app_already_believes_in() {
        // Two complete utterances, the way a lyric or a subtitle block arrives. Neither carries a
        // terminator, which is the case the welding rule ran together.
        let marked = mark_boundaries("優等生だね君って\n心配性だね君って");
        assert_eq!(marked, "優等生だね君って。心配性だね君って");
    }

    #[test]
    fn a_line_that_already_ends_in_one_is_left_alone() {
        let marked = mark_boundaries("水を飲む。\nパンを食べる。");
        assert_eq!(marked, "水を飲む。パンを食べる。", "no doubled terminator");
    }

    #[test]
    fn a_wrapped_sentence_is_welded_as_before() {
        // The break falls inside `宿泊させて欲しい`, so the parser reads through it. Writing a full
        // stop there is the harm the original line-break measurement found from the other side.
        let marked = mark_boundaries("６人宿泊させ\nて欲しい");
        assert_eq!(marked, "６人宿泊させて欲しい");
    }

    #[test]
    fn a_line_hanging_on_a_linking_particle_is_welded_too() {
        // The break lands on a token boundary, so nothing straddles it — but `映画を` cannot end
        // an utterance, which is the second signal.
        let marked = mark_boundaries("私は友達と映画を\n見た");
        assert_eq!(marked, "私は友達と映画を見た");
    }

    #[test]
    fn a_blank_line_is_dropped_rather_than_marked() {
        let marked = mark_boundaries("優等生だね君って\n\n心配性だね君って");
        assert_eq!(marked, "優等生だね君って。心配性だね君って");
    }
}
