//! The three readings of a selected set of words.
//!
//! The reader lets a learner tick words in and out of a set, and this turns that set into the
//! strings the translator is actually asked about:
//!
//! - **full** — the sentence as written, the baseline everything is compared against;
//! - **runs** — the selected words alone, one string per contiguous run;
//! - **without** — the sentence with the selection taken out.
//!
//! # Why both, rather than just the fragment
//!
//! `runs` is the question a learner asks first — *what does 私による say?* — and it is where a
//! 61M-parameter model is least reliable, because it was trained on sentences and a bare
//! postpositional phrase is not one. `without` is the robust half: its input is still a sentence,
//! so the model stays in the domain it knows, and what disappeared from the English is the
//! selection's contribution. Asking both and comparing is what turns a guess into evidence —
//! leave-one-out attribution, which is a recognised way to induce alignment from a translation
//! model rather than a trick invented here.
//!
//! # The sentence, not the passage
//!
//! A capture is a page. Rebuilding a variant from the whole passage would translate paragraphs to
//! study one particle, and the diff would be swamped by sentences that did not change.
//! [`crate::model::sentence_bounds`] draws the boundary, and it is shared with the particle cues
//! for exactly this reason.

use std::collections::BTreeSet;

use crate::model::{sentence_bounds, Token};

/// What to ask the translator, for one selection.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Variants {
    /// The sentence as written.
    pub full: String,
    /// The selected words alone — one string per run of adjacent tokens, in reading order.
    pub runs: Vec<String>,
    /// The sentence with the selection removed. Empty when nothing would be left.
    pub without: String,
}

/// Why a set of selected token indices cannot become one comparison.
///
/// The old builder silently dropped indices outside the first selected word's sentence. That made
/// the reader strike words the English result had never considered. A rejected question is safer
/// than a plausible answer to only part of it, and gives every caller the same invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantSelectionError {
    OutOfRange { index: usize, token_count: usize },
    MultipleSentences,
}

impl std::fmt::Display for VariantSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfRange { index, token_count } => write!(
                f,
                "comparison token {index} is outside the {token_count}-token passage"
            ),
            Self::MultipleSentences => {
                write!(f, "compare words from one sentence at a time")
            }
        }
    }
}

impl std::error::Error for VariantSelectionError {}

/// Build the three strings for `selected` within their one shared sentence.
///
/// Empty is no question. Every non-empty selection either contributes in full or is rejected;
/// this function never manufactures a successful answer by dropping part of its input.
pub fn variants(
    tokens: &[Token],
    selected: &BTreeSet<usize>,
) -> Result<Option<Variants>, VariantSelectionError> {
    let Some(&anchor) = selected.iter().next() else {
        return Ok(None);
    };
    if let Some(&index) = selected.iter().find(|&&index| index >= tokens.len()) {
        return Err(VariantSelectionError::OutOfRange {
            index,
            token_count: tokens.len(),
        });
    }
    let (start, end) = sentence_bounds(tokens, anchor);
    if selected.iter().any(|index| !(start..end).contains(index)) {
        return Err(VariantSelectionError::MultipleSentences);
    }
    let inside: Vec<usize> = selected.iter().copied().collect();

    let full: String = tokens[start..end]
        .iter()
        .map(|t| t.surface.as_str())
        .collect();

    // One string per unbroken stretch. Splicing separated words together instead would hand the
    // model a sequence no Japanese sentence contains, which is the input it handles worst.
    let mut runs: Vec<String> = Vec::new();
    let mut run = String::new();
    let mut previous: Option<usize> = None;
    for index in &inside {
        if previous.is_some_and(|last| last + 1 != *index) {
            runs.push(std::mem::take(&mut run));
        }
        run.push_str(&tokens[*index].surface);
        previous = Some(*index);
    }
    if !run.is_empty() {
        runs.push(run);
    }

    let dropped: BTreeSet<usize> = inside.into_iter().collect();
    let kept: String = (start..end)
        .filter(|i| !dropped.contains(i))
        .map(|i| tokens[i].surface.as_str())
        .collect();

    Ok(Some(Variants {
        full: full.clone(),
        runs,
        without: tidy(&kept, &full),
    }))
}

/// Repair what removing a token leaves behind.
///
/// **This is the one accuracy lever that belongs to this feature.** Deleting a word can produce
/// input no writer would ever type — a clause opening on 、, two commas together, a sentence with
/// its full stop taken away — and a translation model is at its worst on malformed input. A diff
/// computed from a mangled variant is a lesson drawn from noise, so the variant is put back into
/// the shape of a sentence before it is asked about.
///
/// Deliberately conservative: it removes damage, it does not add words. Inventing a particle to
/// patch a hole would be answering a different question from the one the learner asked.
fn tidy(text: &str, full: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        // A doubled separator is damage from the removal, never something that was written.
        let repeated = matches!(character, '、' | '，' | '・') && out.ends_with(['、', '，', '・']);
        if repeated {
            continue;
        }
        out.push(character);
    }
    let out = out.trim();
    // A clause cannot open on a separator; that comma belonged to the words that went.
    let out = out
        .trim_start_matches(['、', '，', '・', '。', '！', '？'])
        .trim_start();

    // If the sentence had a terminator and the removal took it, give it back — the model reads a
    // finished sentence differently from a trailing fragment, and the comparison is only fair if
    // both sides are sentences.
    let terminator = full
        .chars()
        .last()
        .filter(|c| matches!(c, '。' | '！' | '？'));
    match terminator {
        Some(mark) if !out.is_empty() && !out.ends_with(['。', '！', '？']) => {
            format!("{out}{mark}")
        }
        _ => out.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(surfaces: &[&str]) -> Vec<Token> {
        surfaces
            .iter()
            .map(|s| Token {
                surface: (*s).to_string(),
                dict_form: (*s).to_string(),
                reading: (*s).to_string(),
                lemma_reading: (*s).to_string(),
                ..Token::default()
            })
            .collect()
    }

    fn pick(indices: &[usize]) -> BTreeSet<usize> {
        indices.iter().copied().collect()
    }

    #[test]
    fn a_selection_gives_the_sentence_the_fragment_and_the_remainder() {
        let text = tokens(&["汝", "は", "私", "による", "支配", "を", "求めている", "。"]);
        let got = variants(&text, &pick(&[2, 3]))
            .expect("a valid selection")
            .expect("a selection inside the sentence");
        assert_eq!(got.full, "汝は私による支配を求めている。");
        assert_eq!(got.runs, ["私による"]);
        assert_eq!(got.without, "汝は支配を求めている。");
    }

    /// **Separated words are separate questions.** Splicing 汝 onto 支配 would build a string no
    /// Japanese sentence contains, and that is the input this model handles worst.
    #[test]
    fn a_scattered_selection_becomes_one_string_per_run() {
        let text = tokens(&["汝", "は", "私", "による", "支配", "を", "求めている", "。"]);
        let got = variants(&text, &pick(&[0, 4, 5]))
            .expect("a valid selection")
            .expect("a selection");
        assert_eq!(got.runs, ["汝", "支配を"]);
        assert_eq!(got.without, "は私による求めている。");
    }

    /// Only the sentence the selection sits in — a capture is a page, and translating the page to
    /// study one particle would swamp the comparison with text that did not change.
    #[test]
    fn the_variant_is_built_from_one_sentence_not_the_whole_passage() {
        let text = tokens(&["猫", "が", "いる", "。", "犬", "も", "いる", "。"]);
        let got = variants(&text, &pick(&[1]))
            .expect("a valid selection")
            .expect("a selection");
        assert_eq!(got.full, "猫がいる。");
        assert_eq!(got.without, "猫いる。");
        assert!(
            !got.full.contains("犬"),
            "the next sentence stayed out of it"
        );
    }

    /// A caller that breaks the one-sentence invariant gets no partial success.
    #[test]
    fn a_selection_reaching_outside_the_sentence_is_rejected() {
        let text = tokens(&["猫", "が", "いる", "。", "犬", "も", "いる", "。"]);
        assert_eq!(
            variants(&text, &pick(&[1, 5])),
            Err(VariantSelectionError::MultipleSentences)
        );
    }

    // ---- the tidy-up, which is where the accuracy is ----------------------------------------

    /// **Removing a word must not leave input nobody would write.** The model is at its worst on
    /// malformed text, and a diff taken from a mangled variant teaches noise.
    #[test]
    fn removing_a_word_does_not_leave_a_doubled_comma() {
        let text = tokens(&["寒い", "、", "けれど", "、", "行く", "。"]);
        let got = variants(&text, &pick(&[2]))
            .expect("a valid selection")
            .expect("a selection");
        assert_eq!(got.without, "寒い、行く。", "the two 、 collapsed into one");
    }

    #[test]
    fn a_clause_is_not_left_opening_on_a_comma() {
        let text = tokens(&["今日", "は", "、", "寒い", "。"]);
        let got = variants(&text, &pick(&[0, 1]))
            .expect("a valid selection")
            .expect("a selection");
        assert_eq!(got.without, "寒い。");
    }

    /// The comparison is only fair if both sides are sentences: a model reads a finished sentence
    /// differently from a trailing fragment.
    #[test]
    fn a_removed_terminator_is_given_back() {
        let text = tokens(&["猫", "が", "いる", "。"]);
        let got = variants(&text, &pick(&[3]))
            .expect("a valid selection")
            .expect("a selection");
        assert_eq!(got.without, "猫がいる。", "the full stop returned");
    }

    #[test]
    fn a_sentence_without_a_terminator_does_not_gain_one() {
        let text = tokens(&["猫", "が", "いる"]);
        let got = variants(&text, &pick(&[1]))
            .expect("a valid selection")
            .expect("a selection");
        assert_eq!(got.without, "猫いる");
    }

    #[test]
    fn selecting_everything_leaves_nothing_to_compare_against() {
        let text = tokens(&["猫", "が", "いる"]);
        let got = variants(&text, &pick(&[0, 1, 2]))
            .expect("a valid selection")
            .expect("a selection");
        assert_eq!(got.runs, ["猫がいる"]);
        assert!(got.without.is_empty(), "nothing survives, and it says so");
    }

    #[test]
    fn empty_is_no_question_and_out_of_range_is_an_error() {
        let text = tokens(&["猫", "が", "いる"]);
        assert_eq!(variants(&text, &pick(&[])), Ok(None));
        assert_eq!(
            variants(&text, &pick(&[99])),
            Err(VariantSelectionError::OutOfRange {
                index: 99,
                token_count: 3
            })
        );
    }

    /// Every selected token reaches `runs`, in source order, exactly once. Exhausting all subsets
    /// of one sentence guards the lossless law independently of any particular Japanese example.
    #[test]
    fn every_valid_selection_is_lossless_and_order_preserving() {
        let text = tokens(&["甲", "乙", "丙", "丁", "。"]);
        for mask in 1..(1usize << text.len()) {
            let selected = pick(
                &(0..text.len())
                    .filter(|index| mask & (1 << index) != 0)
                    .collect::<Vec<_>>(),
            );
            let got = variants(&text, &selected)
                .expect("one sentence is valid")
                .expect("the set is not empty");
            let expected_full: String = text.iter().map(|token| token.surface.as_str()).collect();
            let expected_selected: String = selected
                .iter()
                .map(|index| text[*index].surface.as_str())
                .collect();
            assert_eq!(got.full, expected_full);
            assert_eq!(
                got.runs.concat(),
                expected_selected,
                "selection mask {mask:b}"
            );
        }
    }
}
