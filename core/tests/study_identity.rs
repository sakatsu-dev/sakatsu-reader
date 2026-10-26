//! Moving the study path into `core` changed nothing about what it produces.
//!
//! # Why a frozen copy of the old code lives here
//!
//! `commands::study` assembled the response inline: analyzer, joins, reading correction, grammar
//! breakdown, patterns, sentence ranges, study keys, statuses, furigana. That moved to
//! `core::study` so it could be called without constructing a Tauri app. An extraction like that
//! is only worth doing if it is provably behaviour-preserving, and "I read both and they look the
//! same" is exactly the standard this branch has been burned by — twice now.
//!
//! So [`frozen`] is the pre-move sequence, transcribed from `commands.rs`, and the comparison is
//! on the **whole serialised `StudyView`** rather than on the tokens. The fields most likely to
//! diverge are the ones a token comparison cannot see: `breakdowns` and `ruby` are parallel arrays
//! whose alignment to the token list is the thing at risk, and `statuses` depends on a store.
//!
//! **This module is deliberately temporary.** It goes when the migration is committed; keeping it
//! would resurrect the duplication the extraction removed.
//!
//! # It runs with nothing installed
//!
//! The fixture dictionary is built here and the kanji table is loaded from a fixture written here,
//! so this gates an ordinary `cargo test` run — the pattern `pipeline_identity.rs` set.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::KanjiDic;
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::store::{Store, WordKey, WordStatus};
use sakatsu_core::study::{self, Resources, StudyView};
use serde_json::json;

/// The pre-move sequence, transcribed from `commands::study` and not to be tidied.
///
/// Reformatting this would defeat its only purpose. The one intended difference is that the four
/// `State` acquisitions become the four references a test can hold — which is the change the
/// extraction is *for*, and the only one.
fn frozen(
    text: &str,
    analyzer: &dyn Analyzer,
    dictionaries: &[Dictionary],
    kanji: &KanjiDic,
    store: Option<&Store>,
) -> StudyView {
    let mut analysis = analyzer.analyze(text).expect("analyzes");
    analysis.tokens = Pipeline::new(dictionaries).segmentation(&analysis.tokens).0;
    sakatsu_core::dict::readings::correct(&mut analysis, dictionaries);

    let breakdowns = analysis
        .tokens
        .iter()
        .map(sakatsu_core::grammar::breakdown)
        .collect::<Vec<_>>();
    let patterns = sakatsu_core::grammar::patterns(&analysis);
    let sentences = sakatsu_core::model::sentence_ranges(&analysis.tokens);

    let keys: Vec<WordKey> = analysis.tokens.iter().map(WordKey::for_token).collect();
    let statuses = match store {
        Some(store) => store.statuses(&keys),
        None => vec![WordStatus::default(); keys.len()],
    };

    let ruby = analysis
        .tokens
        .iter()
        .map(|token| sakatsu_core::dict::furigana(&token.surface, &token.reading, kanji))
        .collect();

    StudyView {
        analysis,
        sentences,
        breakdowns,
        patterns,
        statuses,
        ruby,
    }
}

/// Passages chosen to make every stage produce something: a compound the dictionary rejoins, a
/// place name, a contested reading, grammar to break down, and kanji to set ruby over.
const CORPUS: &[&str] = &[
    "受付係に聞いてください。",
    "善福寺川の近くに住んでいます。",
    "その者は何も言わなかった。",
    "彼は毎日図書館で勉強している。",
    "ここって禁足地なんじゃなかったっけ。",
];

/// A dictionary written for this test, so the check runs with nothing installed.
fn fixture(dir: &Path) -> PathBuf {
    let path = dir.join("fixture.zip");
    let file = std::fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    for (name, contents) in [
        (
            "index.json",
            json!({ "title": "Study Fixture", "format": 3 }).to_string(),
        ),
        (
            "term_bank_1.json",
            json!([
                [
                    "受付係",
                    "うけつけがかり",
                    "n",
                    "",
                    20,
                    ["receptionist"],
                    1,
                    ""
                ],
                ["受付", "うけつけ", "n", "", 30, ["reception"], 2, ""],
                ["係", "かかり", "n", "", 30, ["person in charge"], 3, ""],
                [
                    "善福寺川",
                    "ぜんぷくじがわ",
                    "place",
                    "",
                    10,
                    ["Zenpukujigawa"],
                    4,
                    ""
                ],
                ["者", "もの", "n", "", 20, ["person"], 5, ""],
                ["者", "しゃ", "n", "", 20, ["-er; -ist"], 6, ""],
                ["図書館", "としょかん", "n", "", 40, ["library"], 7, ""],
                ["勉強", "べんきょう", "n vs", "", 40, ["study"], 8, ""]
            ])
            .to_string(),
        ),
    ] {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .expect("member");
        writer.write_all(contents.as_bytes()).expect("write");
    }
    writer.finish().expect("finish");
    path
}

/// A few kanji, in the tab-separated shape `KanjiDic::load` reads, so ruby is not empty.
fn kanji_fixture(dir: &Path) -> KanjiDic {
    let path = dir.join("kanjidic.tsv");
    let rows = [
        ("図", "ズ\tト", "はか-る"),
        ("書", "ショ", "か-く"),
        ("館", "カン", "やかた"),
        ("勉", "ベン", ""),
        ("強", "キョウ\tゴウ", "つよ-い"),
        ("者", "シャ", "もの"),
        ("受", "ジュ", "う-ける"),
        ("付", "フ", "つ-ける"),
        ("係", "ケイ", "かかり"),
    ];
    let body: String = rows
        .iter()
        .map(|(character, on, kun)| format!("{character}\t{on}\t{kun}\n"))
        .collect();
    std::fs::write(&path, body).expect("write kanjidic");
    KanjiDic::load(&path)
}

fn rendered(view: &StudyView) -> String {
    serde_json::to_string_pretty(view).expect("a study view serialises")
}

/// The extraction is behaviour-preserving, with and without a store.
#[test]
fn the_extracted_study_service_matches_the_code_it_replaced() {
    let scratch = common::Scratch::new("study-identity", "response");

    let archive = fixture(&scratch);
    let dictionary = Dictionary::import(&archive, &scratch.join("out")).expect("import");
    let dictionaries = [dictionary];
    let kanji = kanji_fixture(&scratch);
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");

    // A store with something marked in it, so `statuses` is not a vector of one repeated value —
    // which would compare equal however wrongly it were aligned.
    let store = Store::open(&scratch.join("store")).expect("open");
    let marked = analyzer.analyze(CORPUS[3]).expect("analyzes");
    let mut marks = 0;
    for token in marked
        .tokens
        .iter()
        .filter(|token| !token.surface.is_empty())
    {
        if marks >= 3 {
            break;
        }
        store
            .set_status(WordKey::for_token(token), WordStatus::Known)
            .expect("mark");
        marks += 1;
    }
    assert!(
        marks > 0,
        "the store must hold marks, or statuses proves nothing"
    );

    for store in [Some(&store), None] {
        let label = if store.is_some() {
            "with a store"
        } else {
            "with none"
        };
        for text in CORPUS {
            let mut was = frozen(text, &analyzer, &dictionaries, &kanji, store);
            // **The one deliberate addition since the extraction, applied explicitly.** Production
            // now marks which tokens and parts a reader may select (`analyzer::mark_selectable`);
            // the transcribed sequence predates that field. Applying exactly that step here keeps
            // every other field of the view compared byte for byte, rather than weakening the
            // comparison to ignore the flag.
            sakatsu_core::analyzer::mark_selectable(&mut was.analysis);
            let now = study::study(
                text,
                Resources {
                    analyzer: &analyzer,
                    dictionaries: &dictionaries,
                    kanji: &kanji,
                    store,
                },
            )
            .expect("the service analyses what the frozen copy did");

            assert_eq!(
                rendered(&was),
                rendered(&now),
                "{label}: the extraction changed the study view of {text:?}"
            );
        }
    }

    // Teeth: a comparison over a corpus where the derivations produce nothing would pass whatever
    // changed. Each of the four parallel arrays must be non-trivial at least once.
    let view = study::study(
        CORPUS[3],
        Resources {
            analyzer: &analyzer,
            dictionaries: &dictionaries,
            kanji: &kanji,
            store: Some(&store),
        },
    )
    .expect("analyses");
    assert_eq!(
        view.breakdowns.len(),
        view.analysis.tokens.len(),
        "breakdowns are parallel to the tokens"
    );
    assert_eq!(
        view.ruby.len(),
        view.analysis.tokens.len(),
        "and so is ruby"
    );
    assert_eq!(
        view.statuses.len(),
        view.analysis.tokens.len(),
        "and so are statuses"
    );
    assert!(
        view.ruby.iter().any(|token| token.len() > 1),
        "at least one token must be set as furigana over its parts, or the kanji fixture is inert"
    );
    assert!(
        view.statuses
            .iter()
            .any(|status| *status != WordStatus::default()),
        "at least one token must carry a mark, or the store comparison is vacuous"
    );
    assert!(
        !view.sentences.is_empty(),
        "and the passage must partition into at least one sentence"
    );

    drop(store);
    drop(dictionaries);
}

/// `analyse` is a strict prefix of `study`: same tokens, same readings, same components.
///
/// The two commands disagreeing about their readings is a bug this project has already had, and
/// the reason `analyze` was changed to run the full display pipeline. Now that both call one
/// service the property is structural, and this is what says so.
#[test]
fn analyse_is_exactly_the_prefix_of_study() {
    let scratch = common::Scratch::new("study-identity", "prefix");
    let archive = fixture(&scratch);
    let dictionary = Dictionary::import(&archive, &scratch.join("out")).expect("import");
    let dictionaries = [dictionary];
    let kanji = kanji_fixture(&scratch);
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");

    for text in CORPUS {
        let alone = study::analyse(text, &analyzer, &dictionaries).expect("analyses");
        let within = study::study(
            text,
            Resources {
                analyzer: &analyzer,
                dictionaries: &dictionaries,
                kanji: &kanji,
                store: None,
            },
        )
        .expect("analyses");
        assert_eq!(
            serde_json::to_string(&alone).expect("serialises"),
            serde_json::to_string(&within.analysis).expect("serialises"),
            "analyze and study must not disagree about {text:?}"
        );
    }

    drop(dictionaries);
}

/// A real passage produces a real `grammar::patterns` match.
///
/// # Why this is separate from the golden's hand-built one
///
/// `ui/src/model.golden.json` carries a hand-built `Match`, because `grammar::patterns` finds
/// nothing in that fixture and a derived empty vector would leave the type unexposed to the
/// contract check. That is fine for what the golden is for — it checks the **DTO shape** — but it
/// proves nothing about the matcher, and a serialization fixture standing in for behaviour is
/// exactly the confusion this project keeps having to undo.
///
/// So this asks the other question: does the matcher, over real analyzer output, produce matches
/// whose indices and spans are consistent with the tokens they claim?
#[test]
fn the_matcher_produces_real_matches_over_real_analysis() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
    // Particles are the densest patterns in the core pack, and every one of these sentences has
    // several. No dictionary is needed: `grammar::patterns` reads the tokens.
    let passages = [
        "彼は毎日図書館で勉強している。",
        "その問題については明日話し合いましょう。",
        "天気が良ければ公園に行きます。",
    ];

    let mut total = 0usize;
    for text in passages {
        let analysis = analyzer.analyze(text).expect("analyzes");
        let patterns = sakatsu_core::grammar::patterns(&analysis);
        assert!(
            !patterns.is_empty(),
            "no pattern matched {text:?}, which is a passage full of particles"
        );

        for matched in &patterns {
            assert!(
                matched.first_token < matched.last_token_end,
                "{text:?}: {} spans no tokens ({}..{})",
                matched.pattern_id,
                matched.first_token,
                matched.last_token_end
            );
            assert!(
                matched.last_token_end <= analysis.tokens.len(),
                "{text:?}: {} runs past the token list",
                matched.pattern_id
            );
            // `find`, not `core().get`: the matcher searches every loaded pack, and an earlier
            // version of this test asserted the core pack alone — which failed on
            // `hanabira-n5-mashou` from the imported N5 pack. The property that matters is that
            // the id resolves to *something* the grammar panel can render, not which file it
            // came from.
            assert!(
                sakatsu_core::grammar::pack::find(&matched.pattern_id).is_some(),
                "{text:?}: {} resolves to no pattern in any pack, so nothing can explain it",
                matched.pattern_id
            );
            // A grounded match carries the byte span of the tokens it covers.
            if let (Some(start), Some(end)) = (matched.start, matched.end) {
                assert!(start < end && end <= text.len());
                assert_eq!(
                    analysis.tokens[matched.first_token].start,
                    Some(start),
                    "{text:?}: {} starts where its first token does",
                    matched.pattern_id
                );
            }
        }
        total += patterns.len();
    }
    assert!(
        total >= 3,
        "only {total} matches over three particle-heavy passages, which reads like a matcher that \
         has stopped matching rather than a corpus that is clean"
    );
}
