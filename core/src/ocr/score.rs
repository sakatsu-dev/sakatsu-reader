//! Character error rate, for comparing a recognized string against ground truth.
//!
//! Lives in the library rather than in the test that uses it because a scoring bug is
//! invisible: it does not crash, it silently reorders which engine looks better. Here it
//! can be unit-tested against answers worked out by hand.
//!
//! Characters, not bytes. Japanese text is three bytes per character in UTF-8, so a
//! byte-wise edit distance would weight a wrong kanji three times a wrong ASCII digit and
//! quietly make any comparison involving mixed text meaningless.

/// Levenshtein distance in `char`s.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    // Two rows rather than the full matrix: only the previous one is ever read, and a
    // page of text against a page of text would otherwise be a large allocation for
    // nothing.
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];

    for (i, &ac) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, &bc) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ac != bc);
            let deletion = previous[j + 1] + 1;
            let insertion = current[j] + 1;
            current[j + 1] = substitution.min(deletion).min(insertion);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b.len()]
}

/// Character error rate: edits needed, over the length of the truth.
///
/// Can exceed 1.0, and is left that way rather than clamped — an engine that returned
/// three times as much text as it should is worse than one that returned nothing, and a
/// clamp would show them as equal.
///
/// An empty reference scores 0.0 against empty output and 1.0 against anything else.
/// There is no meaningful rate to report when there is nothing to be wrong about, and
/// dividing by zero to get infinity would poison every average it entered.
pub fn character_error_rate(reference: &str, got: &str) -> f64 {
    let length = reference.chars().count();
    if length == 0 {
        return if got.chars().next().is_none() {
            0.0
        } else {
            1.0
        };
    }
    edit_distance(reference, got) as f64 / length as f64
}

/// Ignore differences that are not the recogniser's fault.
///
/// Line breaks are a layout decision: PP-OCRv6 joins its detected lines with `\n` while
/// manga-ocr returns one run, so comparing them raw would score the *pipeline shape*
/// rather than the reading. Whitespace goes the same way — Japanese does not use it, and
/// where an engine emits it is arbitrary.
///
/// # Ellipsis, and why it earns a rule of its own
///
/// The engines disagree about how to spell a pause. manga-ocr writes `...`, PP-OCRv6 writes
/// `…`, and Japanese typesetting proper uses `……`. All three are the same reading, and
/// counting them as different is not a small error: on `それで…` the ASCII form costs one
/// substitution and two insertions against four characters — **a CER of 0.75 for a perfect
/// read**.
///
/// Measured on the corpus before this existed: 27 of 97 samples carried ASCII dots and 22
/// carried `…`, so **28% of the material** was exposed to it. Ground truth written by a
/// person reading the image uses `…`, so the whole of that penalty landed on one engine, for
/// a convention rather than a misreading.
///
/// Runs collapse to a single `…` so `……` and `…` also agree. `。` is deliberately not
/// included: it ends a sentence and is a character in its own right.
/// # Character width, for the same reason
///
/// A full-width `４５` and an ASCII `45` are the same reading. The engines do not agree about
/// which to emit, and neither does a person transcribing: the two are **indistinguishable in
/// the image**, because the glyph a Japanese font draws for a digit inside kanji text is the
/// wide one either way. Scoring them apart does not measure recognition, it measures a font
/// convention — and it makes whoever wrote the ground truth silently pick a winner.
///
/// Measured on the local corpus: 13 samples carry half-width digits or Latin in engine output
/// and 7 carry the wide forms — and on **5 pairs the two engines read identical text** and were
/// counted as disagreeing for this alone.
///
/// `U+FF01`-`U+FF5E` is exactly the ASCII range's wide twin, offset by `0xFEE0`. The
/// ideographic space `U+3000` needs no arm: it is whitespace, and whitespace is already gone.
/// Folding runs *before* the ellipsis test, which is why `．` no longer appears in it — it
/// arrives there as `.` and the existing rule catches it.
/// The wave dash is the same problem wearing a second codepoint. `U+301C` 〜 and `U+FF5E` ～
/// are drawn alike and used alike; which one a system emits is a platform accident, and the
/// engines split on it. Measured here: 4 samples carry `U+301C`, 3 carry `U+FF5E`, and **6
/// disagree only about which** — more than the digits cost. Width folding alone would have made
/// that worse, since it sends `U+FF5E` to ASCII `~` and leaves `U+301C` where it was.
///
/// `・` is the one that cannot simply be folded, and the rule is the run.
///
/// A lone `U+30FB` between words is a **separator** and part of the reading: `中2・夜守コウ`,
/// `吸血鬼・七草ナズナ`. A run of them is an **ellipsis** written the way a lot of Japanese
/// typesetting writes it. Folding all of them would destroy real text; folding none of them
/// charges an engine three substitutions for a punctuation convention.
///
/// Counted on the local corpus before the rule was written: one run of `・・・`, and two lone
/// `・` — both of them genuine separators. So the run test never touches the separators here,
/// and it is the distinction the language itself makes rather than a threshold picked to fit.
///
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_ellipsis = false;
    // 中黒 seen since the last other character, so a run can be told from a separator.
    let mut nakaguro = 0usize;
    for c in text.chars() {
        if c.is_whitespace() {
            continue;
        }
        if c == '\u{30fb}' {
            nakaguro += 1;
            continue;
        }
        flush_nakaguro(&mut out, &mut nakaguro, &mut in_ellipsis);
        // Wide ASCII first, so every rule below sees one spelling. See the note above.
        let c = match c {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
            // The wave dash joins them, or width folding would drive the two spellings apart.
            '\u{301c}' => '~',
            other => other,
        };
        // U+2025 ‥ is the other spelling OCR emits; ． has already become `.` by here.
        if matches!(c, '.' | '\u{2025}' | '\u{2026}') {
            if !in_ellipsis {
                out.push('\u{2026}');
                in_ellipsis = true;
            }
        } else {
            out.push(c);
            in_ellipsis = false;
        }
    }
    flush_nakaguro(&mut out, &mut nakaguro, &mut in_ellipsis);
    out
}

/// Emit whatever the pending 中黒 turned out to be, and forget them.
fn flush_nakaguro(out: &mut String, pending: &mut usize, in_ellipsis: &mut bool) {
    match *pending {
        0 => {}
        1 => {
            out.push('\u{30fb}');
            *in_ellipsis = false;
        }
        _ => {
            if !*in_ellipsis {
                out.push('\u{2026}');
                *in_ellipsis = true;
            }
        }
    }
    *pending = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run is a pause; a single one joins two names. See `normalize`'s note.
    #[test]
    fn a_run_of_nakaguro_is_an_ellipsis_but_one_is_a_separator() {
        assert_eq!(normalize("あ・・・い"), "あ…い");
        assert_eq!(normalize("もっと精進します・・・"), "もっと精進します…");
        // The two real separators in the corpus, which must survive untouched.
        assert_eq!(normalize("中2・夜守コウ"), "中2・夜守コウ");
        assert_eq!(normalize("吸血鬼・七草ナズナ"), "吸血鬼・七草ナズナ");
        // And it agrees with the other spellings of the same pause.
        assert_eq!(normalize("あ・・・い"), normalize("あ…い"));
        assert_eq!(normalize("あ・・い"), normalize("あ...い"));
    }

    /// Two codepoints, one mark. See `normalize`'s note.
    #[test]
    fn both_wave_dashes_read_alike() {
        assert_eq!(normalize("あ〜"), normalize("あ～"));
        assert_eq!(normalize("あ〜"), "あ~");
        assert_eq!(
            character_error_rate(&normalize("ず〜っと"), &normalize("ず～っと")),
            0.0
        );
    }

    /// Width is a font convention rather than a reading. See `normalize`'s note.
    #[test]
    fn wide_ascii_reads_as_ascii() {
        assert_eq!(normalize("４５"), "45");
        assert_eq!(normalize("ＤＬ版"), "DL版");
        // The shape that cost five samples a disagreement they had not earned.
        assert_eq!(normalize("第４５期"), normalize("第45期"));
        // Composed the way `ocr_corpus.rs` composes them: `normalize` is the caller's job,
        // and scoring raw strings is not what the harness does.
        assert_eq!(
            character_error_rate(&normalize("第４５期"), &normalize("第45期")),
            0.0
        );
    }

    /// The ideographic space is whitespace, and whitespace was already gone.
    #[test]
    fn the_wide_space_is_still_just_a_space() {
        assert_eq!(normalize("気持ち　伝わる"), "気持ち伝わる");
    }

    /// Folding runs first, so a wide full stop reaches the ellipsis rule as `.`.
    #[test]
    fn the_wide_full_stop_still_becomes_an_ellipsis() {
        assert_eq!(normalize("あ．．．"), "あ…");
    }

    #[test]
    fn identical_strings_cost_nothing() {
        assert_eq!(
            edit_distance("気持ちが伝わってきます", "気持ちが伝わってきます"),
            0
        );
        assert_eq!(
            character_error_rate("気持ちが伝わってきます", "気持ちが伝わってきます"),
            0.0
        );
    }

    #[test]
    fn distance_counts_characters_and_not_bytes() {
        // One kanji swapped for another. Three bytes differ; one character does, and a
        // byte-wise distance would call this three times worse than it is.
        assert_eq!(edit_distance("日本語", "日本誤"), 1);
        assert!((character_error_rate("日本語", "日本誤") - 1.0 / 3.0).abs() < 1e-12);
    }

    /// The real case from the corpus: the two engines spelling one pause three ways.
    #[test]
    fn the_three_spellings_of_an_ellipsis_are_one_reading() {
        let manga = normalize("それで...");
        let paddle = normalize("それで…");
        let typeset = normalize("それで……");
        assert_eq!(manga, paddle);
        assert_eq!(manga, typeset);
        // And the point of it: a perfect read must not be scored as three quarters wrong.
        assert_eq!(character_error_rate(&paddle, &manga), 0.0);
    }

    #[test]
    fn a_full_stop_is_not_an_ellipsis() {
        // 。 ends a sentence. Folding it away would hide a real difference between an engine
        // that read the punctuation and one that invented it.
        assert_eq!(normalize("見た。"), "見た。");
        assert_ne!(normalize("見た。"), normalize("見た…"));
    }

    #[test]
    fn ellipsis_folding_does_not_swallow_the_text_around_it() {
        assert_eq!(normalize("あ...い"), "あ…い");
        assert_eq!(normalize("...あ"), "…あ");
        assert_eq!(normalize("あ..."), "あ…");
        // Separate runs stay separate: one pause is not two.
        assert_eq!(normalize("あ…い…う"), "あ…い…う");
    }

    #[test]
    fn each_kind_of_edit_costs_one() {
        assert_eq!(edit_distance("あい", "あいう"), 1, "insertion");
        assert_eq!(edit_distance("あいう", "あい"), 1, "deletion");
        assert_eq!(edit_distance("あいう", "あXう"), 1, "substitution");
    }

    #[test]
    fn an_empty_reference_is_not_a_division_by_zero() {
        assert_eq!(character_error_rate("", ""), 0.0);
        assert_eq!(character_error_rate("", "見えた"), 1.0);
    }

    #[test]
    fn reading_nothing_costs_the_whole_reference() {
        assert_eq!(character_error_rate("日本語", ""), 1.0);
    }

    #[test]
    fn runaway_output_scores_worse_than_silence() {
        // A hallucinating VLM is worse than an engine that returned nothing, and the rate
        // has to be able to say so.
        let noisy = character_error_rate("あ", "あいうえおかきくけこ");
        assert!(
            noisy > 1.0,
            "expected worse than total silence, got {noisy}"
        );
    }

    #[test]
    fn normalizing_drops_layout_but_not_content() {
        assert_eq!(
            normalize("気持ちが\n伝わって  きます"),
            "気持ちが伝わってきます"
        );
        assert_eq!(
            character_error_rate(
                &normalize("気持ちが伝わってきます"),
                &normalize("気持ちが\n伝わってきます")
            ),
            0.0,
            "where an engine breaks lines is not a reading error"
        );
    }
}
