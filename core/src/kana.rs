//! Kana utilities.
//!
//! Small and deliberately dependency-free. Two consumers: normalizing lemma
//! readings out of the analyzer (see [`to_hiragana`]), and the grammar engine's
//! formation generator, which works on kana because kana is where Japanese
//! conjugation is phonologically regular.

/// Convert katakana to hiragana, leaving everything else untouched.
///
/// Ichiran writes some lemma readings with a katakana stem — 来る's reading comes
/// back as `クる` — so lemma readings are normalized for display. **Never apply
/// this to a surface reading**: that drives furigana, and コーヒー must stay
/// katakana (`project notes` §4).
///
/// The long-vowel mark ー (U+30FC) and the katakana-only ヷヸヹヺ are left alone:
/// ー is used with both syllabaries and the others have no hiragana counterpart.
pub fn to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            // Katakana ァ..ヶ → hiragana ぁ..ゖ.
            '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            // Iteration marks ヽヾ → ゝゞ.
            '\u{30FD}'..='\u{30FE}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// One character's worth of [`to_hiragana`], for callers working per character.
pub fn to_hiragana_char(c: char) -> char {
    match c {
        '\u{30A1}'..='\u{30F6}' | '\u{30FD}'..='\u{30FE}' => {
            char::from_u32(c as u32 - 0x60).unwrap_or(c)
        }
        _ => c,
    }
}

/// Whether `c` is a hiragana or katakana letter (excluding punctuation such as
/// the ・ middle dot, and excluding the ー length mark).
pub fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}')
}

/// Whether a whole word is written in katakana and nothing else.
///
/// Two callers with the same question and different reasons: the gloss index demotes a
/// katakana headword because it is usually a transliteration, and the analysis page treats
/// a katakana run no dictionary knows as probably a name. Empty is false — nothing is not
/// katakana.
///
/// **At least one katakana *letter*.** The block runs ゠ to ヿ and holds punctuation as well
/// — ゠ ・ ー ヽ ヾ — so a test of the block alone answers "yes" for a token that is nothing
/// but marks. That reached the second caller: Ichiran passes punctuation runs through as
/// unanalyzed tokens with no senses and no score, so clicking a bare ・ printed "No
/// dictionary has this spelling — likely a name." The marks are still allowed *beside*
/// letters, because コーヒー and セイア・ミサキ are katakana words and are written with them.
///
/// The first caller only ever sees dictionary headwords, where the block test was safe; it
/// stays correct under the narrower one for the same reason.
pub fn is_katakana_word(term: &str) -> bool {
    let mut letter = false;
    for c in term.chars() {
        if !matches!(c, '\u{30a0}'..='\u{30ff}') {
            return false;
        }
        letter |= matches!(c, '\u{30a1}'..='\u{30fa}');
    }
    letter
}

/// Whether `c` is a kanji, including the 々 repetition mark.
///
/// 々 is here because it *stands for* a kanji — 人々 is 人 twice — and every caller
/// walking a word's characters has to treat it as one.
pub fn is_kanji(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}'   // CJK unified ideographs
        | '\u{3400}'..='\u{4DBF}' // …extension A
        | '\u{F900}'..='\u{FAFF}' // compatibility ideographs
        | '\u{3005}'              // 々
    )
}

/// Whether `c` belongs to Japanese text at all: kana, kanji, or halfwidth katakana.
///
/// **One definition, because there were four.** Two of them decided the same
/// user-visible branch and disagreed: the command layer counted halfwidth katakana and
/// the reference page's regex did not, so searching ｱｲｳ took the Japanese path in Rust
/// while the page captioned the results as an English search. The others differed over
/// 々. A predicate this small is easy to rewrite badly in a hurry, which is exactly why
/// it should exist once.
pub fn is_japanese(c: char) -> bool {
    is_kana(c)
        || is_kanji(c)
        || matches!(c,
            '\u{3040}'..='\u{309F}'   // hiragana block incl. iteration marks
            | '\u{30A0}'..='\u{30FF}' // katakana block incl. ー and ・
            | '\u{FF66}'..='\u{FF9F}' // halfwidth katakana
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_katakana_stem_reading_becomes_hiragana() {
        // The case from project notes §4: Ichiran writes 来る's reading as 【クる】.
        assert_eq!(to_hiragana("クる"), "くる");
        assert_eq!(to_hiragana("たべる"), "たべる");
    }

    #[test]
    fn conversion_leaves_kanji_marks_and_katakana_only_letters_alone() {
        assert_eq!(to_hiragana("食べる"), "食べる");
        // ー must survive: コーヒー is not ｺーﾋー with a hiragana dash.
        assert_eq!(to_hiragana("コーヒー"), "こーひー");
        // ヺ has no hiragana counterpart.
        assert_eq!(to_hiragana("ヺ"), "ヺ");
        assert_eq!(to_hiragana("ヽヾ"), "ゝゞ");
    }

    #[test]
    fn katakana_words_are_recognised_whole() {
        assert!(is_katakana_word("ブック"));
        assert!(
            is_katakana_word("コーヒー"),
            "the long mark is katakana too"
        );
        assert!(is_katakana_word("セイア"));
        assert!(!is_katakana_word("本"));
        assert!(!is_katakana_word("たべる"));
        assert!(!is_katakana_word("セイアちゃん"), "mixed is not katakana");
        assert!(!is_katakana_word(""));
    }

    /// A word needs a letter in it. Ichiran hands punctuation runs through as tokens of
    /// their own, and a bare ・ satisfying this printed "likely a name" on the analysis page.
    #[test]
    fn marks_without_letters_are_not_a_katakana_word() {
        assert!(!is_katakana_word("・"));
        assert!(!is_katakana_word("ー"));
        assert!(!is_katakana_word("ー・ー"));
        assert!(!is_katakana_word("゠"));
        // But beside letters they are part of the word, which is why they are allowed at all.
        assert!(is_katakana_word("セイア・ミサキ"));
        assert!(is_katakana_word("コーヒー"));
    }

    #[test]
    fn kana_excludes_punctuation_and_the_length_mark() {
        assert!(is_kana('あ'));
        assert!(is_kana('ア'));
        assert!(!is_kana('・'));
        assert!(!is_kana('ー'));
        assert!(!is_kana('食'));
    }
}
