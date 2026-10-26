//! Check an analyzer's readings against the dictionary that is already open.
//!
//! # Why this exists
//!
//! The built-in analyzer takes its readings from UniDic, whose token and lemma readings closed
//! most of the old IPADIC gap. Ichiran takes its readings from a JMdict-derived database and can
//! still settle some homographs from context — 者 is もの or しゃ depending on the word it is
//! part of, and 乳房 is ちぶさ or にゅうぼう.
//!
//! Measured over the 141-capture corpus, that difference is **five words and about seven
//! occurrences in 1,183 aligned tokens** — 0.6%. Small, and also the last thing separating the
//! two analyzers on readings once the punctuation and zero-width artifacts were removed.
//!
//! # Why the dictionary can settle it without being a second analyzer
//!
//! `project notes` §6 already says the merge policy promotes a dictionary where the analyzer has
//! nothing to say. This is the same idea one field over: the app ships Jitendex, Jitendex knows
//! every reading each spelling has, and a reading the dictionary has never heard of for that
//! spelling is a reading worth doubting.
//!
//! **It only ever corrects towards the dictionary, and only when the dictionary is unanimous.**
//! A spelling with two readings on record is exactly the case this cannot resolve — that is the
//! disambiguation Ichiran does with context, and guessing at it would replace a analyzer's
//! considered wrong answer with an arbitrary one.
//!
//! # It also says when it could not resolve one
//!
//! That unresolvable case is not nothing, and for a long time it was treated as nothing: counted
//! into `Corrections::ambiguous` and written to a log line nobody reads. Meanwhile the reader
//! printed the analyzer's pick in the same quiet grey as a reading three dictionaries agree on.
//!
//! `tests/heteronym_readings.rs` measures how often that pick is right: **66.4%** over the 220
//! spellings the analyzer still produces as tokens of their own. So the caveat
//! is now carried on the token as `reading_ambiguous`, and the reader draws it. This pass
//! corrects what it can and admits the rest.

use crate::dict::evidence::Evidence;
use crate::dict::index::Dictionary;
use crate::kana::to_hiragana;
use crate::model::{is_kana_free, Analysis, Token};

/// What a pass over one analysis changed, for logging and for tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Corrections {
    /// Readings replaced with the dictionary's.
    pub corrected: usize,
    /// Tokens the dictionary had an opinion about and agreed with.
    pub confirmed: usize,
    /// Tokens left alone because the dictionary listed more than one reading.
    pub ambiguous: usize,
}

/// Correct the readings in `analysis` against `dictionaries`, in place.
///
/// A no-op when nothing is installed, which is a supported state: a fresh install analyses
/// before it has downloaded anything, and the readings are then UniDic's alone.
pub fn correct(analysis: &mut Analysis, dictionaries: &[Dictionary]) -> Corrections {
    correct_in(analysis, Evidence::direct(dictionaries))
}

/// [`correct`], reading through one request's evidence.
pub fn correct_in(analysis: &mut Analysis, dictionaries: Evidence<'_>) -> Corrections {
    let mut counts = Corrections::default();
    if dictionaries.is_empty() {
        return counts;
    }

    for token in &mut analysis.tokens {
        // Kana needs no correcting: the surface *is* the reading, and a dictionary entry that
        // disagreed would be describing a different word.
        if token.surface.is_empty() || !token.surface.chars().any(is_kanji) {
            continue;
        }

        // **An inflected surface is not a headword**, which decides two different things below.
        let inflected = token.conjugation.is_some()
            || (!token.dict_form.is_empty() && token.dict_form != token.surface);

        // Which spelling the dictionary can be asked about. Looking up 見かけた finds nothing —
        // it is not an entry — so an inflected token is asked about its dictionary form instead.
        // That is a real question with a real answer: 開く is ひらく *or* あく, so a 開いた on
        // screen is a guess whichever way it went.
        let headword = if inflected {
            token.dict_form.clone()
        } else {
            token.surface.clone()
        };
        if headword.is_empty() || !headword.chars().any(is_kanji) {
            continue;
        }

        let Some(reading) = readings_for(&headword, dictionaries) else {
            // **Nothing lists this spelling, which is not the same as nothing being known.**
            // No dictionary has お姉さま, and the correction pass therefore had nothing to say
            // while composition's おあねさま stood. The evidence is one token to the left.
            //
            // **Marking the rest was tried here and rejected on its own numbers.** Flagging every
            // composed reading no dictionary could confirm is *true* — nothing vouches for them —
            // and over 4,000 Tatoeba sentences it marked 634 readings of which essentially all
            // were correct: 非常に, に対して, 一つ, お花見, 人たち, ３時間. That is the caveat rate
            // this project has already measured its way out of once, and the reason is written up
            // in `local documentation`: a mark on one word in three is a mark a reader
            // learns to ignore, and then it is not there where it is earned.
            if !inflected {
                if let Some(repaired) = composed_from_a_known_prefix(token, dictionaries) {
                    token.reading = repaired;
                    counts.corrected += 1;
                }
            }
            continue;
        };
        match reading {
            // **Flagging is a weaker claim than correcting, so it runs where correcting cannot.**
            // Saying "this spelling has more than one reading" is true of the dictionary form
            // whatever the surface does with it; the caveat survives inflection where a
            // replacement reading would not.
            Settled::Several(candidates) => {
                // **A rival nobody uses is not a second reading.** This flagged any spelling the
                // dictionary listed twice, and JMdict lists near-dead alternatives for very common
                // words: 彼 is かれ at 200 beside かの at −1 and あれ at −101 "rarely used"; 道 is
                // みち at 200 beside どう and タオ at 0. Flagging those put the caveat on **32.9% of
                // kanji-bearing tokens** — a mark on one word in three, which a reader learns to
                // ignore, and then it is not there for 年 (とし 200, ねん 200) where it is earned.
                //
                // So a rival counts only if the dictionary ranks it near the best. Measured over
                // 1,500 sentences the rate falls to 12.3%, and the choice of margin barely matters:
                // 0 gives 9.7%, 50 gives 12.2%, 100 gives 12.3%, and only past 200 does it jump
                // back to 29.7% by re-admitting the score-0 alternatives. A plateau that wide means
                // this is a threshold on a real gap rather than a number tuned to a corpus.
                if is_contested(&candidates) {
                    token.reading_ambiguous = true;
                    counts.ambiguous += 1;
                }

                // **A reading that is not on the list was never a choice between the listed
                // ones.** Composition builds a compound's reading by joining its parts, so
                // お父さん arrives as おちちさん — 父 alone really is ちち. The caveat above says
                // "this spelling has several readings", and under it sat one the dictionary had
                // never offered, which makes the caveat read as though おちちさん were among them.
                //
                // Replacing it keeps the caveat and fixes what the caveat is about. The flag is
                // still true — おとうさん and おとっさん both exist — so nothing is claimed more
                // firmly than before; what changes is that the reading on screen is one a source
                // vouches for.
                //
                // Inflected surfaces are left alone for the same reason `One` leaves them: a
                // dictionary entry spelled like an inflected surface is usually a different word.
                // **Being listed is not the same as being this spelling's reading.** 修羅場 is
                // composed しゅらじょう, and the dictionary does list it — at -101, against しゅらば
                // at 0. The old test asked only whether the reading appeared at all, so a reading
                // the source itself marks as rare survived beside a live one, and the caveat did
                // not fire either because the gap is 101 and the margin is 100. It fell between two
                // tests.
                //
                // **Two conditions, and the first one is the load-bearing one.** A negative score is
                // the source saying *rarely used* — the same fact this file already relies on a few
                // lines down, where 彼 is かれ at 200 beside かの at -1 and あれ at -101. A merely
                // lower score is not that, and treating it as such was measured doing real damage:
                // over the two novel chapters a gap-only test rewrote 身体 からだ to しんたい and
                // 街 まち to がい, because Jitendex pairs a priority-tagged 200 with a perfectly live
                // 99 and the gap is 101 — one point past the margin, on some of the most ordinary
                // words there are.
                //
                // The margin is still required, and 肋骨 is why: ろっこつ is negative, at -1, against
                // あばらぼね at 0. A one-point gap is two live readings, not a dead one.
                let best = candidates.first().map(|(_, score)| *score).unwrap_or(0.0);
                let vouched = candidates
                    .iter()
                    .find(|(candidate, _)| to_hiragana(candidate) == to_hiragana(&token.reading))
                    .is_some_and(|(_, score)| *score >= 0.0 || best - score <= CONTESTED_WITHIN);
                if !inflected && !vouched {
                    if let Some((best, _)) = candidates.first() {
                        token.reading = best.clone();
                        counts.corrected += 1;
                    }
                }
            }
            // **Only a word standing in its dictionary form is corrected.** A dictionary entry
            // spelled like an inflected surface is usually a different lexeme: 撮り as a token is
            // the 連用形 of 撮る (とり), while the dictionary's 撮り is the 〜撮り suffix (どり).
            // Correcting one to the other is confident and wrong, and both of those were caught
            // doing exactly that.
            Settled::One(_) if inflected => {}
            Settled::One(correct) => {
                if to_hiragana(&correct) == to_hiragana(&token.reading) {
                    counts.confirmed += 1;
                } else {
                    token.reading = correct;
                    counts.corrected += 1;
                }
            }
        }
    }
    counts
}

enum Settled {
    One(String),
    /// Several readings for one spelling, with their scores, the dictionary's best first.
    ///
    /// The list is carried rather than discarded because *which* readings exist decides whether
    /// the one on screen is a choice or an invention, and the **scores** decide whether the
    /// choice was real. See the `Several` arm in [`correct`].
    Several(Vec<(String, f64)>),
}

/// Join adjacent tokens when the dictionary spells the compound differently than its parts.
///
/// **Rendaku and counter readings are not the analyzer's to know.** 受付 is うけつけ and 係 is
/// かかり, and both are right — but 受付係 is うけつけ**が**かり, and no per-token reading can say
/// so, because the change is caused by the neighbour. The same for 株式 + 会社 → かぶしき**が**いしゃ,
/// 一 + 回 → **いっ**かい, 月曜 + 日 → げつよう**び**. Measured against KWDLC these were the largest
/// class of reading disagreement left after 日本 and the numerals.
///
/// The dictionary already knows every one of them. What was missing is that they arrive as two
/// tokens, so nothing ever asks.
///
/// **Two guards, and a run needs only one of them.** A run is joined when the dictionary has the
/// compound *and* either its reading differs from the parts' readings run together — the rendaku
/// case this started as — or the surface carries no hiragana at all.
///
/// The reading test alone was once the whole rule, on the reasoning that it "keeps this from
/// inventing compounds": 大学 + 生 is an entry too, but だいがく + せい already spells だいがくせい,
/// so nothing needed fixing. That was right about rendaku and wrong about *segmentation* — the
/// join 大学生 is the unit a learner marks known, whatever its reading does.
///
/// `ud_gold.rs` measured the wider promise this doc used to refuse. Over UD_Japanese-GSD, joining
/// any dictionary-known run with no hiragana in it was **99.8% not wrong**: one join in 607
/// crossed a gold boundary. Without the kana-free restriction the same rule was wrong half the
/// time, because それは and には are also things the dictionary knows.
///
/// Shaped after [`super::names::merge_names`], down to keeping the pieces in [`Token::components`]
/// so nothing the analyzer said is lost, and to asking the dictionary rather than trusting a shape.
pub fn merge_compounds(
    tokens: &[Token],
    reading_of: impl Fn(&str) -> Option<String>,
) -> Vec<Token> {
    // Three is enough for 株式会社 and 月曜日 and stops the search being quadratic on a passage.
    const MAX_RUN: usize = 3;

    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut at = 0;

    while at < tokens.len() {
        let mut joined = None;
        let longest = MAX_RUN.min(tokens.len() - at);
        // Longest first, so 月曜日 is not settled as 月曜 the moment that is also an entry.
        for length in (2..=longest).rev() {
            let run = &tokens[at..at + length];
            // Never across a gap: the offsets have to meet, the same guard composition uses.
            if run
                .windows(2)
                .any(|pair| pair[0].end.is_none() || pair[0].end != pair[1].start)
            {
                continue;
            }
            if run.iter().any(|token| token.reading.is_empty()) {
                continue;
            }
            let surface: String = run.iter().map(|token| token.surface.as_str()).collect();
            let Some(reading) = reading_of(&surface) else {
                continue;
            };
            let parts: String = run.iter().map(|token| token.reading.as_str()).collect();
            // **A changed reading is one licence to join; being kana-free is the other.**
            //
            // The reading test alone was the whole rule, and it declined 大学 + 生 because
            // だいがく + せい already spells だいがくせい — correctly, on the evidence available
            // when it was written. `ud_gold.rs` has since measured the wider promise it refused:
            // over UD_Japanese-GSD, joining any adjacent run the dictionary knows whose surface
            // carries **no hiragana** made 607 joins, of which 426 were gold long units exactly,
            // 180 fell inside a longer gold unit, and **one crossed a boundary**. 99.8% not wrong.
            //
            // The kana-free test is what buys that. Unrestricted, the same rule made 1494 joins
            // with 735 across a boundary — half of them wrong — because それは, には and のに are
            // all things the dictionary knows. Excluding hiragana removes that class by
            // construction rather than by a list, and leaves the nominal compounding this is for.
            if to_hiragana(&reading) == to_hiragana(&parts) && !is_kana_free(&surface) {
                continue;
            }
            joined = Some((length, surface, to_hiragana(&reading)));
            break;
        }

        match joined {
            Some((length, surface, reading)) => {
                let run = &tokens[at..at + length];
                let head = &run[0];
                out.push(Token {
                    surface,
                    reading: reading.clone(),
                    dict_form: run
                        .iter()
                        .map(|token| token.surface.as_str())
                        .collect::<String>(),
                    lemma_reading: reading,
                    start: head.start,
                    end: run[length - 1].end,
                    components: run.to_vec(),
                    ..Default::default()
                });
                at += length;
            }
            None => {
                out.push(tokens[at].clone());
                at += 1;
            }
        }
    }

    out
}

/// How far below the best a reading may score and still count as a rival.
///
/// 200 is the dictionary's mark for a common form and 0 or below is where the archaic and the
/// "rarely used" sit, so anything inside 100 of the best is a reading somebody actually uses.
const CONTESTED_WITHIN: f64 = 100.0;

/// Whether this spelling is genuinely contested, or merely lists something nobody says.
fn is_contested(candidates: &[(String, f64)]) -> bool {
    let Some((_, best)) = candidates.first() else {
        return false;
    };
    candidates
        .iter()
        .skip(1)
        .any(|(_, score)| best - score <= CONTESTED_WITHIN)
}

/// Every reading the dictionary lists for `surface`, its own best first, with its scores.
///
/// **A window onto [`readings_for`], not a second implementation of it.** The question
/// `heteronym_readings.rs` has to ask is whether the reading a sentence calls for is the one the
/// dictionary already ranks highest — and answering that from a fresh `lookup` would mean a second
/// opinion about which hits count as readings. Two of those disagreeing is how the number that
/// justifies a rule ends up computed by something other than the rule.
///
/// Empty when the spelling has no listed reading; one entry when it is settled.
pub fn candidates(surface: &str, dictionaries: &[Dictionary]) -> Vec<(String, f64)> {
    match readings_for(surface, Evidence::direct(dictionaries)) {
        None => Vec::new(),
        // `One` discards the score on its way out of `readings_for`, and nothing here needs it:
        // a spelling with a single reading has no ranking to be right or wrong about.
        Some(Settled::One(reading)) => vec![(reading, 0.0)],
        Some(Settled::Several(candidates)) => candidates,
    }
}

/// The reading every dictionary agrees this spelling has, when there is exactly one.
///
/// Matched on the spelling alone. A reading-bucket hit would find every homophone of the
/// query — が finding 蛾, 我 and 雅 — and pick one of their readings, which is the opposite of
/// what this is for.
/// Whether this token's reading is just its parts run together.
///
/// `compose_nominal` and `compose_digits` build a compound's reading by concatenation, which is a
/// guess about the whole that its parts cannot make. A reading that is *not* the concatenation came
/// from somewhere else — an analyzer that read the whole, or an earlier pass — and is not this
/// function's business.
fn invented_by_composition(token: &Token) -> bool {
    if token.components.len() < 2 {
        return false;
    }
    let joined: String = token
        .components
        .iter()
        .map(|part| part.reading.as_str())
        .collect();
    !joined.is_empty() && joined == token.reading
}

/// A composed reading rebuilt around the longest prefix of its parts the dictionary does know.
///
/// **The address forms are the case this exists for.** お姉さま is composed お + 姉 + さま, and 姉
/// on its own really is あね, so the parts give おあねさま. No dictionary lists お姉さま, so the
/// correction pass had nothing to say and the invented reading stood.
///
/// But the evidence is there one token to the left: **お姉 is an entry, and it reads おねえ**. The
/// address-form reading of the base is what the prefixed spelling records, which is exactly the
/// thing the bare base cannot tell you. Ichiran reaches the same answer by splitting there.
///
/// Longest prefix first, so a longer known spelling wins over a shorter one inside it. The prefix's
/// reading must be **well supported** — [`is_contested`] again, the same test the ambiguity mark
/// uses — because a prefix with two live readings is not evidence about which one the whole takes.
/// Every remaining part keeps its own reading, and a part with none stops this: a reading assembled
/// around a hole is not an improvement on a reading assembled from parts.
fn composed_from_a_known_prefix(token: &Token, dictionaries: Evidence<'_>) -> Option<String> {
    if !invented_by_composition(token) {
        return None;
    }
    for take in (2..token.components.len()).rev() {
        let (head, rest) = token.components.split_at(take);
        let prefix: String = head.iter().map(|part| part.surface.as_str()).collect();
        if !prefix.chars().any(is_kanji) {
            continue;
        }
        let candidates = match readings_for(&prefix, dictionaries) {
            Some(Settled::One(reading)) => vec![(reading, 0.0)],
            Some(Settled::Several(candidates)) => candidates,
            None => continue,
        };
        if is_contested(&candidates) {
            continue;
        }
        let (best, _) = candidates.first()?;
        if rest.iter().any(|part| part.reading.is_empty()) {
            return None;
        }
        let tail: String = rest.iter().map(|part| part.reading.as_str()).collect();
        let repaired = format!("{best}{tail}");
        // Nothing to report if the prefix agreed with the parts all along.
        if to_hiragana(&repaired) == to_hiragana(&token.reading) {
            return None;
        }
        return Some(repaired);
    }
    None
}

fn readings_for(surface: &str, dictionaries: Evidence<'_>) -> Option<Settled> {
    let mut candidates: Vec<(String, f64)> = Vec::new();
    for source in dictionaries.sources() {
        let Ok(hits) = source.lookup(surface) else {
            continue;
        };
        for hit in hits {
            if hit.entry.term != surface || hit.entry.reading.is_empty() {
                continue;
            }
            // **A reading equal to the spelling is not a reading.** `Entry::reading` is filled
            // in from `term` when the source leaves it blank, so an entry with no recorded
            // reading looks like one that reads exactly as written — and taking it turned
            // 夜ふかし's よふかし into the literal characters 夜ふかし.
            if hit.entry.reading == hit.entry.term {
                continue;
            }
            // Compared as hiragana so コーヒー and こーひー do not read as a disagreement, while
            // the original is what gets stored — `project notes` §4 keeps katakana katakana.
            let reading = hit.entry.reading.clone();
            if !candidates
                .iter()
                .any(|(seen, _): &(String, f64)| to_hiragana(seen) == to_hiragana(&reading))
            {
                candidates.push((reading, hit.entry.score));
            }
        }
    }

    match candidates.len() {
        0 => None,
        1 => Some(Settled::One(candidates.remove(0).0)),
        _ => {
            // **The dictionary's own ranking, not mine.** お父さん lists おとうさん at score 200
            // with a priority tag and the obsolete おとっさん at −101; sorting by that is the
            // source saying which reading is current, rather than this file guessing.
            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            Some(Settled::Several(candidates))
        }
    }
}

/// Whether a character is one whose reading is worth checking.
///
/// CJK unified ideographs and the extension A block. Deliberately not kana, and deliberately
/// not the iteration mark 々 on its own — it takes its reading from what precedes it.
fn is_kanji(ch: char) -> bool {
    matches!(ch, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Token;

    fn analysis_of(pairs: &[(&str, &str)]) -> Analysis {
        Analysis {
            execution: None,
            text: pairs.iter().map(|(surface, _)| *surface).collect(),
            tokens: pairs
                .iter()
                .map(|(surface, reading)| Token {
                    surface: (*surface).to_string(),
                    reading: (*reading).to_string(),
                    ..Default::default()
                })
                .collect(),
        }
    }

    /// With nothing installed the analyzer's readings stand, untouched.
    #[test]
    fn no_dictionary_means_no_opinion() {
        let mut analysis = analysis_of(&[("者", "しゃ")]);
        let counts = correct(&mut analysis, &[]);
        assert_eq!(counts, Corrections::default());
        assert_eq!(analysis.tokens[0].reading, "しゃ");
    }

    /// Kana is skipped before any lookup happens.
    ///
    /// Not an optimisation: a dictionary entry for a kana spelling would be a different word,
    /// and taking its reading would overwrite a correct one with an unrelated one.
    #[test]
    fn kana_is_never_corrected() {
        let mut analysis = analysis_of(&[("は", "は"), ("コーヒー", "コーヒー")]);
        let counts = correct(&mut analysis, &[]);
        assert_eq!(counts.corrected, 0);
        assert_eq!(analysis.tokens[0].reading, "は");
        assert_eq!(analysis.tokens[1].reading, "コーヒー");
    }

    /// **An inflected form is not a headword**, and both of these were caught being corrected
    /// wrongly before the guard existed.
    ///
    /// 撮り as a token is the 連用形 of 撮る, read とり. The dictionary's 撮り is the 〜撮り
    /// suffix, read どり — same spelling, different word. Same for 迫り: せまり as the verb,
    /// せり as the stage lift. A unanimous dictionary is not a right dictionary when the thing
    /// being looked up is not the thing it has an entry for.
    #[test]
    fn an_inflected_surface_is_never_corrected() {
        for token in [
            Token {
                surface: "撮り".into(),
                dict_form: "撮る".into(),
                reading: "とり".into(),
                ..Default::default()
            },
            Token {
                surface: "見かけた".into(),
                dict_form: "見かける".into(),
                reading: "みかけた".into(),
                ..Default::default()
            },
        ] {
            let expected = token.reading.clone();
            let mut analysis = Analysis {
                execution: None,
                text: token.surface.clone(),
                tokens: vec![token],
            };
            // No dictionaries here, so this pins the intent rather than the mechanism.
            //
            // **It used to pin the mechanism, and the mechanism moved.** The guard was an early
            // `continue` before any lookup happened; it is now a skipped match arm, because the
            // *flagging* half of this pass does run for an inflected token — it asks about the
            // dictionary form. Correcting still does not. `heteronym_readings.rs` is where that
            // is checked against a real dictionary, which is the only place it can be.
            correct(&mut analysis, &[]);
            assert_eq!(analysis.tokens[0].reading, expected);
        }
    }

    #[test]
    fn only_kanji_bearing_surfaces_are_examined() {
        assert!(is_kanji('者'));
        assert!(is_kanji('乳'));
        assert!(!is_kanji('は'));
        assert!(!is_kanji('ー'));
        assert!(
            !is_kanji('々'),
            "the iteration mark has no reading of its own"
        );
        assert!(!is_kanji('♥'));
    }
}
