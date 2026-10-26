//! Moving the lookup path into `core` changed nothing about what it produces.
//!
//! # Why a frozen copy of the old code lives here
//!
//! `commands::look_up_sources`, `look_up_token` and `lookup_in_context_uncached` were private
//! helpers in the desktop shell. They moved to `core::lookup` so the query policy could be called
//! without constructing a Tauri app. An extraction is only worth doing if it is provably
//! behaviour-preserving, and "I read both and they look the same" is the standard this branch has
//! been burned by more than once.
//!
//! So [`frozen_sources`], [`frozen_word`] and [`frozen_in_context`] are the pre-move code,
//! transcribed from `commands.rs` and **not to be tidied** — reformatting them would defeat their
//! only purpose. The one intended difference is that the `State<DictionaryState>` acquisition
//! becomes the `&[Dictionary]` a test can hold, which is the change the extraction is *for* and
//! the only one.
//!
//! # What is compared
//!
//! The **whole serialised view**, not a field or two, for **every token** of every passage — so a
//! difference in ordering, in which source was promoted, or in a field nothing here thought to
//! name still fails. `ContextualWordView` is compared the same way, because its ranking is
//! derived from the hits and is exactly the thing a refactor of the source loop could disturb
//! without touching `WordView` at all.
//!
//! The corpus is chosen so each named kind of word actually occurs: an ordinary word, a name, two
//! contested readings, the explanatory なん (answered from the grammar pack, never the
//! dictionary), contracted motion, and an unknown katakana word that reaches the likely-name
//! guess. A test whose corpus produces only ordinary words would compare two implementations on
//! the one case they cannot differ about.
//!
//! **This module is deliberately temporary.** It goes when the migration is committed; keeping it
//! would resurrect the duplication the extraction removed.
//!
//! # It runs with nothing installed
//!
//! The fixture dictionary is built here and UniDic is embedded, so this gates an ordinary
//! `cargo test` run — the pattern `pipeline_identity.rs` set and `study_identity.rs` followed.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::dict::context::{contextualize_with_lookup, ContextualWordView};
use sakatsu_core::dict::index::{Dictionary, MatchKind};
use sakatsu_core::dict::merge::{self, SourceResult, WordView};
use sakatsu_core::lookup;
use sakatsu_core::model::Token;
use sakatsu_core::pipeline::Pipeline;
use serde_json::json;

/// The pre-move `commands::look_up_sources`, transcribed. Do not tidy.
fn frozen_sources(dictionaries: &[Dictionary], token: &Token) -> Result<Vec<SourceResult>, String> {
    let query = if token.dict_form.is_empty() {
        token.surface.clone()
    } else {
        token.dict_form.clone()
    };

    let fallbacks: Vec<String> = {
        let mut parts: Vec<String> = token
            .components
            .iter()
            .map(|part| {
                if part.dict_form.is_empty() {
                    part.surface.clone()
                } else {
                    part.dict_form.clone()
                }
            })
            .filter(|part| !part.is_empty() && *part != query)
            .collect();
        parts.sort_by_key(|part| std::cmp::Reverse(part.chars().count()));
        parts
    };

    let mut sources = Vec::new();
    for dictionary in dictionaries {
        let mut hits = dictionary
            .lookup(&query)
            .map_err(|error| error.to_string())?;
        // **The one addition to the transcription**, and it changes no behaviour: `SourceResult`
        // gained a field recording which query answered, so this has to say what the frozen code
        // was already doing. It is set from the same branch the frozen code takes, which is what
        // keeps the comparison honest — the alternative, defaulting it, would let the two paths
        // differ here without the identity check noticing.
        let mut retrieved = merge::Retrieved::Whole {
            query: query.clone(),
        };
        if hits.is_empty() {
            for part in &fallbacks {
                hits = dictionary.lookup(part).map_err(|error| error.to_string())?;
                if !hits.is_empty() {
                    retrieved = merge::Retrieved::Component {
                        query: part.clone(),
                    };
                    break;
                }
            }
        }
        merge::prefer_matching_reading(&mut hits, &token.reading);
        if !hits.is_empty() {
            sources.push(SourceResult {
                title: dictionary.meta().title.clone(),
                hits,
                retrieved,
            });
        }
    }
    Ok(sources)
}

/// The pre-move `commands::look_up_token`, transcribed. Do not tidy.
fn frozen_word(dictionaries: &[Dictionary], token: &Token) -> Result<WordView, String> {
    Ok(merge::merge(token, &frozen_sources(dictionaries, token)?))
}

/// The pre-move `commands::lookup_in_context_uncached`, transcribed. Do not tidy.
fn frozen_in_context(
    dictionaries: &[Dictionary],
    tokens: &[Token],
    index: usize,
) -> Result<ContextualWordView, String> {
    let token = tokens
        .get(index)
        .ok_or_else(|| format!("token index {index} is outside the passage"))?;
    let sources = frozen_sources(dictionaries, token)?;
    let word = merge::merge(token, &sources);
    let trustworthy_lookup = word.inline_source.as_deref().is_none_or(|promoted| {
        sources
            .iter()
            .find(|source| source.title == promoted)
            .is_some_and(|source| {
                source
                    .hits
                    .iter()
                    .any(|hit| matches!(hit.matched, MatchKind::Spelling | MatchKind::Reading))
            })
    });
    Ok(contextualize_with_lookup(
        word,
        tokens,
        index,
        trustworthy_lookup,
    ))
}

/// Passages chosen so every kind of answer occurs at least once.
///
/// Each line is here for a reason, and losing one loses a case:
///
/// | Passage | What it reaches |
/// |---|---|
/// | 受付係に… | a compound the dictionary does not have, answered from its longest part |
/// | 善福寺川の… | a place name, which the merge policy answers separately from a meaning |
/// | その者は何も… | 者 as もの or しゃ — the reading decides which entry leads |
/// | 市場で野菜を… | 市場 as しじょう or いちば, the contested reading the guess mark is drawn on |
/// | ここって禁足地なん… | the explanatory なん, answered from the pack while the dictionary's 何 and だ are suppressed |
/// | へえ今度アタシも連れてって… | contracted motion, and アタシ which nothing has — the likely-name guess |
const CORPUS: &[&str] = &[
    "受付係に聞いてください。",
    "善福寺川の近くに住んでいます。",
    "その者は何も言わなかった。",
    "市場で野菜を買いました。",
    "ここって禁足地なんじゃなかったっけ。",
    "へえ今度アタシも連れてってよ",
];

/// A dictionary written for this test, so the check runs with nothing installed.
///
/// It deliberately holds the entries that are **wrong** for なん — 何 "what" and だ the copula.
/// Both are reachable from that token and both would mislead, which is why the construction is
/// answered from the grammar pack instead; a fixture without them could not tell a working
/// suppression from an empty dictionary.
fn fixture(dir: &Path) -> PathBuf {
    let path = dir.join("fixture.zip");
    let file = std::fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    for (name, contents) in [
        (
            "index.json",
            json!({ "title": "Lookup Fixture", "format": 3 }).to_string(),
        ),
        (
            "term_bank_1.json",
            json!([
                ["受付", "うけつけ", "n", "", 30, ["reception"], 1, ""],
                ["係", "かかり", "n", "", 30, ["person in charge"], 2, ""],
                ["聞く", "きく", "v5k vt", "", 40, ["to hear; to ask"], 3, ""],
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
                ["近く", "ちかく", "n", "", 40, ["near; nearby"], 5, ""],
                [
                    "住む",
                    "すむ",
                    "v5m vi",
                    "",
                    40,
                    ["to live; to reside"],
                    6,
                    ""
                ],
                ["者", "もの", "n", "", 20, ["person"], 7, ""],
                ["者", "しゃ", "n", "", 20, ["-er; -ist"], 8, ""],
                ["何", "なに", "pn", "", 40, ["what"], 9, ""],
                ["何", "なん", "pn", "", 40, ["what; how many"], 10, ""],
                ["言う", "いう", "v5u", "", 50, ["to say"], 11, ""],
                [
                    "市場",
                    "しじょう",
                    "n",
                    "",
                    30,
                    ["market (financial)"],
                    12,
                    ""
                ],
                ["市場", "いちば", "n", "", 30, ["marketplace"], 13, ""],
                ["野菜", "やさい", "n", "", 40, ["vegetable"], 14, ""],
                [
                    "買う",
                    "かう",
                    "v5u vt",
                    "",
                    40,
                    ["to buy; to purchase"],
                    15,
                    ""
                ],
                ["だ", "だ", "cop", "", 50, ["be; is"], 16, ""],
                [
                    "禁足地",
                    "きんそくち",
                    "n",
                    "",
                    10,
                    ["forbidden ground"],
                    17,
                    ""
                ],
                [
                    "連れる",
                    "つれる",
                    "v1 vt",
                    "",
                    30,
                    ["to take along"],
                    18,
                    ""
                ],
                ["行く", "いく", "v5k-s vi", "", 50, ["to go"], 19, ""],
                [
                    "今度",
                    "こんど",
                    "n",
                    "",
                    40,
                    ["next time; this time"],
                    20,
                    ""
                ]
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

fn rendered<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).expect("serialises")
}

/// The tokens a passage produces, through the same pipeline the app runs before any lookup.
fn tokens_of(text: &str, analyzer: &dyn Analyzer, dictionaries: &[Dictionary]) -> Vec<Token> {
    let mut analysis = analyzer.analyze(text).expect("analyzes");
    analysis.tokens = Pipeline::new(dictionaries).segmentation(&analysis.tokens).0;
    sakatsu_core::dict::readings::correct(&mut analysis, dictionaries);
    analysis.tokens
}

#[test]
fn the_extracted_lookup_service_matches_the_code_it_replaced() {
    let scratch = common::Scratch::new("lookup-identity", "answers");

    let archive = fixture(&scratch);
    let dictionary = Dictionary::import(&archive, &scratch.join("out")).expect("import");
    let dictionaries = [dictionary];
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");

    // Counted so an empty comparison cannot pass for a clean one — the failure this whole file
    // exists to make impossible is "nothing differed because nothing ran".
    let mut compared = 0;
    let mut answered = 0;

    for text in CORPUS {
        let tokens = tokens_of(text, &analyzer, &dictionaries);
        assert!(!tokens.is_empty(), "{text} produced no tokens");

        for (index, token) in tokens.iter().enumerate() {
            let was = frozen_word(&dictionaries, token).expect("frozen lookup");
            let now = lookup::word(token, &dictionaries).expect("extracted lookup");
            assert_eq!(
                rendered(&was),
                rendered(&now),
                "{text} token {index} ({}) differs",
                token.surface
            );

            let was = frozen_in_context(&dictionaries, &tokens, index).expect("frozen contextual");
            let now = lookup::in_context(&tokens, index, &dictionaries).expect("extracted");
            assert_eq!(
                rendered(&was),
                rendered(&now),
                "{text} token {index} ({}) differs in context",
                token.surface
            );

            compared += 1;
            if !now.word.is_empty() {
                answered += 1;
            }
        }
    }

    // 36 tokens and 24 answers on this fixture, measured rather than guessed. The floors are set
    // below those so ordinary drift in UniDic's segmentation does not fail the run, and far
    // enough above zero that a corpus which stopped producing tokens — or a dictionary that
    // stopped answering — cannot pass as a clean comparison.
    assert!(compared >= 30, "only {compared} tokens compared");
    assert!(
        answered >= 15,
        "only {answered} of {compared} tokens had an answer at all; a corpus the dictionary \
         cannot serve compares two implementations on the empty case"
    );
}

/// The cases the corpus is *for*, asserted individually so a fixture that stops reaching one is a
/// failure here rather than a silent narrowing of the comparison above.
#[test]
fn the_corpus_actually_reaches_every_kind_of_answer() {
    let scratch = common::Scratch::new("lookup-identity", "kinds");

    let archive = fixture(&scratch);
    let dictionary = Dictionary::import(&archive, &scratch.join("out")).expect("import");
    let dictionaries = [dictionary];
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");

    let mut ordinary = false;
    let mut construction = false;
    let mut likely_name = false;
    let mut contested = false;

    for text in CORPUS {
        for token in tokens_of(text, &analyzer, &dictionaries) {
            let view = lookup::word(&token, &dictionaries).expect("lookup");
            if view.inline_source.as_deref() == Some("core") {
                construction = true;
                // The pack answers, and the dictionary's wrong entries do not appear beside it.
                assert!(
                    view.also_in.is_empty(),
                    "な+ん must not disclose 何 or だ: {}",
                    rendered(&view)
                );
            } else if !view.inline.is_empty() {
                ordinary = true;
            }
            if view.likely_name {
                likely_name = true;
            }
            if token.reading_ambiguous {
                contested = true;
            }
        }
    }

    assert!(ordinary, "no ordinary dictionary answer in the corpus");
    assert!(construction, "the explanatory なん never grouped");
    assert!(likely_name, "no unknown katakana word reached the guess");
    assert!(
        contested,
        "no contested reading — 者 and 市場 are both in the fixture"
    );
}
