//! The pipeline extraction changed nothing.
//!
//! # Why a frozen copy of the old code lives here
//!
//! `pipeline::Pipeline` was extracted from three places that each held their own copy of the same
//! ordered passes: `commands::study`, `common::through_pipeline`, and a closure in
//! `performance.rs`. An extraction like that is only worth doing if it is provably
//! behaviour-preserving, and "I read both and they look the same" is exactly the standard this
//! branch has been burned by.
//!
//! So [`frozen`] is a **verbatim** copy of the pre-extraction sequence, taken from
//! `src-tauri/src/commands.rs` before it was touched. Both implementations run over the same
//! inputs and the whole `Analysis` is compared field by field — not a spot check on surfaces,
//! because the fields most likely to diverge (`reading_ambiguous`, `components`, `lemma_reading`)
//! are exactly the ones a surface comparison cannot see.
//!
//! **This module is deliberately temporary.** Once the migration has been verified and the three
//! duplicates are gone, `frozen` goes too — keeping it would resurrect the duplication the
//! extraction removed.
//!
//! # Two corpora
//!
//! The hermetic case builds its own dictionary, so it gates an ordinary `cargo test` run with
//! nothing installed — the pattern `heteronym_readings.rs` set. The installed case is broader and
//! is ignored by default, because it cannot be.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Analysis;
use sakatsu_core::pipeline::Pipeline;
use sakatsu_core::store::WordKey;
use serde_json::json;

/// The pre-extraction sequence, copied verbatim and not to be tidied.
///
/// Reformatting this would defeat its only purpose. It is `with_names` followed by
/// `correct_readings` from `commands.rs`, with the two `state.0.now()` calls replaced by the one
/// slice a test can hold — which is the only intended difference, and the one the plan's section
/// on a single resource snapshot exists to make.
fn frozen(mut analysis: Analysis, dictionaries: &[Dictionary]) -> Analysis {
    if dictionaries.is_empty() {
        return analysis;
    }
    analysis.tokens = sakatsu_core::dict::merge_compounds(&analysis.tokens, |surface| {
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
    });
    analysis.tokens = sakatsu_core::dict::merge_names(&analysis.tokens, |surface| {
        sakatsu_core::dict::look_up_name(dictionaries, surface)
    });
    sakatsu_core::dict::readings::correct(&mut analysis, dictionaries);
    analysis
}

/// Every field of every token, plus the study key each one resolves to.
///
/// Serialising the whole `Analysis` rather than naming fields means a field added later is
/// compared without anyone remembering to add it here.
fn rendered(analysis: &Analysis) -> String {
    let keys: Vec<WordKey> = analysis.tokens.iter().map(WordKey::for_token).collect();
    serde_json::to_string_pretty(&json!({ "analysis": analysis, "keys": keys }))
        .expect("an analysis serialises")
}

/// Passages chosen to make every stage fire: a compound whose whole reads differently from its
/// parts, a place name the analyzer shatters, and a contested reading.
const CORPUS: &[&str] = &[
    "受付係に聞いてください。",
    "善福寺川の近くに住んでいます。",
    "その者は何も言わなかった。",
    "彼は毎日図書館で勉強している。",
    "ここって禁足地なんじゃなかったっけ。",
    "父は昨日新しい車を買いました。",
];

fn compare(dictionaries: &[Dictionary], label: &str) -> usize {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
    let pipeline = Pipeline::new(dictionaries);
    let mut compared = 0;
    for text in CORPUS {
        let raw = analyzer.analyze(text).expect("analyzes");

        let was = frozen(raw.clone(), dictionaries);

        let mut now = raw.clone();
        pipeline.display(&mut now);

        assert_eq!(
            rendered(&was),
            rendered(&now),
            "{label}: the extraction changed the analysis of {text:?}"
        );
        compared += now.tokens.len();
    }
    compared
}

/// A dictionary written for this test, so the check runs with nothing installed.
fn fixture(dir: &Path) -> PathBuf {
    let path = dir.join("fixture.zip");
    let file = std::fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let members = [
        (
            "index.json",
            json!({ "title": "Pipeline Fixture", "format": 3 }).to_string(),
        ),
        (
            "term_bank_1.json",
            json!([
                // The compound whose whole is not the sum of its parts: rendaku the analyzer
                // cannot know about. This is what fires `merge_compounds`.
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
                // A place name, tagged so `look_up_name` will join it. This is what fires
                // `merge_names`.
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
                // Two readings on record, so the correction pass must leave it alone and say so.
                ["者", "もの", "n", "", 20, ["person"], 5, ""],
                ["者", "しゃ", "n", "", 20, ["-er; -ist"], 6, ""],
                // One reading on record, so the correction pass may confirm or replace.
                ["図書館", "としょかん", "n", "", 40, ["library"], 7, ""],
                ["勉強", "べんきょう", "n vs", "", 40, ["study"], 8, ""]
            ])
            .to_string(),
        ),
    ];
    for (name, contents) in members {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .expect("member");
        writer.write_all(contents.as_bytes()).expect("write");
    }
    writer.finish().expect("finish");
    path
}

/// The extraction is behaviour-preserving, with nothing installed.
#[test]
fn the_extracted_pipeline_matches_the_code_it_replaced() {
    let scratch = common::Scratch::new("pipeline-identity", "hermetic");
    let archive = fixture(&scratch);
    let dictionary = Dictionary::import(&archive, &scratch.join("out")).expect("import");
    let dictionaries = [dictionary];

    // Teeth: a comparison over a corpus where no pass fires would pass no matter what changed.
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
    let pipeline = Pipeline::new(&dictionaries);
    let mut joined = analyzer.analyze(CORPUS[0]).expect("analyzes");
    let before = joined.tokens.len();
    pipeline.display(&mut joined);
    assert!(
        joined.tokens.len() < before,
        "the fixture must actually join 受付 and 係, or this test compares two no-ops"
    );
    assert!(
        joined
            .tokens
            .iter()
            .any(|token| token.reading == "うけつけがかり"),
        "and the joined token must carry the compound reading"
    );
    let mut contested = analyzer.analyze(CORPUS[2]).expect("analyzes");
    let report = pipeline.display(&mut contested);
    assert!(
        report.readings.corrected + report.readings.confirmed + report.readings.ambiguous > 0,
        "and the reading pass must reach a verdict on something"
    );

    let compared = compare(&dictionaries, "hermetic");
    assert!(
        compared > 30,
        "the corpus must produce tokens to compare, got {compared}"
    );
    drop(dictionaries);
}

/// The same check over the dictionaries actually installed, which are far larger.
#[test]
#[ignore = "needs installed dictionaries"]
fn the_extracted_pipeline_matches_over_installed_dictionaries() {
    let dictionaries = common::installed_dictionaries();
    assert!(
        !dictionaries.is_empty(),
        "install dictionaries before this explicit check"
    );
    let compared = compare(&dictionaries, "installed");
    println!(
        "compared {} passages over {} dictionaries, {compared} tokens",
        CORPUS.len(),
        dictionaries.len()
    );
}
