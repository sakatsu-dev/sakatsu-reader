//! The grammar a token holds inside it, for the reader.
//!
//! # Why
//!
//! A rule matches whole tokens ([`matcher`](super::matcher)), so it can only find what the
//! analyzer left as tokens of their own. The two analyzers draw those lines in different places.
//! The built-in one reads 来そうにない in pieces and a rule for そうにない fires; Ichiran reads
//! it as one word whose parts are 来 and そうにない, and no run of whole tokens spells そうにない. Ichiran likewise hands over よね, だけど and なんだ as single tokens, where
//! the ね, the けど and the explanatory なん cannot be reached. The same sentence was explained
//! differently depending on which engine read it, and the engine the app prefers explained less.
//!
//! # Where the boundaries come from
//!
//! Never from searching the text. Two sources, each a boundary something else established:
//!
//! - **a compound's own components**, which the analyzer itself split it into;
//! - **[`FUSED`]**, a short table of function words that arrive fused, each divided where the
//!   language divides it, and applied only to a token the analyzer classed as a function word
//!   (a particle, auxiliary, conjunction or expression) and gave no parts of its own.
//!
//! # Which parts of a compound count
//!
//! A match counts when it ends on a part that is itself a grammar point standing as written: not
//! the head word, spelled as its own dictionary form, not inflected, and not a bare particle. That
//! is the そうにない of 来そうにない, and the そう of 降り + そう, where the rule also reads the
//! stem before it. Everything else a compound holds is already explained or is not what it looks
//! like — the と of として and the に of によって belong to a fixed expression, the って of
//! 連れてって is 行って contracted — and a compound whose 〜ていく-style auxiliary the word
//! breakdown explains ([`super::compounds`]) is left to it entirely.
//!
//! # Words an analyzer splits
//!
//! The other direction. [`SINGLE_WORDS`] are words the built-in analyzer cuts into pieces that
//! look like particles — なんとか as なん / と / か — and any match lying wholly inside one is
//! dropped from the reader's view: the と and か as particles, and the と + か that a rule for
//! とか would otherwise read there. They are vocabulary, not grammar: the dictionary answers for
//! the whole word, and the か of なんとか asks no question. [`OPENING_WORDS`] are the same, but
//! only where they open a sentence: Ichiran reads the sigh はぁ as は / ぁ.
//!
//! # What it never does
//!
//! - **Explain a token twice.** A token any whole-token rule already covers is left as that rule
//!   explains it.
//! - **Feed anything but the reader.** [`super::patterns`] is unchanged, because lexical grouping
//!   and the expression offer build units and anchors on its rule elements. A match placed here
//!   carries no elements — the same convention as a structural match — so nothing can join or
//!   anchor to it.

use crate::model::Token;

use super::matcher::{self, Match};
use super::pack;

/// One fused function word: its spelling, the grammar points inside it, and whether they hold
/// only after a content word.
pub struct Fused {
    pub surface: &'static str,
    /// Each part that is a grammar point, with the spelling that part has inside the word. Parts
    /// are located left to right. Only parts with an entry of their own are listed: the わ of わね
    /// and the な of よな have none, so nothing is claimed for them.
    pub parts: &'static [(&'static str, &'static str)],
    /// The explanatory なん explains *something*: 貧血なんです. Opening a sentence, or after a
    /// particle, the same fused word is 何です — なんですか、この… is "what is this", and
    /// nothing is being explained.
    pub after_content: bool,
}

/// Function words that arrive as one token. These are the fused forms Ichiran was observed
/// producing, on the capture corpus and on the Tatoeba sentences in `tests/grammar_parity.rs`; a
/// form is added when it is observed, not because it might occur.
pub const FUSED: &[Fused] = &[
    Fused {
        surface: "よね",
        parts: &[("particle-yo", "よ"), ("particle-ne", "ね")],
        after_content: false,
    },
    Fused {
        surface: "だよね",
        parts: &[("particle-yo", "よ"), ("particle-ne", "ね")],
        after_content: false,
    },
    Fused {
        surface: "わね",
        parts: &[("particle-ne", "ね")],
        after_content: false,
    },
    Fused {
        surface: "よな",
        parts: &[("particle-yo", "よ")],
        after_content: false,
    },
    Fused {
        surface: "のか",
        parts: &[("particle-no", "の"), ("particle-ka", "か")],
        after_content: false,
    },
    Fused {
        surface: "なの",
        parts: &[("particle-no", "の")],
        after_content: false,
    },
    Fused {
        surface: "だけど",
        parts: &[("kedo", "けど")],
        after_content: false,
    },
    Fused {
        surface: "だっけ",
        parts: &[("hanabira-n3-kke", "っけ")],
        after_content: false,
    },
    Fused {
        surface: "なんだ",
        parts: &[(super::EXPLANATORY_NAN, "なん")],
        after_content: true,
    },
    Fused {
        surface: "なんです",
        parts: &[(super::EXPLANATORY_NAN, "なん")],
        after_content: true,
    },
];

/// Words the built-in analyzer splits into particle-shaped pieces, each one word by evidence:
/// UD Japanese-GSD's long-unit layer annotates なんとか (何とか), 少なくとも, 特に and 既に as single
/// units, and JMdict records もしかして, ざっと and いくらでも (幾らでも) as single adverbs (GSD has
/// no occurrence of them). A spelling joins only with such evidence.
pub const SINGLE_WORDS: &[&str] = &[
    "なんとか",
    "何とか",
    "少なくとも",
    "特に",
    "既に",
    "もしかして",
    "ざっと",
    "いくらでも",
];

/// Words that are one word where they open a sentence, and may be something else elsewhere. The
/// sigh はぁ is an interjection to UniDic and JMdict alike (はあ), but Ichiran splits it, and its
/// は became the topic particle. After a word, 私はぁ… is that particle drawn out, so it stays.
pub const OPENING_WORDS: &[&str] = &["はぁ"];

/// Patterns inside tokens that `outer` — the whole-token matches — leaves unexplained.
pub fn find(tokens: &[Token], outer: &[Match]) -> Vec<Match> {
    let packs = pack::all();
    let mut found = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if outer
            .iter()
            .any(|claimed| claimed.first_token <= index && index < claimed.last_token_end)
        {
            continue;
        }
        if token.components.len() >= 2 {
            // A compound that is itself one of these words — the built-in analyzer groups 何とか
            // as 何 + と + か — holds no grammar to find.
            if SINGLE_WORDS.contains(&token.surface.as_str()) {
                continue;
            }
            if !super::compounds::detect(token).is_empty() {
                continue;
            }
            for within in matcher::find_across(&token.components, &packs) {
                if counts(&token.components, &within) {
                    found.push(placed(index, within.pattern_id, within.start, within.end));
                }
            }
        } else if token.components.is_empty() && pack::is_function_word(token) {
            let Some(fused) = FUSED.iter().find(|fused| fused.surface == token.surface) else {
                continue;
            };
            if fused.after_content && !pack::follows_word(tokens, index) {
                continue;
            }
            for (pattern, start, end) in fused_parts(token, fused) {
                found.push(placed(index, pattern.to_owned(), start, end));
            }
        }
    }
    found
}

/// Whether a match among a compound's parts ends on a grammar point standing as written: a part
/// that is not the head word, spelled as its own dictionary form, not inflected, and not matched
/// as a bare particle.
fn counts(parts: &[Token], within: &Match) -> bool {
    let last = within.last_token_end - 1;
    if last == 0 {
        return false;
    }
    let part = &parts[last];
    part.dict_form == part.surface
        && part.conjugation.is_none()
        && !within.pattern_id.starts_with("particle-")
}

/// Drop each match that lies wholly inside one run of whole tokens spelling one of
/// [`SINGLE_WORDS`], or one of [`OPENING_WORDS`] where it opens a sentence.
pub fn without_split_words(tokens: &[Token], found: Vec<Match>) -> Vec<Match> {
    // Which split word, if any, each token belongs to. Runs never overlap: each spells a whole
    // word, and the scan resumes after the run it found.
    let mut word_of = vec![None; tokens.len()];
    let mut start = 0;
    while start < tokens.len() {
        let words: Vec<&str> = if pack::opens_sentence(tokens, start) {
            SINGLE_WORDS.iter().chain(OPENING_WORDS).copied().collect()
        } else {
            SINGLE_WORDS.to_vec()
        };
        let mut spelled = String::new();
        let mut run_end = None;
        for (end, token) in tokens.iter().enumerate().skip(start) {
            spelled.push_str(&token.surface);
            if !words.iter().any(|word| word.starts_with(spelled.as_str())) {
                break;
            }
            if end > start && words.contains(&spelled.as_str()) {
                run_end = Some(end);
            }
        }
        match run_end {
            Some(end) => {
                word_of[start..=end]
                    .iter_mut()
                    .for_each(|word| *word = Some(start));
                start = end + 1;
            }
            None => start += 1,
        }
    }
    found
        .into_iter()
        .filter(|m| {
            let word = word_of[m.first_token];
            word.is_none() || !(m.first_token..m.last_token_end).all(|index| word_of[index] == word)
        })
        .collect()
}

/// A match on the token at `index`, spanning exactly the part it was found in.
fn placed(index: usize, pattern_id: String, start: Option<usize>, end: Option<usize>) -> Match {
    Match {
        pattern_id,
        first_token: index,
        last_token_end: index + 1,
        start,
        end,
        // No rule over whole tokens ran here, so there are no rule positions to report.
        elements: Vec::new(),
    }
}

/// The grammar points inside a fused function word, with each part's span in the source.
///
/// A span is given only when the token's surface is exactly the source it is grounded on;
/// otherwise the part is still reported, against the token, without one.
fn fused_parts(token: &Token, fused: &Fused) -> Vec<(&'static str, Option<usize>, Option<usize>)> {
    let grounded = match (token.start, token.end) {
        (Some(start), Some(end)) if end.checked_sub(start) == Some(token.surface.len()) => {
            Some(start)
        }
        _ => None,
    };
    let mut from = 0;
    let mut placed = Vec::new();
    for (pattern, spelling) in fused.parts.iter() {
        let Some(offset) = token.surface[from..].find(spelling) else {
            continue;
        };
        let at = from + offset;
        from = at + spelling.len();
        if pack::find(pattern).is_none() {
            continue;
        }
        let (start, end) = match grounded {
            Some(base) => (Some(base + at), Some(base + from)),
            None => (None, None),
        };
        placed.push((*pattern, start, end));
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Sense;

    fn word(surface: &str, start: usize, pos: &str) -> Token {
        Token {
            surface: surface.into(),
            dict_form: surface.into(),
            start: Some(start),
            end: Some(start + surface.len()),
            senses: if pos.is_empty() {
                Vec::new()
            } else {
                vec![Sense {
                    pos: pos.into(),
                    ..Default::default()
                }]
            },
            ..Default::default()
        }
    }

    fn ids(found: &[Match]) -> Vec<&str> {
        found.iter().map(|m| m.pattern_id.as_str()).collect()
    }

    /// Every table entry names patterns that exist, and parts its spelling contains in order —
    /// a typo here would otherwise be a grammar point that silently never appears.
    #[test]
    fn every_fused_entry_names_real_patterns_and_parts_it_contains() {
        for fused in FUSED {
            let mut from = 0;
            for (pattern, spelling) in fused.parts.iter() {
                let surface = fused.surface;
                assert!(
                    pack::find(pattern).is_some(),
                    "{surface}: no entry {pattern}"
                );
                let at = surface[from..]
                    .find(spelling)
                    .unwrap_or_else(|| panic!("{surface} does not contain {spelling} in order"));
                from += at + spelling.len();
            }
        }
    }

    fn compound(surface: &str, parts: Vec<Token>) -> Token {
        let mut whole = word(surface, 0, "");
        whole.components = parts;
        whole
    }

    /// The と of として belongs to a fixed expression. Claiming it would tell a reader the
    /// quotative or "with" particle is there.
    #[test]
    fn a_bare_particle_inside_a_compound_is_not_claimed() {
        let toshite = compound(
            "として",
            vec![word("と", 0, "[prt]"), word("して", 3, "[vs]")],
        );
        let nidatte = compound(
            "によって",
            vec![word("に", 0, "[prt]"), word("よって", 3, "")],
        );
        assert!(find(&[toshite, nidatte], &[]).is_empty());
    }

    /// 連れてって is 連れて + 行って contracted. Its って is a verb written short, so a rule for
    /// the quotative って must not be told it is there.
    #[test]
    fn a_part_that_is_not_spelled_as_its_own_word_is_not_claimed() {
        let mut contracted = word("って", 9, "");
        contracted.dict_form = "行く".into();
        let tsuretette = compound("連れてって", vec![word("連れて", 0, ""), contracted]);
        assert!(find(&[tsuretette], &[]).is_empty());
    }

    /// The head word is the vocabulary, not a grammar point inside the compound.
    #[test]
    fn the_head_of_a_compound_is_never_claimed() {
        let head = compound(
            "そうにないね",
            vec![word("そうにない", 0, ""), word("ね", 15, "")],
        );
        let found = find(&[head], &[]);
        assert!(
            found
                .iter()
                .all(|m| m.pattern_id != "hanabira-n1-verb-sou-ni-nai"),
            "{found:?}"
        );
    }

    fn particle(id: &str, at: usize) -> Match {
        Match {
            pattern_id: id.into(),
            first_token: at,
            last_token_end: at + 1,
            start: None,
            end: None,
            elements: Vec::new(),
        }
    }

    /// なんとか split as なん / と / か is one word, and its と and か are no particles. The same
    /// pieces spelling something else — なんと / かわいい, "what pretty…" — are left alone.
    #[test]
    fn a_particle_inside_a_split_single_word_is_dropped() {
        let split = [
            word("なん", 0, ""),
            word("と", 6, ""),
            word("か", 9, ""),
            word("なる", 12, ""),
        ];
        let kept = without_split_words(
            &split,
            vec![particle("particle-to", 1), particle("particle-ka", 2)],
        );
        assert!(kept.is_empty(), "{kept:?}");

        // A rule for とか reads と + か there too, and is just as wrong; one that runs on past the
        // word is about something else and stays.
        let mut toka = particle("hanabira-n2-to-ka", 1);
        toka.last_token_end = 3;
        let mut beyond = particle("past-the-word", 2);
        beyond.last_token_end = 4;
        let kept = without_split_words(&split, vec![toka, beyond]);
        assert_eq!(ids(&kept), ["past-the-word"]);

        let other = [
            word("なん", 0, ""),
            word("と", 6, ""),
            word("かわいい", 9, ""),
        ];
        let kept = without_split_words(&other, vec![particle("particle-to", 1)]);
        assert_eq!(ids(&kept), ["particle-to"]);
    }

    /// The built-in analyzer can group 何とか as one compound of 何 + と + か. It is still one
    /// word, and the と + か inside it is not the とか of listing.
    #[test]
    fn a_compound_that_is_a_single_word_is_not_divided() {
        let nantoka = compound(
            "何とか",
            vec![word("何", 0, ""), word("と", 3, ""), word("か", 6, "")],
        );
        assert!(find(&[nantoka], &[]).is_empty());
    }

    /// The sigh はぁ split as は / ぁ holds no topic particle where it opens a sentence. After a
    /// word, 私はぁ is the particle drawn out, and it stays.
    #[test]
    fn a_split_sigh_is_one_word_only_where_it_opens_a_sentence() {
        let sigh = [word("は", 0, "[prt]"), word("ぁ", 3, ""), word("。", 6, "")];
        let kept = without_split_words(&sigh, vec![particle("particle-wa", 0)]);
        assert!(kept.is_empty(), "{kept:?}");

        let drawn_out = [
            word("私", 0, "[pn]"),
            word("は", 3, "[prt]"),
            word("ぁ", 6, ""),
        ];
        let kept = without_split_words(&drawn_out, vec![particle("particle-wa", 1)]);
        assert_eq!(ids(&kept), ["particle-wa"]);
    }

    /// A pause opens the next thought: でも after ・・・ or … begins a sentence, after 、 it does not.
    #[test]
    fn an_ellipsis_opens_a_sentence_and_a_comma_does_not() {
        for mark in ["・・・", "…", "……"] {
            assert!(
                pack::opens_sentence(&[word(mark, 0, ""), word("でも", 9, "")], 1),
                "{mark}"
            );
        }
        assert!(!pack::opens_sentence(
            &[word("、", 0, ""), word("でも", 3, "")],
            1
        ));
    }

    /// The explanatory なん explains the word before it. Opening a sentence or after a particle,
    /// the fused なんです is 何です, "what is it", and explains nothing.
    #[test]
    fn the_explanatory_nan_needs_a_content_word_before_it() {
        let after_noun = [word("貧血", 0, "[n]"), word("なんです", 6, "[aux]")];
        assert_eq!(
            ids(&find(&after_noun, &[])),
            [crate::grammar::EXPLANATORY_NAN]
        );

        let opening = [
            word("。", 0, ""),
            word("なんです", 3, "[aux]"),
            word("か", 15, "[prt]"),
        ];
        assert!(
            find(&opening, &[]).is_empty(),
            "なんですか opening a sentence"
        );

        let after_particle = [
            word("それ", 0, "[pn]"),
            word("は", 6, "[prt]"),
            word("なんです", 9, "[aux]"),
        ];
        assert!(find(&after_particle, &[]).is_empty(), "それはなんです");
    }

    /// A compound's own parts are matched, and the match is reported on the compound with the
    /// span of the part it was found in.
    #[test]
    fn a_pattern_inside_a_compound_is_reported_on_the_compound_with_the_parts_span() {
        let mut compound = word("来そうにない", 0, "");
        compound.components = vec![word("来", 0, "[vk]"), word("そうにない", 3, "[exp,adj-i]")];
        let found = find(&[compound], &[]);
        assert_eq!(ids(&found), ["hanabira-n1-verb-sou-ni-nai"]);
        assert_eq!((found[0].first_token, found[0].last_token_end), (0, 1));
        assert_eq!((found[0].start, found[0].end), (Some(3), Some(18)));
        assert!(
            found[0].elements.is_empty(),
            "nothing may anchor to an inside match"
        );
    }

    /// A fused function word yields each listed part, at that part's own bytes.
    #[test]
    fn a_fused_function_word_yields_its_parts_at_their_own_spans() {
        let found = find(&[word("よね", 9, "[exp,prt]")], &[]);
        assert_eq!(ids(&found), ["particle-yo", "particle-ne"]);
        assert_eq!((found[0].start, found[0].end), (Some(9), Some(12)));
        assert_eq!((found[1].start, found[1].end), (Some(12), Some(15)));
    }

    /// The same spelling that is not a function word is not divided: the table is about
    /// particles, not about every token that happens to be spelled like one.
    #[test]
    fn a_token_that_is_not_a_function_word_is_never_divided() {
        assert!(find(&[word("よね", 0, "[n]")], &[]).is_empty());
        assert!(find(&[word("よね", 0, "")], &[]).is_empty());
    }

    /// A token a whole-token rule already explains is left alone.
    #[test]
    fn a_token_already_explained_is_not_explained_again() {
        let claimed = Match {
            pattern_id: "kedo".into(),
            first_token: 0,
            last_token_end: 1,
            start: Some(0),
            end: Some(9),
            elements: Vec::new(),
        };
        assert!(find(&[word("だけど", 0, "[conj]")], &[claimed]).is_empty());
    }
}
