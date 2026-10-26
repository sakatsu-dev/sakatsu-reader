//! Grouping as the **reader** receives it: through `study::analyse`, and on to `study::study`.
//!
//! # The question only this level answers
//!
//! Selection decides. Until assembly runs, nothing the reader sees has changed, and every test up
//! to this point is about proposals. These are about output: the tokens the interface renders,
//! the word key the reader's status is stored under, the components they can still select, and
//! the grammar and readings recomputed over the assembled sequence.
//!
//! # One path
//!
//! Every test here goes through `study::analyse` — the same call the commands make. There is
//! deliberately no test that assembles by another route, because a test that did would be
//! checking something the reader does not get.
//!
//! Run against the isolated copy; the reader's own store is never opened.

mod common;

use std::path::PathBuf;

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::{Analysis, Token};
use sakatsu_core::store::WordKey;

fn dictionaries() -> Vec<Dictionary> {
    let dir = PathBuf::from(
        std::env::var_os("SAKATSU_LEXICAL_DICT")
            .expect("SAKATSU_LEXICAL_DICT must point at an isolated dictionary copy"),
    );
    let opened: Vec<Dictionary> = Dictionary::discover(&dir)
        .iter()
        .map(|path| Dictionary::open(path).expect("the copy opens"))
        .collect();
    assert!(!opened.is_empty(), "no dictionary under {}", dir.display());
    assert!(
        opened.iter().any(Dictionary::has_sequence),
        "the copy has no sequence bucket"
    );
    opened
}

/// What the reader is given, through the call the commands make.
fn reader_sees(text: &str, dictionaries: &[Dictionary]) -> Analysis {
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    sakatsu_core::study::analyse(text, &analyzer, dictionaries).expect("the passage analyses")
}

/// The tokens the reader would have seen **without grouping**.
///
/// The analyzer *and the display pipeline*, because that is what grouping is a change to. The
/// pipeline composes 深夜 + 徘徊 into 深夜徘徊 and joins names; comparing against the raw analyzer
/// would credit grouping with the pipeline's work, and — worse — report a passage as changed when
/// grouping left it alone.
#[allow(dead_code)]
fn ungrouped(text: &str) -> Analysis {
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let mut analysis =
        sakatsu_core::analyzer::Analyzer::analyze(&analyzer, text).expect("it analyses");
    sakatsu_core::pipeline::Pipeline::new(&[]).display(&mut analysis);
    analysis
}

/// The same, with the dictionaries the reader has — the pipeline consults them.
fn ungrouped_with(text: &str, dictionaries: &[Dictionary]) -> Analysis {
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let mut analysis =
        sakatsu_core::analyzer::Analyzer::analyze(&analyzer, text).expect("it analyses");
    sakatsu_core::pipeline::Pipeline::new(dictionaries).display(&mut analysis);
    analysis
}

fn surfaces(analysis: &Analysis) -> Vec<&str> {
    analysis
        .tokens
        .iter()
        .map(|token| token.surface.as_str())
        .collect()
}

fn find<'a>(analysis: &'a Analysis, surface: &str) -> &'a Token {
    analysis
        .tokens
        .iter()
        .find(|token| token.surface == surface)
        .unwrap_or_else(|| panic!("{surface} is not a token: {:?}", surfaces(analysis)))
}

/// **The acceptance case, as output.**
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_split_adjective_reaches_the_reader_as_one_word() {
    let dictionaries = dictionaries();
    let text = "かっこいい曲";
    println!(
        "  baseline {:?}",
        surfaces(&ungrouped_with(text, &dictionaries))
    );
    let seen = reader_sees(text, &dictionaries);
    println!("  reader   {:?}", surfaces(&seen));

    assert!(
        surfaces(&ungrouped_with(text, &dictionaries)).contains(&"かっこ"),
        "the analyzer must still split it, or this proves nothing"
    );
    assert_eq!(surfaces(&seen), vec!["かっこいい", "曲"]);

    let unit = find(&seen, "かっこいい");
    assert_eq!(unit.dict_form, "格好いい");
    assert_eq!(unit.reading, "かっこいい");
    assert_eq!(unit.lemma_reading, "かっこいい");

    // **The word key**, which is what the reader's Known/Seen status is filed under and what the
    // dictionary lookup asks for. A unit whose key were still かっこ's would store the status of
    // a different word.
    assert_eq!(
        WordKey::for_token(unit),
        WordKey::new("格好いい", "かっこいい")
    );

    // **Component access, retained.** かっこ and いい are still there to select.
    assert_eq!(
        unit.components
            .iter()
            .map(|token| token.surface.as_str())
            .collect::<Vec<_>>(),
        vec!["かっこ", "いい"]
    );
    // …and each keeps its own key, so selecting a component stores the component's status.
    assert_eq!(
        WordKey::for_token(&unit.components[0]).term,
        ungrouped_with(text, &dictionaries).tokens[0].dict_form
    );

    // A composed token does not inherit a constituent's role.
    assert!(unit.lexical_role.is_none());
}

/// **The inflected case**, where the unit's lemma is not any constituent's.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_inflected_adjective_reaches_the_reader_with_its_own_lemma() {
    let dictionaries = dictionaries();
    let text = "かっこよかった曲";
    let seen = reader_sees(text, &dictionaries);
    println!("  reader {:?}", surfaces(&seen));
    assert_eq!(surfaces(&seen), vec!["かっこよかった", "曲"]);

    let unit = find(&seen, "かっこよかった");
    assert_eq!(unit.dict_form, "格好いい");
    // The analyzer's reading of the source, and the dictionary form's own reading. **Different**,
    // and both needed: the first drives furigana, the second the lookup.
    assert_eq!(unit.reading, "かっこよかった");
    assert_eq!(unit.lemma_reading, "かっこいい");
    assert_eq!(
        WordKey::for_token(unit),
        WordKey::new("格好いい", "かっこいい")
    );

    // **The conjugation names the unit, not its head.** よかった's lemma is よい; this word's is
    // 格好いい, and a conjugation still saying よい would send status and lookups elsewhere.
    let conjugation = unit
        .conjugation
        .as_ref()
        .expect("an inflected unit keeps its form");
    assert_eq!(conjugation.lemma, "格好いい");
    assert_eq!(conjugation.reading, "かっこいい");
    assert!(
        !conjugation.props.is_empty(),
        "the form the head carried is the form the unit is in"
    );
    println!(
        "  conjugation {} ({}) {:?}",
        conjugation.lemma,
        conjugation.reading,
        conjugation
            .props
            .iter()
            .map(|prop| (prop.pos.as_str(), prop.form.as_str()))
            .collect::<Vec<_>>()
    );
}

/// **The construction case**, and the listed item between the endpoints.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn both_enumerative_endpoints_reach_the_reader_as_words() {
    let dictionaries = dictionaries();
    let text = "映画とか買い物とかします。";
    let seen = reader_sees(text, &dictionaries);
    println!(
        "  baseline {:?}",
        surfaces(&ungrouped_with(text, &dictionaries))
    );
    println!("  reader   {:?}", surfaces(&seen));

    assert_eq!(
        surfaces(&seen),
        vec!["映画", "とか", "買い物", "とか", "します", "。"]
    );
    for unit in seen.tokens.iter().filter(|token| token.surface == "とか") {
        assert_eq!(unit.dict_form, "とか");
        assert_eq!(
            unit.components
                .iter()
                .map(|token| token.surface.as_str())
                .collect::<Vec<_>>(),
            vec!["と", "か"],
            "the components stay selectable"
        );
        // A particle construction is not inflected, and nothing invents a conjugation for it.
        assert!(unit.conjugation.is_none());
    }
}

/// **Grouping an already-grouped passage forms nothing new.**
///
/// `study::analyse` groups once. Nothing should run it twice — but "nothing should" is a claim
/// about every call site, now and later, and the cheap way to make it not matter is for the
/// second pass to be a fixed point. A unit's constituents live in `components`, not in the token
/// list, so there is no seam left to join; this asserts that rather than assuming it.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn grouping_what_is_already_grouped_changes_nothing() {
    let dictionaries = dictionaries();
    for text in [
        "かっこいい曲",
        "かっこよかった曲",
        "映画とか買い物とかします。",
    ] {
        let once = reader_sees(text, &dictionaries);
        let twice = sakatsu_core::lexical::regrouped(&once, &dictionaries);
        assert!(
            twice.applied.is_empty(),
            "{text}: a second pass formed {} more unit(s)",
            twice.applied.len()
        );
        assert_eq!(
            surfaces(&twice.analysis),
            surfaces(&once),
            "{text}: a second pass changed the tokens"
        );
        assert!(twice.is_sound(), "{text}: {}", twice.summary());
    }
}

/// **The abstention cases reach the reader untouched**, which is the promise.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn a_passage_with_nothing_supported_is_unchanged() {
    let dictionaries = dictionaries();
    for text in [
        "かっこを書く",
        "深夜徘徊 するで",
        "図書館で本を読んでいる人がたくさんいますが、今日は静かです。",
        "明日は雨が降るとか聞きました。",
        "かわいい人",
    ] {
        let seen = reader_sees(text, &dictionaries);
        assert_eq!(
            surfaces(&seen),
            surfaces(&ungrouped_with(text, &dictionaries)),
            "{text}: the reader's tokens must be the analyzer's"
        );
    }
}

/// **The assembled sequence satisfies the span contract**, which nothing downstream re-checks.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn what_the_reader_receives_tiles_its_own_source() {
    let dictionaries = dictionaries();
    for text in [
        "かっこいい曲",
        "かっこよかった曲",
        "映画とか買い物とかします。",
        "図書館で本を読んでいる人がたくさんいますが、今日は静かです。",
    ] {
        let seen = reader_sees(text, &dictionaries);
        assert!(
            sakatsu_core::analyzer::validate_analysis(&seen).is_empty(),
            "{text}: the reader was given tokens that do not tile their source"
        );
        for token in &seen.tokens {
            let (start, end) = (token.start.expect("grounded"), token.end.expect("grounded"));
            assert_eq!(&seen.text[start..end], token.surface, "{text}");
        }
    }
}

/// **The two commands agree.** `analyse` is a strict prefix of `study`, so the words the analyze
/// command reports and the words the study command reports are the same words.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_analyze_and_study_commands_see_the_same_words() {
    let dictionaries = dictionaries();
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    for text in [
        "かっこいい曲",
        "かっこよかった曲",
        "映画とか買い物とかします。",
    ] {
        let analysed = sakatsu_core::study::analyse(text, &analyzer, &dictionaries).expect("ok");
        let studied = sakatsu_core::study::study(
            text,
            sakatsu_core::study::Resources {
                analyzer: &analyzer,
                dictionaries: &dictionaries,
                store: None,
                kanji: &sakatsu_core::dict::KanjiDic::default(),
            },
        )
        .expect("ok");
        assert_eq!(
            analysed, studied.analysis,
            "{text}: the two commands disagree about the tokens"
        );
    }
}

/// **The study view is recomputed over the assembled sequence**, not over the baseline.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn the_study_view_describes_the_words_the_reader_sees() {
    let dictionaries = dictionaries();
    let analyzer =
        sakatsu_core::analyzer::LinderaAnalyzer::new().expect("the embedded UniDic loads");
    let studied = sakatsu_core::study::study(
        "かっこよかった曲",
        sakatsu_core::study::Resources {
            analyzer: &analyzer,
            dictionaries: &dictionaries,
            store: None,
            kanji: &sakatsu_core::dict::KanjiDic::default(),
        },
    )
    .expect("ok");

    // Everything parallel to the tokens is parallel to the *assembled* tokens.
    let count = studied.analysis.tokens.len();
    assert_eq!(studied.breakdowns.len(), count);
    assert_eq!(studied.statuses.len(), count);
    assert_eq!(studied.ruby.len(), count);
    assert_eq!(surfaces(&studied.analysis), vec!["かっこよかった", "曲"]);

    // **The form is explained through the constituent that carries it.**
    //
    // `grammar::ladder` recurses into a compound's components, so the step names よい → よかった
    // and marks which component it belongs to. That is the honest account: the transformation
    // happened to よい, and 格好いい's own past is かっこよかった by suppletion — a rung generated
    // from the unit's lemma reading would read かっこかった, which is not a word.
    //
    // Asserted rather than assumed, because if `ladder` ever stops recursing, the reader would be
    // shown a fabricated formation and nothing else here would notice.
    let breakdown = &studied.breakdowns[0];
    println!("  breakdown {breakdown:?}");
    let step = breakdown
        .steps
        .first()
        .expect("an inflected unit has a form to explain");
    assert_eq!(step.label, "Past (~ta)");
    assert_eq!(step.pos, "adj-i");
    assert_eq!(step.lemma, "よい", "the step is about the constituent");
    assert_eq!(
        step.component,
        Some(1),
        "and says which constituent it is about"
    );
    let formation = step.formation.as_ref().expect("a formation");
    assert_eq!(
        (formation.from.as_str(), formation.result.as_str()),
        ("よい", "よかった")
    );
    assert_ne!(
        formation.result, "かっこかった",
        "the suppletion must never be generated away"
    );

    // The unit's own conjugation still names the unit, for everything that asks which word this
    // is rather than how its form was built.
    let conjugation = studied.analysis.tokens[0]
        .conjugation
        .as_ref()
        .expect("the unit is an inflected word");
    assert_eq!(conjugation.lemma, "格好いい");
    assert_eq!(conjugation.reading, "かっこいい");

    // And the ruby is laid over the unit's own surface and reading.
    let ruby: String = studied.ruby[0]
        .iter()
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(ruby, "かっこよかった");
}

/// **Facts the desktop checklist needs**, rather than examples chosen by eye.
///
/// A row that says "paste this and look for ruby" is worthless if the grouped word has no kanji,
/// and a row that says "open a part of a part if one is offered" is not an acceptance case at
/// all. This reports, for short pasteable passages: which group, whether the unit's surface
/// carries kanji, whether its reading differs from its surface — both are needed for ruby to be
/// drawn at all — and how deep its components nest.
#[test]
#[ignore = "needs a verified isolated dictionary copy in SAKATSU_LEXICAL_DICT"]
fn short_passages_for_the_desktop_checklist() {
    let dictionaries = dictionaries();
    fn has_kanji(text: &str) -> bool {
        text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }
    fn depth(token: &Token) -> usize {
        1 + token.components.iter().map(depth).max().unwrap_or(0)
    }

    for text in [
        "かっこいい曲",
        "かっこよかった曲",
        "映画とか買い物とかします。",
        "味は申し分ない。",
        "本当に申し訳ない。",
        "つけ麺の王様は間違いなくここ。",
        "それは水っぽく感じた。",
        "それは問題無く終わった。",
        "気持ちよく眠れた。",
        "気持ちよかったです。",
    ] {
        let seen = reader_sees(text, &dictionaries);
        let units: Vec<&Token> = seen
            .tokens
            .iter()
            .filter(|token| !token.components.is_empty())
            .collect();
        if units.is_empty() {
            println!("  {text:<26} — no grouping");
            continue;
        }
        for unit in units {
            println!(
                "  {text:<26} {:<12} kanji={:<5} reading={:<14} ruby={:<5} depth={} parts={:?}",
                unit.surface,
                has_kanji(&unit.surface),
                unit.reading,
                has_kanji(&unit.surface) && unit.reading != unit.surface,
                depth(unit),
                unit.components
                    .iter()
                    .map(|part| (part.surface.as_str(), part.components.len()))
                    .collect::<Vec<_>>()
            );
        }
    }
}
