//! What a *character* is, as against what a word means.
//!
//! # Why this exists
//!
//! Jitendex and JMnedict are word dictionaries and neither ships a kanji bank, so before
//! this the app could not answer "what is this character and how is it read". Looking 館
//! up returned the word やかた, "mansion" — which is not what 館 does in 図書館. 勉 and 聞
//! returned nothing at all, being characters rather than words.
//!
//! The point is not the readings as trivia. It is that compounds take on'yomi and
//! standalone verbs take kun'yomi, and seeing 新聞 しんぶん beside 新しい あたらしい and
//! 聞く きく turns a great deal of memorisation into a pattern.
//!
//! # Where the table comes from
//!
//! `scripts/fetch-kanjidic.ps1` parses KANJIDIC2 (EDRDG, CC BY-SA 4.0) into one flat
//! `char<TAB>on<TAB>kun<TAB>meanings<TAB>strokes<TAB>grade<TAB>nanori` file. The separately
//! derived `krad.tsv` contributes visible components. Like the WordNet table
//! behind particle senses it is **optional**: absent, everything here answers empty and
//! the reference page says how to fetch it rather than showing nothing.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

/// One character.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Kanji {
    /// On'yomi, as KANJIDIC writes them: katakana, e.g. `ショク`.
    pub on: Vec<String>,
    /// Kun'yomi, verbatim: `た.べる` is stem た plus okurigana べる, and a leading or
    /// trailing `-` marks a prefix or suffix use. The period is load-bearing — it is what
    /// [`align`] uses to tell the character's own reading from the kana after it.
    pub kun: Vec<String>,
    pub meanings: Vec<String>,
    pub strokes: u8,
    /// Jōyō school year, where the character has one. 1–6 are primary, 8 secondary.
    pub grade: Option<u8>,
    /// Name-only readings. Displayed as names and deliberately excluded from [`align`].
    pub nanori: Vec<String>,
    /// Visible parts from basic KRADFILE. These are not necessarily traditional radicals.
    pub components: Vec<String>,
}

/// The kanji table, or an empty one when it has not been fetched.
#[derive(Debug, Default)]
pub struct KanjiDic {
    entries: HashMap<char, Kanji>,
    by_component: HashMap<String, Vec<char>>,
}

impl KanjiDic {
    /// Read the table the fetch script writes.
    ///
    /// A missing file is the ordinary state of a fresh install, not an error: the caller
    /// gets an empty table that never matches, and the interface says so.
    pub fn load(path: &Path) -> KanjiDic {
        let Ok(text) = std::fs::read_to_string(path) else {
            return KanjiDic::default();
        };
        let mut map = HashMap::new();
        for line in text.lines() {
            let mut fields = line.split('\t');
            let Some(character) = fields.next().and_then(|f| f.chars().next()) else {
                continue;
            };
            let list = |field: Option<&str>, separator: char| -> Vec<String> {
                field
                    .unwrap_or("")
                    .split(separator)
                    .filter(|part| !part.is_empty())
                    .map(ToString::to_string)
                    .collect()
            };
            let on = list(fields.next(), ',');
            let kun = list(fields.next(), ',');
            let meanings = list(fields.next(), ';');
            let strokes = fields.next().unwrap_or("").parse().unwrap_or(0);
            let grade = fields.next().unwrap_or("").parse().ok();
            // Legacy tables end at grade. The missing supplemental field is intentionally
            // ordinary so an existing installation keeps working until its owner chooses Update.
            let nanori = list(fields.next(), ',');
            map.insert(
                character,
                Kanji {
                    on,
                    kun,
                    meanings,
                    strokes,
                    grade,
                    nanori,
                    components: Vec::new(),
                },
            );
        }
        let mut by_component: HashMap<String, Vec<char>> = HashMap::new();
        if let Some(parent) = path.parent() {
            if let Ok(krad) = std::fs::read_to_string(parent.join("krad.tsv")) {
                for line in krad.lines() {
                    let Some((literal, parts)) = line.split_once('\t') else {
                        continue;
                    };
                    let Some(character) = literal.chars().next() else {
                        continue;
                    };
                    let components: Vec<String> =
                        parts.chars().map(|part| part.to_string()).collect();
                    if let Some(entry) = map.get_mut(&character) {
                        entry.components = components.clone();
                    }
                    for component in components {
                        by_component.entry(component).or_default().push(character);
                    }
                }
            }
        }
        KanjiDic {
            entries: map,
            by_component,
        }
    }

    pub fn get(&self, character: char) -> Option<&Kanji> {
        self.entries.get(&character)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Characters whose English meanings use every word of the query.
    ///
    /// A plain scan: twelve thousand characters is small enough that an index would be
    /// machinery for its own sake.
    ///
    /// Ordered by, in turn:
    ///
    /// - **whether it is taught in school at all.** `grade` is the only signal KANJIDIC
    ///   carries for how ordinary a character is, and without it "eat" answered 侑, 飠 and
    ///   蛀 above 食 — a rare character, a bare radical, and a word for woodworm.
    /// - how early the match sits in the meaning list, so a character that *means* this
    ///   beats one that merely mentions it;
    /// - the school year, then the stroke count.
    pub fn by_meaning(&self, query: &str, limit: usize) -> Vec<char> {
        let wanted = super::index::gloss_words(query);
        if wanted.is_empty() {
            return Vec::new();
        }

        let mut hits: Vec<(u8, usize, u8, u8, char)> = Vec::new();
        for (character, kanji) in &self.entries {
            let mut best: Option<usize> = None;
            for (index, meaning) in kanji.meanings.iter().enumerate() {
                let words = super::index::gloss_words(meaning);
                if wanted.iter().all(|word| words.contains(word)) {
                    best = Some(best.map_or(index, |kept: usize| kept.min(index)));
                }
            }
            if let Some(at) = best {
                hits.push((
                    u8::from(kanji.grade.is_none()),
                    at,
                    kanji.grade.unwrap_or(u8::MAX),
                    kanji.strokes,
                    *character,
                ));
            }
        }
        hits.sort_unstable();
        hits.into_iter()
            .take(limit)
            .map(|(.., character)| character)
            .collect()
    }

    /// Characters visibly containing `component`, in learner-useful deterministic order.
    pub fn related(&self, component: &str, excluding: char, limit: usize) -> (usize, Vec<char>) {
        let Some(characters) = self.by_component.get(component) else {
            return (0, Vec::new());
        };
        let mut characters: Vec<char> = characters
            .iter()
            .copied()
            .filter(|character| *character != excluding)
            .collect();
        characters.sort_by_key(|character| {
            let kanji = self.entries.get(character);
            (
                u8::from(kanji.and_then(|entry| entry.grade).is_none()),
                kanji.and_then(|entry| entry.grade).unwrap_or(u8::MAX),
                kanji.map(|entry| entry.strokes).unwrap_or(u8::MAX),
                *character,
            )
        });
        let total = characters.len();
        characters.truncate(limit.min(24));
        (total, characters)
    }
}

/// Which reading each character of `term` contributes to `reading`.
///
/// Parallel to `term`'s characters. `Some` names the reading that character supplies;
/// `None` means nothing is claimed for it — either it is kana, or the alignments that
/// worked disagreed about it.
///
/// # Returning `None` is the important case
///
/// `None` overall means the word's reading is **not built from its characters' own
/// readings**, and it is the right answer for 熟字訓 — 明日 あした, 大人 おとな,
/// 今日 きょう — where no character contributes anything and any marking would be a
/// confident lie. The interface has to show that as "this one is irregular" rather than
/// as an empty result.
///
/// # What it allows
///
/// Two sound changes, because both are regular:
///
/// - **rendaku**, a following element's first mora voicing — 人々 ひと + **び**と;
/// - **sokuon**, a first element's last mora becoming っ — 学 がく + 校 こう → が**っ**こう.
///
/// Anything less regular is not guessed at.
///
/// # A space in the reading is a boundary
///
/// Ichiran reads a compound as its parts separated by a space — 帰京する is `ききょう する` —
/// and `analyzer::ichiran::convert` keeps that space on purpose. Here it may be passed over
/// only **between** two characters of `term`, never inside one character's reading, so it
/// constrains the alignment rather than being thrown away. Without this, every such compound
/// failed to align and fell back to whole-word ruby.
pub fn align(term: &str, reading: &str, dic: &KanjiDic) -> Option<Vec<Option<String>>> {
    let term: Vec<char> = term.chars().collect();
    let reading: Vec<char> = reading.chars().collect();
    if term.is_empty() || reading.is_empty() {
        return None;
    }

    let mut solutions: Vec<Vec<Option<String>>> = Vec::new();
    let mut chosen: Vec<Option<String>> = vec![None; term.len()];
    walk(&term, &reading, 0, 0, dic, &mut chosen, &mut solutions);

    let first = solutions.first()?;
    // Where the successful alignments disagree about a character, that character is left
    // unmarked while the ones they agree on are kept. A tie is not an answer — the rule
    // `grammar::particles::Reading::favoured` already applies to particle senses.
    let mut agreed = first.clone();
    for solution in &solutions[1..] {
        for (kept, other) in agreed.iter_mut().zip(solution) {
            if kept.as_deref() != other.as_deref() {
                *kept = None;
            }
        }
    }
    Some(agreed)
}

/// One run of a word as it should be set: the text, and the reading to print over it.
///
/// `reading` is `None` for a run that needs no annotation — kana, or a word whose reading
/// could not be split.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ruby {
    pub text: String,
    pub reading: Option<String>,
}

/// Split `surface` into the runs a reader should see, with kana over the kanji they belong
/// to where that can be established and over the whole word where it cannot.
///
/// # Why it so often gives up
///
/// [`align`] answers `None` per character for two different reasons — the character is
/// kana, or the alignments that worked disagreed about it — and the difference is not
/// recoverable from the result. So the rule is all or nothing: **every** kanji must have
/// been given a reading, or the whole word is annotated as one.
///
/// Partial furigana is worse than none. Two of three kanji marked reads as a claim about
/// the third, and 新聞社 with しん and ぶん placed but しゃ missing says the last character
/// is silent. Whole-word ruby says only "this is how the word sounds", which is true
/// whatever the characters are doing.
pub fn furigana(surface: &str, reading: &str, dic: &KanjiDic) -> Vec<Ruby> {
    let whole = || {
        vec![Ruby {
            text: surface.to_string(),
            reading: (!reading.is_empty() && reading != surface).then(|| reading.to_string()),
        }]
    };

    if !surface.chars().any(crate::kana::is_kanji) {
        // Nothing to annotate: the word is already as readable as it is going to get.
        return vec![Ruby {
            text: surface.to_string(),
            reading: None,
        }];
    }

    let Some(marks) = align(surface, reading, dic) else {
        return whole();
    };
    let settled = surface
        .chars()
        .zip(&marks)
        .all(|(c, mark)| !crate::kana::is_kanji(c) || mark.is_some());
    if !settled {
        return whole();
    }

    let mut runs: Vec<Ruby> = Vec::new();
    for (character, mark) in surface.chars().zip(&marks) {
        match mark {
            // A kanji with a reading of its own stands alone, so the kana sits over it.
            Some(sound) => runs.push(Ruby {
                text: character.to_string(),
                reading: Some(sound.clone()),
            }),
            // Kana joins whatever plain run precedes it rather than becoming its own.
            None => match runs.last_mut() {
                Some(last) if last.reading.is_none() => last.text.push(character),
                _ => runs.push(Ruby {
                    text: character.to_string(),
                    reading: None,
                }),
            },
        }
    }
    runs
}

/// How many complete alignments to collect before giving up on finding more.
///
/// Only used to decide *agreement*, and a word whose characters can be read this many
/// ways has already told us the answer is "several".
const MAX_SOLUTIONS: usize = 32;

fn walk(
    term: &[char],
    reading: &[char],
    at: usize,
    read: usize,
    dic: &KanjiDic,
    chosen: &mut Vec<Option<String>>,
    solutions: &mut Vec<Vec<Option<String>>>,
) {
    if solutions.len() >= MAX_SOLUTIONS {
        return;
    }
    // A space in the reading is a boundary between characters (see [`align`]): pass over it
    // here, where one character's reading has ended and the next has not begun. A space the
    // term itself writes is matched literally below instead.
    let mut read = read;
    if !term.get(at).is_some_and(|c| c.is_whitespace()) {
        while reading.get(read).is_some_and(|c| c.is_whitespace()) {
            read += 1;
        }
    }
    if at == term.len() {
        if read == reading.len() {
            solutions.push(chosen.clone());
        }
        return;
    }

    let character = term[at];

    // Kana in the word must appear in the reading exactly. This is what handles okurigana
    // without any special case: 食べる is 食 + べ + る, and べる matches itself.
    if !is_kanji(character) {
        let expected = to_hiragana(character);
        if read < reading.len() && to_hiragana(reading[read]) == expected {
            walk(term, reading, at + 1, read + 1, dic, chosen, solutions);
        }
        return;
    }

    // 々 repeats the character before it — 人々 is 人 twice.
    let source = if character == '々' && at > 0 {
        term[at - 1]
    } else {
        character
    };
    let Some(kanji) = dic.get(source) else {
        return;
    };

    for candidate in candidates(kanji) {
        for variant in variants(&candidate) {
            let taken: Vec<char> = variant.chars().collect();
            if read + taken.len() > reading.len() {
                continue;
            }
            if reading[read..read + taken.len()] != taken[..] {
                continue;
            }
            // **`variant`, not `candidate`.** The match was made against the variant, so the
            // variant is what this character sounds like here; the candidate is only where it
            // came from. Recording the candidate printed the dictionary's spelling over a word
            // nobody says that way — 人間 as にんけん, 学校 as がくこう, 人々 as ひとひと —
            // and the reading beside it in the vocabulary panel, which reads the word rather
            // than the characters, disagreed on screen.
            chosen[at] = Some(variant.clone());
            walk(
                term,
                reading,
                at + 1,
                read + taken.len(),
                dic,
                chosen,
                solutions,
            );
            chosen[at] = None;
        }
    }
}

/// Whether `reading` is one [`align`] could give `character`: a KANJIDIC reading or one of the
/// regular sound changes of one.
///
/// **A consistency check, not evidence.** Per-character furigana is only ever produced from these
/// same candidates, so a reading [`furigana`] placed over a single kanji passes this by
/// construction. It answers "does the displayed reading come from the table", never "is it right".
pub fn attested(character: char, reading: &str, dic: &KanjiDic) -> bool {
    let Some(kanji) = dic.get(character) else {
        return false;
    };
    let wanted = crate::kana::to_hiragana(reading);
    candidates(kanji)
        .iter()
        .flat_map(|candidate| variants(candidate))
        .any(|variant| variant == wanted)
}

/// Every reading a character can contribute, in hiragana and without okurigana.
///
/// On readings are converted from katakana. Kun readings give up everything after the
/// period — that part is written as kana in the word and matches itself — and the `-`
/// marking prefix or suffix use is dropped.
fn candidates(kanji: &Kanji) -> Vec<String> {
    let mut out = Vec::new();
    for reading in &kanji.on {
        let hiragana: String = reading.chars().map(to_hiragana).collect();
        if !hiragana.is_empty() && !out.contains(&hiragana) {
            out.push(hiragana);
        }
    }
    for reading in &kanji.kun {
        let bare = reading.trim_matches('-');
        let mut parts = bare.split('.');
        let stem = parts.next().unwrap_or("").to_string();
        if stem.is_empty() {
            continue;
        }
        let okurigana = parts.next().unwrap_or("");

        let mut push = |candidate: String| {
            if !candidate.is_empty() && !out.contains(&candidate) {
                out.push(candidate);
            }
        };

        // The word writes the okurigana as kana, so the character supplies only the stem:
        // 食べる is 食 た + べる.
        push(stem.clone());

        // Except inside a compound, where the okurigana is not written but is still
        // *read*: 剣突 is けんつく from 突 つ.く, and 寄集め is よせあつめ from 寄 よ.せる.
        // Both were abstentions until this was added — found by printing what the matcher
        // gave up on across the whole dictionary rather than by reasoning about it.
        push(format!("{stem}{okurigana}"));

        // And the continuative, which is how a verb enters a compound noun: 焼豚 is
        // やきぶた, 焼 や.く becoming やき. Regular for the godan endings, and already
        // covered by the bare stem for ichidan.
        if let Some(continuative) = continuative(okurigana) {
            push(format!("{stem}{continuative}"));
        }
    }
    out
}

/// The 連用形 of a one-character godan ending: く → き, す → し, and so on.
///
/// `None` for anything longer, which is an ichidan verb whose continuative is the bare
/// stem — 食べる → 食べ — and that is already offered.
fn continuative(okurigana: &str) -> Option<char> {
    let mut chars = okurigana.chars();
    let only = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match only {
        'う' => 'い',
        'く' => 'き',
        'ぐ' => 'ぎ',
        'す' => 'し',
        'つ' => 'ち',
        'ぬ' => 'に',
        'ぶ' => 'び',
        'む' => 'み',
        'る' => 'り',
        _ => return None,
    })
}

/// A reading and the regular sound changes it undergoes inside a compound.
fn variants(reading: &str) -> Vec<String> {
    let mut out = vec![reading.to_string()];
    let chars: Vec<char> = reading.chars().collect();

    // Rendaku: the first mora voices. 人々 ひと + びと.
    if let Some(voiced) = voice(chars[0]) {
        let mut with = String::from(voiced);
        with.extend(chars[1..].iter());
        out.push(with);
    }
    // Handakuten, the は-row's other option: ひと + ぴと.
    if let Some(plosive) = plosive(chars[0]) {
        let mut with = String::from(plosive);
        with.extend(chars[1..].iter());
        out.push(with);
    }
    // Sokuon: the last mora becomes っ. 学 がく + 校 こう → がっこう.
    if chars.len() > 1 && matches!(chars[chars.len() - 1], 'く' | 'き' | 'つ' | 'ち' | 'ふ') {
        let mut with: String = chars[..chars.len() - 1].iter().collect();
        with.push('っ');
        out.push(with);
    }
    out
}

fn voice(c: char) -> Option<char> {
    Some(match c {
        'か' => 'が',
        'き' => 'ぎ',
        'く' => 'ぐ',
        'け' => 'げ',
        'こ' => 'ご',
        'さ' => 'ざ',
        'し' => 'じ',
        'す' => 'ず',
        'せ' => 'ぜ',
        'そ' => 'ぞ',
        'た' => 'だ',
        'ち' => 'ぢ',
        'つ' => 'づ',
        'て' => 'で',
        'と' => 'ど',
        'は' => 'ば',
        'ひ' => 'び',
        'ふ' => 'ぶ',
        'へ' => 'べ',
        'ほ' => 'ぼ',
        _ => return None,
    })
}

fn plosive(c: char) -> Option<char> {
    Some(match c {
        'は' => 'ぱ',
        'ひ' => 'ぴ',
        'ふ' => 'ぷ',
        'へ' => 'ぺ',
        'ほ' => 'ぽ',
        _ => return None,
    })
}

/// Katakana to hiragana; anything else unchanged.
fn to_hiragana(c: char) -> char {
    match c {
        // The small kana and ー are shared, so only the main block shifts.
        '\u{30a1}'..='\u{30f6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        other => other,
    }
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' | '々')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dic(rows: &[(char, &str, &str)]) -> KanjiDic {
        KanjiDic {
            entries: rows
                .iter()
                .map(|(character, on, kun)| {
                    (
                        *character,
                        Kanji {
                            on: on
                                .split(',')
                                .filter(|s| !s.is_empty())
                                .map(String::from)
                                .collect(),
                            kun: kun
                                .split(',')
                                .filter(|s| !s.is_empty())
                                .map(String::from)
                                .collect(),
                            meanings: vec!["meaning".into()],
                            strokes: 1,
                            grade: None,
                            nanori: Vec::new(),
                            components: Vec::new(),
                        },
                    )
                })
                .collect(),
            by_component: HashMap::new(),
        }
    }

    // ---- furigana --------------------------------------------------------------------

    /// **A sound change is printed as it is pronounced, not as the dictionary spells it.**
    ///
    /// 人間 is にんげん. KANJIDIC lists 間 as ケン, and the aligner reaches げん through the
    /// rendaku rule in `variants` — so the match is on げん while the dictionary entry says
    /// けん. Printing the entry puts にんけん over a word nobody says that way, and it is the
    /// furigana that is wrong rather than the reading: the vocabulary panel beside it, which
    /// reads the word rather than the characters, says にんげん.
    ///
    /// The same applies to every rendaku compound and to sokuon: 学校 matched がっ through
    /// 学 がく and would otherwise print がく over 学 and read がくこう.
    #[test]
    fn a_rendaku_reading_is_printed_voiced() {
        let dic = dic(&[
            ('人', "ジン,ニン", "ひと"),
            ('間', "カン,ケン", "あいだ,ま"),
        ]);
        assert_eq!(
            furigana("人間", "にんげん", &dic),
            vec![
                Ruby {
                    text: "人".into(),
                    reading: Some("にん".into())
                },
                Ruby {
                    text: "間".into(),
                    reading: Some("げん".into())
                },
            ],
            "げん is what the word says; けん is only where it came from"
        );
    }

    #[test]
    fn a_compound_puts_each_reading_over_its_own_character() {
        let dic = dic(&[
            ('図', "ズ,ト", "え,はか.る"),
            ('書', "ショ", "か.く"),
            ('館', "カン", "やかた,たて"),
        ]);
        assert_eq!(
            furigana("図書館", "としょかん", &dic),
            vec![
                Ruby {
                    text: "図".into(),
                    reading: Some("と".into())
                },
                Ruby {
                    text: "書".into(),
                    reading: Some("しょ".into())
                },
                Ruby {
                    text: "館".into(),
                    reading: Some("かん".into())
                },
            ]
        );
    }

    /// Okurigana is not annotated and does not become a run of its own per character —
    /// it joins the plain run beside it, so 食べる is two runs and not four.
    #[test]
    fn okurigana_joins_the_plain_run_rather_than_being_marked() {
        let dic = dic(&[('食', "ショク", "た.べる,く.う")]);
        assert_eq!(
            furigana("食べる", "たべる", &dic),
            vec![
                Ruby {
                    text: "食".into(),
                    reading: Some("た".into())
                },
                Ruby {
                    text: "べる".into(),
                    reading: None
                },
            ]
        );
    }

    /// 熟字訓: the reading belongs to the word, not to either character, so it goes over
    /// the whole thing. `align` says so by answering `None`.
    #[test]
    fn a_reading_that_belongs_to_no_character_covers_the_word() {
        let dic = dic(&[('明', "メイ", "あか.るい"), ('日', "ニチ", "ひ,か")]);
        assert_eq!(
            furigana("明日", "あした", &dic),
            vec![Ruby {
                text: "明日".into(),
                reading: Some("あした".into())
            }]
        );
    }

    /// The rule that matters: one unsettled kanji takes the whole word with it. Marking
    /// the characters that did align would read as a claim that the other is silent.
    #[test]
    fn one_unsettled_character_gives_up_on_the_whole_word() {
        // 書 is missing its reading, so nothing can be claimed for it.
        let dic = dic(&[('図', "ズ,ト", ""), ('書', "", ""), ('館', "カン", "")]);
        let runs = furigana("図書館", "としょかん", &dic);
        assert_eq!(
            runs,
            vec![Ruby {
                text: "図書館".into(),
                reading: Some("としょかん".into())
            }],
            "partial marking escaped: {runs:?}"
        );
    }

    /// The path a new install takes. KANJIDIC is an optional download and `KanjiDic::load`
    /// on a missing file yields an empty table, so this is the common case and must still
    /// produce the whole-word ruby the reader showed before any of this existed.
    #[test]
    fn without_a_kanji_table_every_word_is_annotated_whole() {
        let empty = dic(&[]);
        assert_eq!(
            furigana("図書館", "としょかん", &empty),
            vec![Ruby {
                text: "図書館".into(),
                reading: Some("としょかん".into())
            }]
        );
    }

    #[test]
    fn a_word_without_kanji_is_not_annotated_at_all() {
        let dic = dic(&[]);
        assert_eq!(
            furigana("ください", "ください", &dic),
            vec![Ruby {
                text: "ください".into(),
                reading: None
            }]
        );
    }

    /// **Ichiran's compound reading aligns across its space.** 帰京する reads `ききょう する` as
    /// Ichiran gives it — real engine output, from the JoyoKanji-Yomi benchmark run. Before the
    /// space was treated as a boundary, this and 884 of the benchmark's 8,558 items fell back to
    /// whole-word ruby.
    #[test]
    fn a_space_in_the_reading_is_a_boundary_between_characters() {
        let dic = dic(&[('帰', "キ", "かえ.る"), ('京', "キョウ,ケイ", "みやこ")]);
        assert_eq!(
            furigana("帰京する", "ききょう する", &dic),
            vec![
                Ruby {
                    text: "帰".into(),
                    reading: Some("き".into())
                },
                Ruby {
                    text: "京".into(),
                    reading: Some("きょう".into())
                },
                Ruby {
                    text: "する".into(),
                    reading: None
                },
            ]
        );
        // The boundary constrains: a space inside 京's reading matches no alignment, so the word
        // is annotated whole, with the reading as given.
        assert!(align("帰京する", "ききょ うする", &dic).is_none());
        // A space between two kanji is a boundary too, wherever the parts divide.
        assert_eq!(
            align("帰京", "き きょう", &dic),
            Some(vec![Some("き".into()), Some("きょう".into())])
        );
    }

    /// The case the word dictionary gets wrong: 館 is やかた as a word and カン here.
    #[test]
    fn a_compound_marks_the_on_reading_each_character_contributes() {
        let dic = dic(&[
            ('図', "ズ,ト", "え,はか.る"),
            ('書', "ショ", "か.く"),
            ('館', "カン", "やかた,たて"),
        ]);
        let marked = align("図書館", "としょかん", &dic).expect("aligns");
        assert_eq!(
            marked,
            vec![
                Some("と".to_string()),
                Some("しょ".to_string()),
                Some("かん".to_string())
            ]
        );
    }

    /// A kun reading gives its stem; the okurigana is kana in the word and matches itself.
    #[test]
    fn okurigana_is_matched_as_the_kana_it_is() {
        let dic = dic(&[('食', "ショク,ジキ", "く.う,た.べる")]);
        let marked = align("食べる", "たべる", &dic).expect("aligns");
        assert_eq!(marked, vec![Some("た".to_string()), None, None]);
    }

    #[test]
    fn a_compound_that_geminates_still_aligns() {
        let dic = dic(&[('学', "ガク", "まな.ぶ"), ('校', "コウ,キョウ", "")]);
        let marked = align("学校", "がっこう", &dic).expect("aligns");
        assert_eq!(
            marked,
            vec![Some("がっ".to_string()), Some("こう".to_string())],
            "the clipped mora is what 学 contributes here, not the がく it came from"
        );
    }

    #[test]
    fn a_repeated_character_voices_the_second_time() {
        let dic = dic(&[('人', "ジン,ニン", "ひと")]);
        let marked = align("人々", "ひとびと", &dic).expect("aligns");
        assert_eq!(
            marked,
            vec![Some("ひと".to_string()), Some("びと".to_string())],
            "々 is 人 again, and the second one is voiced"
        );
    }

    /// **The case that must not be guessed at.**
    ///
    /// 明日 is あした, and neither 明 nor 日 contributes any part of that. A matcher that
    /// answered anything here would be teaching a reading that does not exist.
    #[test]
    fn a_word_whose_reading_is_not_built_from_its_characters_says_nothing() {
        let dic = dic(&[
            ('明', "メイ,ミョウ", "あ.かり,あか.るい"),
            ('日', "ニチ,ジツ", "ひ,-び,-か"),
        ]);
        assert!(align("明日", "あした", &dic).is_none());
        // And the regular reading of the same characters does align.
        assert!(align("明日", "みょうにち", &dic).is_some());
    }

    /// Where two alignments both work and disagree, neither is asserted.
    #[test]
    fn a_character_two_readings_could_explain_is_left_unmarked() {
        // Contrived so both orders fit: either character can supply こう.
        let dic = dic(&[('甲', "コウ", ""), ('乙', "コウ,オツ", "")]);
        let marked = align("甲乙", "こうこう", &dic).expect("aligns");
        assert_eq!(marked, vec![Some("こう".into()), Some("こう".into())]);
    }

    #[test]
    fn a_character_the_table_does_not_have_stops_the_alignment() {
        let dic = dic(&[('食', "ショク", "た.べる")]);
        assert!(align("食堂", "しょくどう", &dic).is_none());
    }

    #[test]
    fn an_absent_table_answers_nothing_rather_than_failing() {
        let empty = KanjiDic::default();
        assert!(empty.is_empty());
        assert!(empty.get('食').is_none());
        assert!(empty.by_meaning("eat", 5).is_empty());
        assert!(align("図書館", "としょかん", &empty).is_none());
    }

    #[test]
    fn legacy_table_loads_and_nanori_never_becomes_an_alignment_candidate() {
        let dir = std::env::temp_dir().join("sakatsu-kanji-legacy-and-nanori");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let table = dir.join("kanjidic.tsv");
        std::fs::write(
            &table,
            "日\tニチ\tひ\tday\t4\t1\n明\tメイ\tあか.るい\tbright\t8\t2\tあす\n",
        )
        .unwrap();
        let dic = KanjiDic::load(&table);

        assert!(
            dic.get('日').unwrap().nanori.is_empty(),
            "legacy row stays valid"
        );
        assert_eq!(dic.get('明').unwrap().nanori, ["あす"]);
        assert!(
            align("明日", "あすひ", &dic).is_none(),
            "name readings must never enter furigana alignment"
        );
    }

    #[test]
    fn related_characters_are_deterministic_excluded_and_capped() {
        let characters: Vec<char> = (0x4e00..0x4e20).filter_map(char::from_u32).collect();
        let mut entries = HashMap::new();
        for (index, character) in characters.iter().copied().enumerate() {
            entries.insert(
                character,
                Kanji {
                    grade: if index == 2 { None } else { Some(3) },
                    strokes: 10,
                    ..Kanji::default()
                },
            );
        }
        entries.get_mut(&characters[0]).unwrap().grade = Some(2);
        entries.get_mut(&characters[0]).unwrap().strokes = 4;
        entries.get_mut(&characters[1]).unwrap().grade = Some(1);
        entries.get_mut(&characters[1]).unwrap().strokes = 9;
        entries.get_mut(&characters[3]).unwrap().grade = Some(1);
        entries.get_mut(&characters[3]).unwrap().strokes = 3;
        let dic = KanjiDic {
            entries,
            by_component: HashMap::from([("木".into(), characters.clone())]),
        };

        let (total, related) = dic.related("木", characters[4], usize::MAX);
        assert_eq!(total, characters.len() - 1);
        assert_eq!(related.len(), 24);
        assert_eq!(
            related[..4],
            [characters[3], characters[1], characters[0], characters[5]]
        );
        assert!(!related.contains(&characters[4]));
    }

    #[test]
    fn meanings_are_searchable_and_the_plainest_character_comes_first() {
        let mut dic = dic(&[('食', "ショク", "た.べる"), ('飯', "ハン", "めし")]);
        dic.entries.get_mut(&'食').unwrap().meanings = vec!["eat".into(), "food".into()];
        dic.entries.get_mut(&'飯').unwrap().meanings = vec!["meal".into(), "food".into()];
        dic.entries.get_mut(&'食').unwrap().strokes = 9;
        dic.entries.get_mut(&'飯').unwrap().strokes = 12;

        assert_eq!(dic.by_meaning("eat", 5), vec!['食']);
        // Both mean "food"; 食 has it earlier in its list, so it leads.
        assert_eq!(dic.by_meaning("food", 5), vec!['食', '飯']);
        assert!(dic.by_meaning("nothinglikethis", 5).is_empty());
    }
}
