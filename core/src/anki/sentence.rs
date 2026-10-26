//! The sentence a card carries, in the markup Anki's furigana filters read.
//!
//! Two jobs, and both used to be done wrong or not at all. The card was sent
//! `analysis.text` — *everything* that had been analysed — under a comment claiming the
//! sentence travelled with the word, so a paragraph of manga put the whole paragraph on every
//! card made from it. And the reading was dropped entirely, leaving a wall of kanji on the
//! front of a card made by a tool that had the readings in hand.

use crate::dict::{furigana, KanjiDic};
use crate::model::{sentence_bounds, Token};

/// One token as Anki furigana markup, appended to `out`.
///
/// Anki reads `気持[きも]ち` and renders きも over 気持 alone. Its regex for that is roughly
/// `" ?([^ >]+?)\[(.+?)\]"`, and two details of it decide everything here:
///
/// - **The base is every non-space character before the bracket**, so `私は日本[にほん]` would
///   put にほん over the whole of `私は日本`. A separator is required, and the leading ` ?` in
///   the pattern means Anki eats it again — so the space costs nothing in the rendered card and
///   is invisible through `{{kanji:}}` too.
/// - **`>` cannot appear in a base.** That is what lets the target word be wrapped in `<b>`
///   without breaking the reading: the `>` of the opening tag terminates the scan as surely as
///   a space would, so no separator is needed after it.
fn write_token(out: &mut String, token: &Token, dic: &KanjiDic) {
    for run in furigana(&token.surface, &token.reading, dic) {
        match run.reading {
            Some(reading) => {
                // Only where the previous character could otherwise be read as part of the
                // base. After `>` or a space it already cannot be, and adding another would
                // put a space in the sentence that the writer did not.
                if !matches!(out.chars().last(), None | Some(' ') | Some('>')) {
                    out.push(' ');
                }
                out.push_str(&run.text);
                out.push('[');
                out.push_str(&reading);
                out.push(']');
            }
            None => out.push_str(&run.text),
        }
    }
}

/// The sentence around `tokens[index]`, with that token in `<b>` and readings in Anki's markup.
///
/// Returns an empty string when the index is out of range rather than panicking: this is fed by
/// an IPC call, and a card not made is a better answer than a window that disappears.
pub fn markup(tokens: &[Token], index: usize, dic: &KanjiDic) -> String {
    if index >= tokens.len() {
        return String::new();
    }

    let (start, end) = sentence_bounds(tokens, index);
    let mut out = String::new();
    for (at, token) in tokens[start..end]
        .iter()
        .enumerate()
        .map(|(n, t)| (start + n, t))
    {
        if at == index {
            out.push_str("<b>");
            write_token(&mut out, token, dic);
            out.push_str("</b>");
        } else {
            write_token(&mut out, token, dic);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineBreak;

    /// `KanjiDic::load` on a path that does not exist gives an empty table, which is the state
    /// of a machine that has not installed KANJIDIC2 — and the state most of these tests want,
    /// because `furigana` then answers whole-word and the markup is predictable.
    fn no_kanji_table() -> KanjiDic {
        KanjiDic::load(std::path::Path::new("does-not-exist.tsv"))
    }

    fn token(surface: &str, reading: &str) -> Token {
        Token {
            surface: surface.into(),
            reading: reading.into(),
            ..Token::default()
        }
    }

    #[test]
    fn the_card_gets_one_sentence_and_not_the_whole_passage() {
        let tokens = vec![
            token("猫", "ねこ"),
            token("だ", ""),
            token("。", ""),
            token("犬", "いぬ"),
            token("も", ""),
            token("。", ""),
        ];
        // The word is in the second sentence, so the first must not come with it.
        let out = markup(&tokens, 3, &no_kanji_table());
        assert!(!out.contains('猫'), "the other sentence came too: {out}");
        assert!(out.contains('犬'), "its own sentence is missing: {out}");
        assert!(out.ends_with('。'), "the terminator closes it: {out}");
    }

    #[test]
    fn a_terminator_closes_its_own_sentence_rather_than_opening_the_next() {
        let tokens = vec![token("猫", "ねこ"), token("。", ""), token("犬", "いぬ")];
        let out = markup(&tokens, 1, &no_kanji_table());
        assert!(out.contains('猫'), "the terminator belongs with 猫: {out}");
        assert!(!out.contains('犬'), "it does not open the next: {out}");
    }

    #[test]
    fn text_with_no_terminator_is_one_sentence() {
        let tokens = vec![token("水", "みず"), token("を", ""), token("飲む", "のむ")];
        let out = markup(&tokens, 0, &no_kanji_table());
        assert!(out.contains('水') && out.contains("飲む"), "{out}");
    }

    #[test]
    fn the_target_is_marked_and_nothing_else_is() {
        let tokens = vec![token("水", "みず"), token("を", ""), token("飲む", "のむ")];
        let out = markup(&tokens, 2, &no_kanji_table());
        assert_eq!(out.matches("<b>").count(), 1, "one mark only: {out}");
        assert!(out.contains("<b>飲む[のむ]</b>"), "{out}");
    }

    /// The separator Anki's own filter eats again. Without it the base is every non-space
    /// character before the bracket, and のむ would be claimed by the whole of `を飲む`.
    ///
    /// The target is the *first* token here on purpose: with it on 飲む the mark's own `</b>`
    /// would supply the separation and this would pass without the rule existing.
    #[test]
    fn a_reading_is_separated_from_the_kana_before_it() {
        let tokens = vec![token("水", "みず"), token("を", ""), token("飲む", "のむ")];
        let out = markup(&tokens, 0, &no_kanji_table());
        assert!(out.contains("を 飲む[のむ]"), "no separator: {out}");
    }

    /// `>` ends a base as surely as a space, so the mark supplies the separation itself.
    #[test]
    fn the_mark_supplies_its_own_separation() {
        let tokens = vec![token("を", ""), token("飲む", "のむ")];
        let out = markup(&tokens, 1, &no_kanji_table());
        assert!(out.contains("<b>飲む[のむ]"), "a space crept in: {out}");
    }

    #[test]
    fn a_sentence_with_no_kanji_carries_no_brackets() {
        let tokens = vec![token("そう", "そう"), token("だ", "")];
        let out = markup(&tokens, 0, &no_kanji_table());
        assert!(!out.contains('['), "nothing to annotate: {out}");
        assert_eq!(out, "<b>そう</b>だ");
    }

    #[test]
    fn an_index_past_the_end_makes_no_card_rather_than_a_panic() {
        assert_eq!(markup(&[token("猫", "ねこ")], 9, &no_kanji_table()), "");
        assert_eq!(markup(&[], 0, &no_kanji_table()), "");
    }

    /// The card's sentence has to be the reader's sentence, so this asserts they are produced by
    /// the same function rather than by two lists that happen to match today. They did not match:
    /// this module split on `。！？!?` while [`sentence_bounds`] split on `。！？`, so `Nice!すごい。`
    /// split for the card and not for the screen.
    ///
    /// **A sentence ends two ways, and this loops over both.** It used to iterate characters,
    /// `'\n'` among them, on the belief that a line break arrives as a token of its own. It does
    /// not: no analyzer emits one, which is why the break is now a field on the token it follows.
    /// `core/tests/sentence_lines.rs` proves that against both analyzers by running them.
    #[test]
    fn the_card_splits_where_the_reader_splits() {
        enum Ends {
            Written(char),
            Break,
        }

        for ending in [
            Ends::Written('。'),
            Ends::Written('！'),
            Ends::Written('？'),
            Ends::Written('!'),
            Ends::Written('?'),
            Ends::Break,
        ] {
            let (middle, says) = match ending {
                Ends::Written(mark) => {
                    let mut piece = token(&mark.to_string(), "");
                    piece.line_break = LineBreak::None;
                    (piece, format!("`{mark}`"))
                }
                Ends::Break => {
                    // The line ended here. Nothing is written; the token records it.
                    let mut piece = token("これ", "これ");
                    piece.line_break = LineBreak::Break;
                    (piece, "a line break".to_string())
                }
            };
            let tokens = [token("これ", "これ"), middle, token("それ", "それ")];
            assert_eq!(
                sentence_bounds(&tokens, 0),
                (0, 2),
                "{says} must close the sentence it follows"
            );
            assert_eq!(
                sentence_bounds(&tokens, 2),
                (2, 3),
                "{says} must open a new one after it"
            );
        }
    }
}
