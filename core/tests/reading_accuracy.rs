//! Are the furigana right?
//!
//! ```text
//! cargo test -p sakatsu-core --test reading_accuracy -- --ignored --nocapture
//! ```
//!
//! # Why this is worth a test of its own
//!
//! A reading sits over every kanji word in the reader, and a learner takes it on trust —
//! that is the entire point of showing it. So a wrong reading does not produce a
//! complaint, it produces someone confidently mispronouncing a word for months. Of every
//! way this app can be wrong, it is the one that fails most quietly and costs most.
//!
//! Before this, about eight readings were asserted anywhere in the suite, all from
//! committed Ichiran fixtures — none from Lindera, which is what a user without a sidecar
//! actually gets.
//!
//! # The dictionary is the authority, not me
//!
//! Every reading is checked against what Jitendex lists for that entry. I am a poor judge
//! of whether 上手 is じょうず here, and this way I am not the judge at all: the same
//! dictionary the app shows the learner is the one that decides.
//!
//! Which also sets the limit honestly — where the dictionary lists several readings for a
//! spelling, any of them is accepted and the word is reported as ambiguous. Pinning one
//! would be asserting my preference through the dictionary's mouth.

mod common;
#[path = "common/sentences.rs"]
mod sentences;

use std::collections::BTreeMap;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::Dictionary;
use sakatsu_core::model::contains_japanese;

use sentences::SENTENCES;

/// Whether a string contains a kanji — the words that get furigana at all.
fn has_kanji(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{3400}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'))
}

/// Kana differences that are not reading errors.
///
/// The analyzer reports katakana readings for some entries where the dictionary spells
/// them in hiragana. That is a notation difference, not a claim about pronunciation, and
/// failing on it would bury the real disagreements in noise.
fn to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            other => other,
        })
        .collect()
}

/// Rendaku and counter readings come from the compound, not from its parts.
///
/// **Each token was already right on its own, which is why this was invisible.** 受付 is うけつけ
/// and 係 is かかり; both correct, and 受付係 is うけつけ*が*かり. The voicing is caused by the
/// neighbour, so no per-token reading can produce it — and the dictionary has known the answer all
/// along. What was missing is that they arrive as two tokens and nothing asked.
///
/// **The controls are the point, again — and the guard they check has moved.** `merge_compounds`
/// once joined *only* where the dictionary's reading differed from the parts run together, so
/// 大学 + 生 could not pass: だいがく + せい already spells だいがくせい. It now also joins a
/// dictionary-known run whose surface carries no hiragana, because `ud_gold.rs` measured that
/// wider rule at 99.8% not-wrong against gold — 大学生 is the unit a learner marks known whatever
/// its reading does.
///
/// So the control below is no longer "an entry the parts already spell". It is **hiragana**:
/// 本を読む holds two runs the dictionary lists — 本を is not one of them, but the rule must not
/// reach for それは or には either, and no-hiragana is what keeps it away from all of them.
#[test]
#[ignore = "needs the dictionaries"]
fn a_compound_takes_the_reading_the_dictionary_gives_the_whole() {
    let dir = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&dir).into_iter().collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no Jitendex at {}", dir.display());
        return;
    }
    let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    let reading_of = |surface: &str| -> Option<String> {
        dictionaries.iter().find_map(|dictionary| {
            dictionary
                .lookup(surface)
                .ok()?
                .into_iter()
                .find_map(|hit| {
                    (hit.entry.term == surface
                        && !hit.entry.reading.is_empty()
                        && hit.entry.reading != hit.entry.term)
                        .then_some(hit.entry.reading)
                })
        })
    };
    let read = |text: &str| -> String {
        let mut analysis = analyzer.analyze(text).expect("analyzes");
        analysis.tokens = sakatsu_core::dict::merge_compounds(&analysis.tokens, reading_of);
        sakatsu_core::dict::readings::correct(&mut analysis, &dictionaries);
        analysis
            .tokens
            .iter()
            .map(|token| token.reading.clone())
            .collect()
    };

    // Rendaku, then counters. Neither is reachable from the parts.
    assert!(read("受付係に聞く。").contains("うけつけがかり"));
    assert!(read("株式会社で働く。").contains("かぶしきがいしゃ"));
    assert!(read("一回だけやる。").contains("いっかい"));
    assert!(read("九時に来る。").contains("くじ"));
    assert!(read("月曜日は休み。").contains("げつようび"));

    // The widening: a kana-free compound joins even where the parts already spell its reading.
    let surfaces = |text: &str| -> Vec<String> {
        let analysis = analyzer.analyze(text).expect("analyzes");
        sakatsu_core::dict::merge_compounds(&analysis.tokens, reading_of)
            .iter()
            .map(|token| token.surface.clone())
            .collect()
    };
    let got = surfaces("大学生になった。");
    assert!(
        got.iter().any(|token| token == "大学生"),
        "大学生 is one word even though だいがく+せい already spells its reading, got {got:?}"
    );

    // The guard that replaced the old one: nothing with hiragana in it is joined, which is what
    // keeps this away from それは and には — the class that made the unrestricted rule wrong half
    // the time in `ud_gold.rs`.
    let plain = analyzer.analyze("本を読む。").expect("analyzes");
    let joined = sakatsu_core::dict::merge_compounds(&plain.tokens, reading_of);
    assert_eq!(
        joined.len(),
        plain.tokens.len(),
        "nothing in 本を読む is a kana-free compound, so nothing should be joined"
    );
    let particles = analyzer.analyze("それは彼には無理だ。").expect("analyzes");
    let joined = sakatsu_core::dict::merge_compounds(&particles.tokens, reading_of);
    let got: Vec<&str> = joined.iter().map(|token| token.surface.as_str()).collect();
    assert!(
        !got.iter()
            .any(|token| *token == "それは" || *token == "には"),
        "particle sequences the dictionary lists must not be welded together, got {got:?}"
    );
}

/// The "this reading is a guess" mark is only worn where the guess was real.
///
/// **A caveat on a third of the page is not a caveat.** The mark fired on any spelling the
/// dictionary listed twice, and JMdict lists near-dead alternatives for very common words — 彼 is
/// かれ at 200 beside あれ at -101 "rarely used"; 道 is みち at 200 beside どう and タオ at 0.
/// Measured over 1,500 sentences it appeared on **32.9% of kanji-bearing tokens**, which teaches a
/// reader to ignore it, and then it is missing where it is earned. Behind the margin it is 12.3%.
///
/// Both halves are asserted because the fix is a threshold and a threshold can be pushed either
/// way: 年 is とし *and* ねん at 200 apiece and must keep its mark, while 彼 must lose one.
#[test]
#[ignore = "needs the dictionaries"]
fn the_guess_mark_is_worn_only_where_the_dictionary_is_really_divided() {
    let dir = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&dir).into_iter().collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no Jitendex at {}", dir.display());
        return;
    }
    let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    let marked = |text: &str, surface: &str| -> bool {
        let mut analysis = analyzer.analyze(text).expect("analyzes");
        sakatsu_core::dict::readings::correct(&mut analysis, &dictionaries);
        analysis
            .tokens
            .iter()
            .find(|token| token.surface == surface)
            .unwrap_or_else(|| panic!("{surface} is a token of {text}"))
            .reading_ambiguous
    };

    // Really divided: two readings the dictionary ranks alike.
    assert!(
        marked("年を取る。", "年"),
        "年 is とし and ねん, both at 200"
    );
    assert!(
        marked("私は学生です。", "私"),
        "私 is わたし and あたし, both at 200"
    );

    // Not divided: one reading, and rivals nobody uses.
    assert!(
        !marked("彼が来た。", "彼"),
        "彼 is かれ; かの and あれ score -1 and -101"
    );
    assert!(
        !marked("道を歩く。", "道"),
        "道 is みち; どう and タオ score 0"
    );
}

/// A compound's reading is never left as one the dictionary does not list.
///
/// **Composition invents readings, and this is where that is caught.** Joining tokens joins their
/// readings, so お父さん arrives as おちちさん — 父 alone really is ちち. `dict::readings` used to
/// flag the spelling ambiguous and leave that concatenation on screen, which is the worst of both:
/// a reading no source vouches for, wearing a caveat implying it was one of the dictionary's.
///
/// The three cases here differ in exactly the way that matters. お母さん has one listed reading
/// and is corrected outright; お父さん has two — current おとうさん and obsolete おとっさん — so the
/// caveat stays and the *reading under it* becomes one that exists; 開いた is inflected, where a
/// headword lookup would be answering about a different word, so it is flagged and left alone.
#[test]
#[ignore = "needs the dictionaries"]
fn a_composed_reading_is_replaced_when_no_entry_lists_it() {
    let dir = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&dir).into_iter().collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no Jitendex at {}", dir.display());
        return;
    }
    let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    let read = |text: &str, surface: &str| -> (String, bool) {
        let mut analysis = analyzer.analyze(text).expect("analyzes");
        sakatsu_core::dict::readings::correct(&mut analysis, &dictionaries);
        let token = analysis
            .tokens
            .iter()
            .find(|token| token.surface == surface)
            .unwrap_or_else(|| panic!("{surface} is a token of {text}"));
        (token.reading.clone(), token.reading_ambiguous)
    };

    // **Replacing and caveating are separate decisions, and this is where they part.** お父さん
    // lists おとうさん at 200 and おとっさん at -101 "obsolete", so the composed おちちさん is still
    // replaced — no entry lists it — while the caveat is *not* worn, because a rival nobody says
    // is not a second reading. An earlier version of this test asserted the opposite and was
    // right until the margin went in; both behaviours are correct, in that order.
    let (reading, ambiguous) = read("お父さんが来た。", "お父さん");
    assert_eq!(reading, "おとうさん");
    assert!(
        !ambiguous,
        "おとっさん is obsolete at -101; it does not earn a caveat"
    );

    // One listed reading: corrected, and nothing to caveat.
    let (reading, _) = read("お母さんが来た。", "お母さん");
    assert_eq!(reading, "おかあさん");

    // Inflected: flagged, never rewritten — the headword is a different question.
    let (reading, ambiguous) = read("開いた扉。", "開いた");
    assert!(ambiguous);
    assert_eq!(reading, "ひらいた");
}

/// **Listed is not the same as chosen**, and one margin decides both questions.
///
/// 修羅場 is composed しゅらじょう — 修羅 plus the 接尾辞 場, which really is ジョウ — and the
/// dictionary does list しゅらじょう, at -101, against しゅらば at 0. The replacement test asked only
/// whether the reading appeared *at all*, so a reading the source marks as rare survived beside a
/// much stronger one; and the caveat did not fire either, because the gap is 101 and the rival
/// margin is 100. The reading fell exactly between two tests.
///
/// **The counterexamples are why this is a margin and not a ranking.** A blanket "take the highest
/// score" would be wrong on both of these:
///
/// - 肋骨 lists あばらぼね at 0 and ろっこつ at -1. Both are live, the gap is 1, and forcing the
///   higher one would overwrite the reading a medical context wants.
/// - 金 lists かね and きん at 200 apiece, こがね at 99, かな at -101. Two readings tie at the top
///   and nothing about a score says which a sentence means.
///
/// So **two** conditions, and the first is the load-bearing one: the current reading must be
/// *negatively* scored — the source's own "rarely used" marker, the same fact `readings.rs` already
/// relies on where 彼 is かれ at 200 beside かの at -1 — and it must also be outside the margin, so a
/// near-tie between two live readings is caveated rather than overwritten.
///
/// Asking only about the gap was tried, and the novel chapters measured what it cost: see the
/// second group of assertions.
#[test]
#[ignore = "needs the dictionaries"]
fn a_listed_but_far_weaker_reading_is_not_a_supported_selection() {
    let dir = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&dir).into_iter().collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no Jitendex at {}", dir.display());
        return;
    }
    let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    let read = |text: &str, surface: &str| -> (String, bool) {
        let mut analysis = analyzer.analyze(text).expect("analyzes");
        sakatsu_core::dict::readings::correct(&mut analysis, &dictionaries);
        let token = analysis
            .tokens
            .iter()
            .find(|token| token.surface == surface)
            .unwrap_or_else(|| panic!("{surface} is a token of {text}"));
        (token.reading.clone(), token.reading_ambiguous)
    };

    // The reported case: composed しゅらじょう at -101 gives way to しゅらば at 0.
    let (reading, _) = read("修羅場だった。", "修羅場");
    assert_eq!(reading, "しゅらば");

    // **The control the margin exists for.** ろっこつ is negative too, at -1 against あばらぼね at
    // 0 — so rarity alone would take it. A one-point gap is two live readings, not a dead one:
    // kept, and caveated rather than overwritten.
    let (reading, ambiguous) = read("肋骨が折れた。", "肋骨");
    assert_eq!(reading, "ろっこつ", "a one-point gap is not a weak reading");
    assert!(ambiguous, "two live readings earn the caveat");

    // **The controls the *rarity* condition exists for, and they were found by measuring.**
    //
    // An earlier version of this asked only about the gap, and Jitendex pairs a priority-tagged 200
    // with a perfectly live 99 all over the language — a gap of 101, one point past the margin. Run
    // over the two novel chapters it rewrote 身体 からだ to しんたい, 街 まち to がい, 歳 とし to
    // さい and 未だ いまだ to まだ, none of which the dictionary calls rare and the last of which is
    // simply wrong: 未だに is いまだに.
    //
    // A negative score is the source's own "rarely used" marker, which is a different claim from a
    // lower one, and only that is acted on now.
    assert_eq!(read("未だに分からない。", "未だ").0, "いまだ");
    for (text, surface, reading) in [
        ("身体が痛い。", "身体", "しんたい"),
        ("街を歩く。", "街", "まち"),
        ("歳を取る。", "歳", "とし"),
    ] {
        assert_eq!(
            read(text, surface).0,
            reading,
            "{surface}: a 99 beside a 200 is a live reading, not a rare one"
        );
    }

    // Unmoved: decided by the tests either side of this one.
    assert_eq!(read("お父さんが来た。", "お父さん").0, "おとうさん");
}

/// When the whole is not an entry, the longest prefix that *is* one still knows the base's reading.
///
/// **The case above stops one step short of this one.** お母さん is corrected because お母さん is
/// itself an entry; お姉さま is not an entry, so the correction pass had nothing to say and
/// composition's おあねさま stood — 姉 alone really is あね, exactly as 父 is ちち.
///
/// The evidence was there the whole time, one token to the left: **お姉 is an entry and reads
/// おねえ**. A prefixed spelling records the address-form reading of its base, which is the one
/// thing the bare base cannot tell you. Ichiran arrives at おねえ by splitting at the same place.
///
/// **A missing lookup is not a licence.** Where no prefix is known either — お兄 and お婆 are in
/// neither installed dictionary — the composed reading is left exactly as it was rather than
/// reconstructed from something weaker. That is the point of the negative cases below: this reaches
/// as far as the evidence does and stops.
#[test]
#[ignore = "needs the dictionaries"]
fn a_composed_reading_falls_back_to_the_longest_known_prefix() {
    let dir = common::dictionaries_dir().join("jitendex");
    let dictionaries: Vec<Dictionary> = Dictionary::open(&dir).into_iter().collect();
    if dictionaries.is_empty() {
        eprintln!("SKIP: no Jitendex at {}", dir.display());
        return;
    }
    let analyzer = LinderaAnalyzer::new().expect("the dictionary is compiled in");
    let read = |text: &str, surface: &str| -> String {
        let mut analysis = analyzer.analyze(text).expect("analyzes");
        sakatsu_core::dict::readings::correct(&mut analysis, &dictionaries);
        analysis
            .tokens
            .iter()
            .find(|token| token.surface == surface)
            .unwrap_or_else(|| panic!("{surface} is a token of {text}"))
            .reading
            .clone()
    };

    // The reported case, and the same word spelled the other way.
    assert_eq!(read("お姉さまが来た。", "お姉さま"), "おねえさま");
    assert_eq!(read("お姉様が来た。", "お姉様"), "おねえさま");
    // Not a list of the observed words: the rule reaches the rest of the family on the same
    // evidence, and お母さま is not an entry any more than お姉さま is.
    assert_eq!(read("お母さまが来た。", "お母さま"), "おかあさま");

    // **Where no prefix is known, nothing is invented.** お兄 and お婆 are in neither dictionary,
    // so these keep the composed reading — wrong, and honestly wrong, rather than reconstructed
    // from a source that does not exist.
    assert_eq!(read("お兄さまが来た。", "お兄さま"), "おあにさま");
    assert_eq!(read("お婆さまが来た。", "お婆さま"), "おばばさま");

    // Controls: the whole *is* an entry, so none of this is reached and nothing moves.
    assert_eq!(read("お姉さんが来た。", "お姉さん"), "おねえさん");
    assert_eq!(read("お茶を飲む。", "お茶"), "おちゃ");
    assert_eq!(read("お嬢様が来た。", "お嬢様"), "おじょうさま");
    // Controls: ordinary compounds whose parts already spell them. A prefix rule that reached
    // these would be rewriting readings that were never in doubt.
    assert_eq!(read("新入生が来た。", "新入生"), "しんにゅうせい");
    assert_eq!(read("大成功だった。", "大成功"), "だいせいこう");
    assert_eq!(read("小学生が来た。", "小学生"), "しょうがくせい");
    assert_eq!(read("お花見に行く。", "お花見"), "おはなみ");
}

#[test]
#[ignore = "needs the imported Jitendex"]
fn readings_agree_with_the_dictionary() {
    let dir = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&dir) {
        println!("SKIP: no imported Jitendex at {}", dir.display());
        return;
    }
    let dictionary = Dictionary::open(&dir).expect("opens");
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    let mut checked = 0;
    let mut agreed = 0;
    let mut ambiguous: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut disagreed: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut unknown = 0;

    for sentence in SENTENCES {
        let analysis = analyzer.analyze(sentence.text).expect("analyses");
        for token in &analysis.tokens {
            if !has_kanji(&token.surface) || !contains_japanese(&token.surface) {
                continue;
            }

            // **The surface's own reading**, because that is what the reader prints:
            // `<ruby>{surface}<rt>{reading}</rt></ruby>`. Comparing the *lemma* reading
            // against the *dictionary form* — the first thing this test did — measures
            // something nobody sees, and reports 食べた as a 食べる/たべ mismatch when the
            // furigana over it was never claimed to read 食べる.
            //
            // Which limits what can be checked: the dictionary indexes dictionary forms,
            // so it can only confirm a reading for a word that is already in one. An
            // inflected surface has no entry to check against, and inventing one would
            // mean deriving the reading myself — which is exactly the judgement this test
            // exists to avoid making.
            if token.dict_form.is_empty() || token.surface != token.dict_form {
                continue;
            }
            let term = &token.surface;
            let reading = &token.reading;
            if reading.is_empty() {
                continue;
            }

            let hits = dictionary.lookup(term).unwrap_or_default();
            // Only entries spelled exactly as this word; a lookup can also return
            // near-matches, and their readings say nothing about this one.
            //
            // Entries whose "reading" is the spelling carry no reading at all — 辺ぴ is
            // listed as 辺ぴ — and have nothing to say either way.
            let mut listed: Vec<String> = hits
                .iter()
                .filter(|hit| hit.entry.term == *term && hit.entry.reading != *term)
                .map(|hit| to_hiragana(&hit.entry.reading))
                .collect();
            listed.sort();
            listed.dedup();

            if listed.is_empty() {
                unknown += 1;
                continue;
            }

            checked += 1;
            let ours = to_hiragana(reading);
            if listed.contains(&ours) {
                agreed += 1;
                if listed.len() > 1 {
                    ambiguous.insert(term.clone(), listed);
                }
            } else {
                disagreed.push((term.clone(), ours, listed));
            }
        }
    }

    println!("\n=== readings against Jitendex ===");
    println!("  checked          {checked}");
    println!("  agreed           {agreed}");
    println!("  disagreed        {}", disagreed.len());
    println!("  not in the dictionary (not counted)  {unknown}");

    if !ambiguous.is_empty() {
        println!("\n  words the dictionary itself lists more than one reading for:");
        for (term, readings) in &ambiguous {
            println!("    {term}  {}", readings.join(" / "));
        }
        println!("  (any of these is accepted — pinning one would be my preference, not the dictionary's)");
    }

    if !disagreed.is_empty() {
        println!("\n  disagreements — the analyzer read it one way, the dictionary lists another:");
        for (term, ours, listed) in &disagreed {
            println!(
                "    {term}   analyzer: {ours}   dictionary: {}",
                listed.join(" / ")
            );
        }
    }

    // A rate, not a count: the suite's word count will change as sentences are added, and
    // a fixed number would quietly stop meaning the same thing.
    let rate = if checked == 0 {
        1.0
    } else {
        agreed as f64 / checked as f64
    };
    println!("\n  agreement {:.1}%", rate * 100.0);
    assert!(
        rate >= FLOOR,
        "readings agreed on {:.1}% of {checked} words, floor is {:.0}%",
        rate * 100.0,
        FLOOR * 100.0
    );
}

/// Measured at **100% over 147 words** the first time this ran properly, so the floor sits
/// a little under: adding a sentence with a word the dictionary reads unusually should
/// prompt a look, not a red build.
///
/// A ratchet. Raise it if readings improve; never lower it to make a run pass.
///
/// Worth recording what the first run actually found, since it was not what it looked
/// like. Agreement came out at 77% — and every single disagreement was this test comparing
/// the wrong two things, not the analyzer being wrong. It read `lemma_reading` against the
/// dictionary *form*, so an inflected 食べた was reported as "analyzer says たべ,
/// dictionary says たべる". The reader prints `token.reading` over `token.surface`, and
/// once that is what gets compared, the analyzer agrees with the dictionary everywhere.
const FLOOR: f64 = 0.95;
