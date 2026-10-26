//! Validating one occurrence of a construction inside a match, once, for everyone who needs it.
//!
//! # Why this is a module and not two similar functions
//!
//! Two callers ask the same question. `expression::offered` asks it to anchor a dictionary entry
//! beside the reader's selection; `lexical::select` asks it to decide whether a run of tokens may
//! be joined into one unit. They had two implementations, and the second was much weaker — it
//! read the suffix fields and trusted them. Describing both as "the validated occurrence" was
//! then stronger than the code: one path checked that the ending sits at the end of the run it
//! was found in and inside its parent, and the other did not.
//!
//! So the checks live here, once, as a pure function over tokens and a [`Match`]. A caller adds
//! whatever *its own* question needs — the offer adds "the selection is inside this element", the
//! selector adds "the occurrence is a whole run of tokens" — and neither can quietly drop a check
//! the other relies on.
//!
//! # What is checked
//!
//! Everything §3 established, in one place:
//!
//! 1. the rule position exists on this match — **no element spans, no occurrence**, which is what
//!    a `Match` from a construction annotation or an older interface payload has;
//! 2. the element's tokens are inside the match's own tokens;
//! 3. the consumed run is grounded, byte-contiguous, and covers exactly as much source as it
//!    spells — a token whose span disagrees with its surface tiles its neighbours perfectly and
//!    still makes an offset into the run meaningless;
//! 4. the run **ends with** the expression, read from the tokens rather than taken on trust;
//! 5. the element's own record of what it consumed agrees with those tokens;
//! 6. the ending is exactly the expression's length **and sits at the end of the run** — checking
//!    only its length left the span free to name any six bytes, including the *other* endpoint's
//!    `とか` further down the sentence;
//! 7. the parent's recorded bounds agree with its tokens, and the occurrence lies inside them.

use crate::model::Token;

use super::matcher::Match;

/// One occurrence, with everything a caller needs and nothing it would have to re-derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Validated {
    /// The rule position that consumed it.
    pub element: usize,
    /// The ending's own byte span — the `とか`, not the run it was found in.
    pub start: usize,
    pub end: usize,
    /// The tokens the element consumed.
    pub first_token: usize,
    pub last_token_end: usize,
    /// The construction's own byte span, which an occurrence never claims.
    pub parent_start: usize,
    pub parent_end: usize,
}

/// What a run of tokens spells, and the span it occupies, when it tiles that span.
///
/// A run that does not tile its own span — a gap, a drifted offset, an ungrounded token, a
/// surface that is not the source — yields `None`, because anything anchored to that span would
/// be drawn over text nobody verified.
pub fn spelled_run(tokens: &[Token], first: usize, end: usize) -> Option<(String, usize, usize)> {
    let run = tokens.get(first..end)?;
    if run.is_empty() {
        return None;
    }
    let mut spelled = String::new();
    let mut at: Option<usize> = None;
    for token in run {
        let (start, stop) = (token.start?, token.end?);
        if let Some(previous) = at {
            if previous != start {
                return None;
            }
        }
        // **Each token, not just the total.** A run whose first token claims three bytes too many
        // and whose second claims three too few tiles its neighbours perfectly and adds up to the
        // right length, while every offset *inside* it is wrong. The totals agreeing is not the
        // same fact as the parts agreeing, and an ending located by offset needs the parts.
        //
        // For an analysis that passed the span contract this is implied — `validate_analysis`
        // compares each token's surface with its source. These functions are pure over whatever
        // tokens they are handed, and `expression::offered` is handed tokens from the interface
        // rather than from a freshly validated analysis.
        if stop.checked_sub(start)? != token.surface.len() {
            return None;
        }
        spelled.push_str(&token.surface);
        at = Some(stop);
    }
    let (from, to) = (run.first()?.start?, at?);
    if to.checked_sub(from)? != spelled.len() {
        return None;
    }
    Some((spelled, from, to))
}

/// The parent's own extent, when its record agrees with the tokens it named.
fn parent_bounds(tokens: &[Token], found: &Match) -> Option<(usize, usize)> {
    let recorded = found.start.zip(found.end)?;
    let actual = tokens
        .get(found.first_token)
        .and_then(|token| token.start)
        .zip(
            found
                .last_token_end
                .checked_sub(1)
                .and_then(|last| tokens.get(last))
                .and_then(|token| token.end),
        )?;
    (recorded == actual).then_some(recorded)
}

/// Validate the occurrence at one rule position, or refuse it.
pub fn validate(
    tokens: &[Token],
    found: &Match,
    position: usize,
    expression: &str,
) -> Option<Validated> {
    let (parent_start, parent_end) = parent_bounds(tokens, found)?;
    let element = found.element(position)?;
    if element.first_token < found.first_token || element.last_token_end > found.last_token_end {
        return None;
    }
    let (start, end) = element.suffix()?;
    let (spelled, run_start, run_end) =
        spelled_run(tokens, element.first_token, element.last_token_end)?;
    if !spelled.ends_with(expression) {
        return None;
    }
    if element.start != Some(run_start) || element.end != Some(run_end) {
        return None;
    }
    if end != run_end || Some(start) != run_end.checked_sub(expression.len()) {
        return None;
    }
    // **Defence against a malformed token list**, and it is worth saying which. Given tokens in
    // source order, this is implied: the element's tokens are inside the match's, so its run is
    // inside the match's span. `validate` is a pure function over whatever tokens it is handed,
    // and a caller that hands it tokens out of order would otherwise get an occurrence outside
    // the construction it claims to be in.
    if start < parent_start || end > parent_end {
        return None;
    }
    Some(Validated {
        element: position,
        start,
        end,
        first_token: element.first_token,
        last_token_end: element.last_token_end,
        parent_start,
        parent_end,
    })
}

/// Every valid occurrence a match carries at the given rule positions, in position order.
pub fn all(
    tokens: &[Token],
    found: &Match,
    positions: &[usize],
    expression: &str,
) -> Vec<Validated> {
    positions
        .iter()
        .filter_map(|&position| validate(tokens, found, position, expression))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::matcher::ElementSpan;

    fn token(surface: &str, at: usize) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            start: Some(at),
            end: Some(at + surface.len()),
            ..Default::default()
        }
    }

    /// **Tokens out of source order cannot produce an occurrence.**
    ///
    /// Hand-built, because the matcher does not emit this and never should. What is under test is
    /// that `validate` does not *assume* it: every other check here passes — the element is
    /// inside the match's tokens, its run tiles its span and ends with とか, the ending is at the
    /// end of that run, and the match's recorded bounds agree with its first and last tokens —
    /// and the occurrence still lies outside the span the match claims.
    #[test]
    fn an_occurrence_outside_its_parents_span_is_refused() {
        // The first token of the match sits *after* the rest of it in the source.
        let tokens = vec![token("映画", 100), token("と", 0), token("か", 3)];
        let found = Match {
            pattern_id: "t".into(),
            first_token: 0,
            last_token_end: 3,
            start: Some(100),
            end: Some(6),
            elements: vec![ElementSpan {
                element: 0,
                first_token: 1,
                last_token_end: 3,
                start: Some(0),
                end: Some(6),
                suffix_start: Some(0),
                suffix_end: Some(6),
            }],
        };
        // The parent's recorded bounds do agree with its own first and last tokens, so that check
        // is not what refuses this.
        assert_eq!(parent_bounds(&tokens, &found), Some((100, 6)));
        assert_eq!(validate(&tokens, &found, 0, "とか"), None);
    }

    /// **Two token-length errors that cancel are still two errors.**
    ///
    /// The run tiles its span, and the span is exactly as long as the run spells — every total
    /// agrees. What does not agree is either token taken on its own, and an ending is located by
    /// offsetting into one of them.
    #[test]
    fn opposing_token_length_errors_do_not_cancel() {
        let honest = vec![token("映画と", 0), token("か", 9)];
        assert_eq!(
            spelled_run(&honest, 0, 2),
            Some(("映画とか".to_string(), 0, 12)),
            "the honest layout first, so the test is about the change it makes"
        );

        // 映画と claims three bytes too many and か claims three too few. The run still tiles
        // 0..12 with no gap, and 0..12 is still exactly as long as 映画とか spells — every total
        // agrees, and neither token does.
        let mut cancelling = honest.clone();
        cancelling[0].end = Some(12);
        cancelling[1].start = Some(12);
        cancelling[1].end = Some(12);
        assert_eq!(
            cancelling[0].end.unwrap() - cancelling[0].start.unwrap(),
            cancelling[0].surface.len() + 3
        );
        assert_eq!(cancelling[1].end.unwrap() - cancelling[1].start.unwrap(), 0);
        assert!(
            spelled_run(&cancelling, 0, 2).is_none(),
            "the totals agree and the parts do not; nothing here can be anchored"
        );
    }

    /// The same shape in source order validates, so the refusal above is about the order.
    #[test]
    fn the_same_occurrence_in_source_order_validates() {
        let tokens = vec![token("映画", 0), token("と", 6), token("か", 9)];
        let found = Match {
            pattern_id: "t".into(),
            first_token: 0,
            last_token_end: 3,
            start: Some(0),
            end: Some(12),
            elements: vec![ElementSpan {
                element: 0,
                first_token: 1,
                last_token_end: 3,
                start: Some(6),
                end: Some(12),
                suffix_start: Some(6),
                suffix_end: Some(12),
            }],
        };
        let valid = validate(&tokens, &found, 0, "とか").expect("a well-formed occurrence");
        assert_eq!((valid.start, valid.end), (6, 12));
        assert_eq!((valid.parent_start, valid.parent_end), (0, 12));
    }
}
