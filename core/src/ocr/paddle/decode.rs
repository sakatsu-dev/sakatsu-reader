//! Turning the recogniser's logits back into characters.
//!
//! The recogniser emits one distribution per time step over a fixed class list, trained
//! with CTC loss. Decoding greedily means taking the best class at each step, then
//! collapsing runs of the same class and dropping the blank — that collapse is what lets
//! CTC align a variable number of steps to a shorter string, and skipping it turns 見る
//! into 見見るる.

use super::super::OcrError;

/// The character list the recogniser's output indices point into.
///
/// # The two extra classes
///
/// The PP-OCRv6 recogniser declares 18710 output classes against a dictionary of 18708
/// characters. The difference is PaddleOCR's own convention, and getting it
/// wrong is the worst kind of bug here — every character past the mismatch decodes as its
/// neighbour, producing fluent-looking Japanese that is simply not what the image says.
///
/// The list is: `blank` at index 0, the dictionary's lines at 1..=n, and a space at the
/// end, matching `CTCLabelDecode`'s `['blank'] + dict + [' ']`.
#[derive(Debug)]
pub struct Charset {
    entries: Vec<String>,
}

impl Charset {
    /// Build the class list from a PaddleOCR dictionary file.
    pub fn from_dictionary(text: &str) -> Charset {
        let mut entries = Vec::with_capacity(text.lines().count() + 2);
        entries.push(String::new()); // 0 is the CTC blank, and never emitted
        entries.extend(text.lines().map(str::to_owned));
        entries.push(" ".into());
        Charset { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check the dictionary against the model that will index into it.
    ///
    /// Called once at load rather than trusted, because the two arrive from different
    /// files and nothing else would notice them disagreeing.
    pub fn check(&self, classes: usize) -> Result<(), OcrError> {
        if self.len() == classes {
            return Ok(());
        }
        Err(OcrError::EngineUnavailable(format!(
            "the character dictionary does not match the recognition model: {} classes \
             expected, dictionary gives {} (blank + {} entries + space). They must come \
             from the same PP-OCRv6 export.",
            classes,
            self.len(),
            self.entries.len().saturating_sub(2),
        )))
    }

    fn get(&self, index: usize) -> Option<&str> {
        // Index 0 is the blank and decodes to nothing.
        match index {
            0 => None,
            _ => self.entries.get(index).map(String::as_str),
        }
    }
}

/// Greedy CTC decode of one line: `steps × classes` logits, row-major.
///
/// Returns the text and the mean probability of the characters kept, which is the
/// closest thing the model offers to "how sure was it" — useful for dropping a line that
/// is only noise.
pub fn greedy(logits: &[f32], classes: usize, charset: &Charset) -> (String, f32) {
    if classes == 0 || logits.is_empty() {
        return (String::new(), 0.0);
    }

    let mut text = String::new();
    let mut total = 0.0f32;
    let mut kept = 0usize;
    let mut previous = usize::MAX;

    for step in logits.chunks_exact(classes) {
        let (index, &score) = step
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .expect("a non-empty step");

        // Collapse a run, but only an unbroken one: a blank between two identical
        // classes is exactly how CTC spells a doubled character, so 学校 keeps both
        // halves of っ in ぎゅっと.
        if index != previous {
            if let Some(character) = charset.get(index) {
                text.push_str(character);
                total += score;
                kept += 1;
            }
        }
        previous = index;
    }

    let confidence = if kept > 0 { total / kept as f32 } else { 0.0 };
    (text, confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// blank, あ, い, う, then the appended space.
    fn charset() -> Charset {
        Charset::from_dictionary("あ\nい\nう")
    }

    /// One step's logits, with `best` given the winning score.
    fn step(classes: usize, best: usize) -> Vec<f32> {
        let mut row = vec![0.1f32; classes];
        row[best] = 0.9;
        row
    }

    #[test]
    fn the_class_list_is_blank_then_the_dictionary_then_a_space() {
        let charset = charset();
        assert_eq!(charset.len(), 5, "blank + 3 entries + space");
        assert_eq!(charset.get(0), None, "the blank decodes to nothing");
        assert_eq!(charset.get(1), Some("あ"));
        assert_eq!(charset.get(3), Some("う"));
        assert_eq!(charset.get(4), Some(" "), "the appended space");
    }

    /// The check that stops a mismatched pair of files producing confident nonsense.
    #[test]
    fn a_dictionary_that_does_not_match_the_model_is_refused() {
        let charset = charset();
        assert!(charset.check(5).is_ok());

        let error = charset.check(6).expect_err("a mismatch is fatal");
        let message = error.to_string();
        assert!(message.contains('6') && message.contains('5'), "{message}");
    }

    #[test]
    fn a_run_of_one_class_collapses_to_one_character() {
        let charset = charset();
        let logits: Vec<f32> = [1, 1, 1, 2, 2]
            .iter()
            .flat_map(|&best| step(5, best))
            .collect();
        let (text, confidence) = greedy(&logits, 5, &charset);
        assert_eq!(text, "あい");
        assert!(confidence > 0.8, "confidence was {confidence}");
    }

    /// The reason the collapse is "unbroken run" and not "same as last emitted".
    #[test]
    fn a_blank_between_two_of_the_same_class_keeps_both() {
        let charset = charset();
        let logits: Vec<f32> = [1, 0, 1].iter().flat_map(|&best| step(5, best)).collect();
        assert_eq!(greedy(&logits, 5, &charset).0, "ああ");
    }

    #[test]
    fn blanks_alone_decode_to_nothing() {
        let charset = charset();
        let logits: Vec<f32> = [0, 0, 0].iter().flat_map(|&best| step(5, best)).collect();
        let (text, confidence) = greedy(&logits, 5, &charset);
        assert!(text.is_empty());
        assert_eq!(confidence, 0.0, "nothing kept means no confidence");
    }
}
