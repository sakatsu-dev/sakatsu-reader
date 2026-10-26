//! The displayed reading, after the same ordered dictionary passes as commands::study.
//! Keep this separate from the raw heteronym gate: joining a word changes the denominator.
mod common;
#[path = "common/heteronyms.rs"]
mod heteronyms;

use sakatsu_core::{
    analyzer::{Analyzer, LinderaAnalyzer},
    dict::{readings, Dictionary},
    kana::to_hiragana,
    model::{Analysis, Token},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf};

/// Shared with the application, not transcribed from it.
///
/// **This function used to be the counter-example.** It reimplemented desktop discovery from a
/// comment, filtered on a root-level `meta.json` — the pre-generation layout, so it omitted every
/// dictionary published as a generation — and finished with `.ok()`, so a dictionary that failed
/// to open became a dictionary that was not there. Either way the gate scored a smaller corpus and
/// reported a rate for it.
fn installed_dictionaries() -> Vec<Dictionary> {
    common::installed_dictionaries()
}

/// The joins **and** the reading pass: what the reader is shown.
///
/// The production pipeline, called rather than restated. `common::through_pipeline` remains
/// joins-only on purpose, for the segmentation harnesses whose denominator joining would change.
fn displayed(mut analysis: Analysis, dictionaries: &[Dictionary]) -> Analysis {
    sakatsu_core::pipeline::Pipeline::new(dictionaries).display(&mut analysis);
    analysis
}

fn candidate_definitions(term: &str, dictionaries: &[Dictionary]) -> Vec<Value> {
    let mut choices: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (reading, _) in readings::candidates(term, dictionaries) {
        choices.entry(to_hiragana(&reading)).or_default();
    }
    for dictionary in dictionaries {
        for hit in dictionary.lookup(term).expect("dictionary lookup") {
            if hit.entry.term != term {
                continue;
            }
            if let Some(glosses) = choices.get_mut(&to_hiragana(&hit.entry.reading)) {
                for gloss in hit
                    .entry
                    .definitions
                    .iter()
                    .flat_map(|sense| &sense.glosses)
                {
                    if !glosses.contains(gloss) {
                        glosses.push(gloss.clone());
                    }
                }
            }
        }
    }
    choices
        .into_iter()
        .map(|(reading, glosses)| json!({"reading":reading,"glosses":glosses}))
        .collect()
}

fn token_at(analysis: &Analysis, start: usize, end: usize) -> Option<&Token> {
    analysis
        .tokens
        .iter()
        .find(|token| token.start == Some(start) && token.end == Some(end))
}

fn export_case(
    id: String,
    text: &str,
    surface: &str,
    start: usize,
    accepted: &[String],
    analysis: &Analysis,
    dictionaries: &[Dictionary],
) -> Value {
    let end = start + surface.len();
    assert_eq!(
        text.get(start..end),
        Some(surface),
        "target must name exact UTF-8 bytes"
    );
    let token = token_at(analysis, start, end);
    let candidates = candidate_definitions(surface, dictionaries);
    json!({"id":id,"text":text,"surface":surface,"start":start,"end":end,
        "accepted":accepted,"baseline":token.map(|t| to_hiragana(&t.reading)),
        "eligible": token.is_some_and(|t| t.reading_ambiguous && t.conjugation.is_none() && t.surface==t.dict_form),
        "candidates":candidates})
}

fn scratch() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scratch/context-next")
}

#[test]
#[ignore = "needs installed dictionaries; exports local experiment inputs"]
fn displayed_heteronym_baseline() {
    let dictionaries = installed_dictionaries();
    assert!(
        !dictionaries.is_empty(),
        "install dictionaries before this explicit measurement"
    );
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
    let (mut raw_aligned, mut raw_right, mut full_aligned, mut full_right, mut fixed, mut broken) =
        (0, 0, 0, 0, 0, 0);
    let (mut lost, mut gained, mut comparable, mut eligible, mut eligible_right) = (0, 0, 0, 0, 0);
    let mut rows = Vec::new();
    for (id, case) in heteronyms::HETERONYMS.iter().enumerate() {
        let raw = analyzer.analyze(case.text).expect("analyze fixture");
        let full = displayed(raw.clone(), &dictionaries);
        // Preserve the original fixture's first matching token when it exists. Otherwise retain
        // a literal target span so newly joined words can be reported as gained, not lost data.
        let start = raw
            .tokens
            .iter()
            .find(|t| t.surface == case.term)
            .and_then(|t| t.start)
            .unwrap_or_else(|| case.text.find(case.term).expect("fixture contains term"));
        let end = start + case.term.len();
        let before = token_at(&raw, start, end).filter(|t| !t.reading.is_empty());
        let after = token_at(&full, start, end).filter(|t| !t.reading.is_empty());
        let gold = to_hiragana(case.reading);
        let was = before.is_some_and(|t| to_hiragana(&t.reading) == gold);
        let now = after.is_some_and(|t| to_hiragana(&t.reading) == gold);
        raw_aligned += usize::from(before.is_some());
        raw_right += usize::from(was);
        full_aligned += usize::from(after.is_some());
        full_right += usize::from(now);
        lost += usize::from(before.is_some() && after.is_none());
        gained += usize::from(before.is_none() && after.is_some());
        if before.is_some() && after.is_some() {
            comparable += 1;
            fixed += usize::from(!was && now);
            broken += usize::from(was && !now);
        }
        let row = export_case(
            format!("heteronym-{id:03}"),
            case.text,
            case.term,
            start,
            &[gold],
            &full,
            &dictionaries,
        );
        if row["eligible"] == true {
            eligible += 1;
            eligible_right += usize::from(now);
        }
        rows.push(row);
    }
    println!("raw {raw_right}/{raw_aligned}; displayed {full_right}/{full_aligned}; lost {lost}, gained {gained}; shared {comparable}: fixed {fixed}, broken {broken}; eligible {eligible_right}/{eligible}");
    // This is the existing raw gate, not a newly invented floor on the displayed population.
    assert!(raw_aligned >= 150 && raw_right as f64 / raw_aligned as f64 >= 0.66);
    let report = json!({"schema":1,"pipeline":"analyzer -> compounds -> names -> correct",
        "dictionaries":dictionaries.iter().map(|d| d.meta()).collect::<Vec<_>>(),
        "raw":{"right":raw_right,"aligned":raw_aligned},"displayed":{"right":full_right,"aligned":full_aligned},
        "shared":comparable,"fixed":fixed,"broken":broken,"lost":lost,"gained":gained,"cases":rows});
    std::fs::create_dir_all(scratch()).unwrap();
    std::fs::write(
        scratch().join("reading-pipeline.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "needs installed dictionaries; exercises the actual join/correct order"]
fn a_joined_word_is_corrected_after_joining() {
    let dictionaries = installed_dictionaries();
    assert!(!dictionaries.is_empty());
    // Synthetic pre-join analyzer output: neither part is wrong on its own; the compound needs
    // rendaku and must subsequently receive the same dictionary checks as a native whole token.
    let analysis = Analysis {
        execution: None,
        text: "受付係".into(),
        tokens: vec![
            Token {
                surface: "受付".into(),
                reading: "うけつけ".into(),
                start: Some(0),
                end: Some(6),
                ..Default::default()
            },
            Token {
                surface: "係".into(),
                reading: "かかり".into(),
                start: Some(6),
                end: Some(9),
                ..Default::default()
            },
        ],
    };
    let result = displayed(analysis, &dictionaries);
    assert_eq!(result.tokens.len(), 1);
    assert_eq!(result.tokens[0].reading, "うけつけがかり");
    assert_eq!(result.tokens[0].surface, "受付係");
    assert_eq!(result.tokens[0].components.len(), 2);
}

/// Private, source-reviewed capture labels. Pending proposals never contribute to accuracy.
#[test]
#[ignore = "needs local source-verified capture readings and installed dictionaries"]
fn source_verified_capture_readings() {
    use sha2::{Digest, Sha256};
    let path = common::corpus_dir().join("reading-gold.json");
    let body = std::fs::read(&path).expect("prepare source-reviewed capture labels first");
    let gold: Value = serde_json::from_slice(&body).expect("valid capture label JSON");
    let dictionaries = installed_dictionaries();
    assert!(!dictionaries.is_empty());
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
    let mut rows = Vec::new();
    let (mut aligned, mut right, mut missing_candidate, mut eligible) = (0, 0, 0, 0);
    for case in gold["cases"].as_array().expect("capture cases") {
        if case["status"] != "source_verified" {
            continue;
        }
        assert_eq!(
            case["evidence"], "printed_ruby",
            "other evidence needs explicit review support"
        );
        let file = case["source_image"].as_str().unwrap();
        assert_eq!(
            std::path::Path::new(file).components().count(),
            1,
            "source must be a local filename"
        );
        let bytes = std::fs::read(common::corpus_dir().join(file)).expect("source image exists");
        assert_eq!(
            sakatsu_core::hashing::hex(Sha256::digest(bytes)),
            case["source_sha256"].as_str().unwrap(),
            "image changed since review"
        );
        let text = case["text"].as_str().unwrap();
        let surface = case["surface"].as_str().unwrap();
        let start = case["start"].as_u64().unwrap() as usize;
        assert_eq!(
            case["end"].as_u64().unwrap() as usize,
            start + surface.len()
        );
        let accepted: Vec<String> = serde_json::from_value(case["accepted"].clone()).unwrap();
        assert!(!accepted.is_empty());
        let analysis = displayed(analyzer.analyze(text).unwrap(), &dictionaries);
        let mut row = export_case(
            case["id"].as_str().unwrap().into(),
            text,
            surface,
            start,
            &accepted,
            &analysis,
            &dictionaries,
        );
        row["reading_kind"] = case["reading_kind"].clone();
        row["group"] = case["group"].clone();
        row["split"] = case["split"].clone();
        if let Some(baseline) = row["baseline"].as_str() {
            aligned += 1;
            right += usize::from(accepted.iter().any(|r| r == baseline));
        }
        let present = row["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| accepted.iter().any(|r| c["reading"] == *r));
        missing_candidate += usize::from(!present);
        eligible += usize::from(row["eligible"] == true);
        println!(
            "capture {}: kind {}, aligned {}, eligible {}, gold candidate {}",
            case["id"],
            case["reading_kind"],
            !row["baseline"].is_null(),
            row["eligible"],
            present
        );
        rows.push(row);
    }
    assert!(!rows.is_empty(), "no verified cases were evaluated");
    let output = json!({"schema":1,"gold_sha256":sakatsu_core::hashing::hex(Sha256::digest(body)),
        "cases":rows,"right":right,"aligned":aligned,"eligible":eligible,
        "gold_candidate_missing":missing_candidate,"pending_sources":gold["pending_sources"].as_array().map(Vec::len)});
    std::fs::create_dir_all(scratch()).unwrap();
    std::fs::write(
        scratch().join("capture-pipeline.json"),
        serde_json::to_vec_pretty(&output).unwrap(),
    )
    .unwrap();
    println!("capture source gold: {} cases, {right}/{aligned} aligned correct, eligible {eligible}, gold candidate missing {missing_candidate}; pending sources are unscored",rows.len());
}
