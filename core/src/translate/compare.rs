//! What one English sentence lost when words were taken out of the Japanese.
//!
//! # Why this is Rust and not the component that draws it
//!
//! It began in `word-diff.js` and moved here for the reason [`crate::grammar::particles`] decides
//! `favoured` in Rust: the number that decides how the interface is *worded* has to be the number
//! the interface actually computes. `core/tests/lens_agreement.rs` measures how often the two
//! lenses corroborate across thousands of sentences, and that measurement is worthless if the
//! panel applies a slightly different rule than the harness did. One implementation, both callers.
//!
//! # It is a comparison, not a claim
//!
//! Nothing here knows any Japanese. It reports what changed in a translation model's output, which
//! is evidence about the model and only indirectly about the grammar — `project notes` §7 is blunt
//! that this translator gets plenty wrong. Wording built on this must never promote a difference
//! into a statement about what a particle means.

/// One run of the original translation, and whether it survived the removal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Piece {
    pub text: String,
    pub gone: bool,
}

/// Split on whitespace, keeping punctuation attached — `domination.` and `domination` are the
/// same word for this purpose, and [`bare`] is what makes them compare equal.
fn words(text: &str) -> Vec<&str> {
    text.split_whitespace().filter(|w| !w.is_empty()).collect()
}

/// Punctuation that should not make a word into a different word.
const PUNCTUATION: &[char] = &['.', ',', '!', '?', ';', ':', '"', '\'', '(', ')'];

/// For matching only. The original spelling is what gets shown.
fn bare(word: &str) -> String {
    word.chars()
        .filter(|c| !PUNCTUATION.contains(c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// `before` in order, each run marked kept or gone.
///
/// A longest-common-subsequence backtrack rather than "is this word still somewhere in `after`":
/// *the* appears five times in a sentence and set membership cannot say which one left.
pub fn word_diff(before: &str, after: &str) -> Vec<Piece> {
    let shown = words(before);
    let left: Vec<String> = shown.iter().map(|w| bare(w)).collect();
    let right: Vec<String> = words(after).iter().map(|w| bare(w)).collect();

    // table[i][j] = length of the longest common subsequence of left[i..] and right[j..]
    let mut table = vec![vec![0usize; right.len() + 1]; left.len() + 1];
    for i in (0..left.len()).rev() {
        for j in (0..right.len()).rev() {
            table[i][j] = if left[i] == right[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }

    let mut pieces: Vec<Piece> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < left.len() {
        if j < right.len() && left[i] == right[j] {
            push(&mut pieces, shown[i], false);
            i += 1;
            j += 1;
        } else if j < right.len() && table[i + 1][j] < table[i][j + 1] {
            j += 1;
        } else {
            push(&mut pieces, shown[i], true);
            i += 1;
        }
    }
    pieces
}

/// Adjacent runs of the same kind join, so the marking is a phrase rather than a stutter.
fn push(pieces: &mut Vec<Piece>, word: &str, gone: bool) {
    match pieces.last_mut() {
        Some(last) if last.gone == gone => {
            last.text.push(' ');
            last.text.push_str(word);
        }
        _ => pieces.push(Piece {
            text: word.to_string(),
            gone,
        }),
    }
}

/// Words shared by almost any two English phrases, and therefore no evidence of anything.
const EMPTY: &[&str] = &[
    "the", "a", "an", "of", "to", "is", "are", "was", "it", "i", "you", "he", "she", "they", "and",
    "in", "on", "at", "s", "t",
];

/// Whether the two lenses corroborate — the only cheap signal that the model is not guessing.
///
/// The fragment is the fragile reading: a model trained on sentences is at its worst on a bare
/// phrase. When what the fragment says is also what the sentence lost, two independent readings
/// agree and the answer can be shown plainly. When they do not, the interface says so rather than
/// picking a winner.
///
/// Deliberately generous about *how much* overlaps. Requiring the strings to match would call
/// "by me" and "by myself" a disagreement, which helps nobody; one shared content word is enough
/// to say the two are talking about the same thing.
pub fn lenses_agree(fragment: &str, pieces: &[Piece]) -> bool {
    let lost: std::collections::HashSet<String> = pieces
        .iter()
        .filter(|piece| piece.gone)
        .flat_map(|piece| words(&piece.text).into_iter().map(bare).collect::<Vec<_>>())
        .filter(|word| !word.is_empty())
        .collect();
    if lost.is_empty() {
        return false;
    }
    words(fragment)
        .into_iter()
        .map(bare)
        .filter(|word| !word.is_empty() && !EMPTY.contains(&word.as_str()))
        .any(|word| lost.contains(&word))
}

/// Whether the fresh translation of the shortened sentence is worth printing beside the marked-up
/// original.
///
/// # The invariant that makes this clean
///
/// [`word_diff`] marks the words of `before` that survived into `after`, so **the kept pieces are
/// a subset of the result by construction** — a word is only "kept" because it was found in the
/// fresh translation. So the two can differ in exactly one direction: the result may contain words
/// the survivors do not.
///
/// That is the whole rule. Strike some words out and what remains on the line already *is* the
/// answer; printing it again underneath says the same thing twice. It earns its place only when
/// the model rewrote something — `犬を散歩に連れて行く` losing its object comes back "He goes for a
/// walk", and neither *He* nor *goes* is anywhere in the line above.
///
/// Compared as sets of content words rather than as strings: the interesting difference is whether
/// a word appeared at all, and requiring the order to match too would print the second line for
/// every reshuffle, which is the noise this removes.
pub fn result_says_more(pieces: &[Piece], without: &str) -> bool {
    // Nothing left to say when the whole sentence went — the panel has its own line for that.
    if without.trim().is_empty() {
        return false;
    }
    let kept: std::collections::HashSet<String> = pieces
        .iter()
        .filter(|piece| !piece.gone)
        .flat_map(|piece| words(&piece.text).into_iter().map(bare).collect::<Vec<_>>())
        .filter(|word| !word.is_empty())
        .collect();
    words(without)
        .into_iter()
        .map(bare)
        .filter(|word| !word.is_empty())
        .any(|word| !kept.contains(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marked-as-gone text, joined — what a reader sees struck through.
    fn gone(pieces: &[Piece]) -> String {
        pieces
            .iter()
            .filter(|p| p.gone)
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn the_words_that_left_are_the_ones_marked() {
        let pieces = word_diff(
            "Even if you deny it in your heart, you seek domination by me.",
            "Even if you deny it in your heart, you seek domination.",
        );
        assert_eq!(gone(&pieces), "by me.");
    }

    /// **Why an LCS and not a set difference.** `the` appears twice; only one left, and asking
    /// "is `the` still present?" answers yes for both and marks neither.
    #[test]
    fn a_repeated_word_is_matched_by_position_rather_than_by_presence() {
        let pieces = word_diff("the cat and the dog", "the dog");
        assert_eq!(gone(&pieces), "cat and the");
    }

    #[test]
    fn punctuation_does_not_make_a_word_into_a_different_word() {
        let pieces = word_diff("He runs, and he stops.", "He runs.");
        assert_eq!(gone(&pieces), "and he stops.");
    }

    #[test]
    fn an_unchanged_translation_marks_nothing() {
        let pieces = word_diff("It is cold.", "It is cold.");
        assert_eq!(gone(&pieces), "");
        assert_eq!(pieces.len(), 1);
        assert!(!pieces[0].gone);
    }

    #[test]
    fn an_empty_remainder_means_the_whole_sentence_went() {
        let pieces = word_diff("It is cold.", "");
        assert_eq!(gone(&pieces), "It is cold.");
    }

    #[test]
    fn the_lenses_agree_when_the_fragment_is_what_the_sentence_lost() {
        let pieces = word_diff("you seek domination by me.", "you seek domination.");
        assert!(lenses_agree("by me", &pieces));
        assert!(!lenses_agree("in the morning", &pieces));
    }

    /// A shared function word is not agreement — counting it would make the check answer yes on
    /// nearly everything, which is the same as not having it.
    #[test]
    fn a_shared_function_word_is_not_agreement() {
        let pieces = word_diff("I gave the book to the teacher.", "I gave the book.");
        assert!(!lenses_agree("to the", &pieces));
        assert!(lenses_agree("to the teacher", &pieces));
    }

    #[test]
    fn nothing_lost_is_nothing_to_agree_with() {
        let pieces = word_diff("It is cold.", "It is cold.");
        assert!(!lenses_agree("cold", &pieces));
    }

    /// The common case, and the one the rule exists for: strike a phrase out and what is left on
    /// the line already says what the sentence became.
    #[test]
    fn a_result_that_is_only_the_surviving_words_is_not_worth_a_second_line() {
        let pieces = word_diff("I feel a sense of well-being.", "I feel.");
        assert_eq!(gone(&pieces), "a sense of well-being.");
        assert!(!result_says_more(&pieces, "I feel."));
    }

    /// And the case that earns it: the shortened sentence was translated afresh, so it can reach
    /// for words the original never used — and those are invisible in the marked-up line, because
    /// `word_diff` can only mark words that were there to begin with.
    #[test]
    fn a_result_with_words_the_original_never_had_is_worth_printing() {
        let pieces = word_diff("I take my dog for a walk.", "He goes for a walk.");
        assert!(result_says_more(&pieces, "He goes for a walk."));
    }

    /// The invariant this rests on, asserted rather than assumed: everything marked *kept* is
    /// present in the result, so the two can only differ in one direction.
    #[test]
    fn the_kept_words_are_always_a_subset_of_the_result() {
        for (before, after) in [
            ("I feel a sense of well-being.", "I feel."),
            ("I take my dog for a walk.", "He goes for a walk."),
            ("the cat and the dog", "the dog"),
            ("He runs, and he stops.", "He runs."),
        ] {
            let result: std::collections::HashSet<String> =
                words(after).into_iter().map(bare).collect();
            for piece in word_diff(before, after).iter().filter(|p| !p.gone) {
                for word in words(&piece.text).into_iter().map(bare) {
                    assert!(
                        result.contains(&word),
                        "{word:?} was marked kept but is not in {after:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_left_is_not_something_more_to_say() {
        let pieces = word_diff("It is cold.", "");
        assert!(!result_says_more(&pieces, ""));
        assert!(!result_says_more(&pieces, "   "));
    }

    /// Punctuation and case are not new words. `bare` already folds them for the diff, and using
    /// the same normalisation here is what keeps the two decisions from disagreeing.
    #[test]
    fn punctuation_alone_does_not_make_a_result_say_more() {
        let pieces = word_diff("He runs, and he stops.", "He runs.");
        assert!(!result_says_more(&pieces, "he runs"));
    }

    #[test]
    fn wording_differences_still_count_as_agreement() {
        let pieces = word_diff("He walks to school by train.", "He walks to school.");
        assert!(
            lenses_agree("By train.", &pieces),
            "case and a full stop are not a disagreement"
        );
    }
}
