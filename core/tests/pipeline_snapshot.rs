//! One dictionary snapshot serves a whole pipeline invocation, joins and readings alike.
//!
//! # The bug this pins
//!
//! The desktop pipeline used to resolve the live dictionary set **twice**: once for the joins and
//! again for the reading correction. Each stage pinned its own pass, and the joins' comment said
//! why — *"Re-reading per name would let an import land mid-passage and rejoin some of it against
//! one set of dictionaries and the rest against another"* — but it was one stage too narrow. An
//! import landing between the two joined a passage against one generation and corrected it against
//! the next.
//!
//! # Why this is deterministic, and not a race
//!
//! Nothing here sleeps or spawns. `Pipeline` borrows a dictionary slice for its whole life, so the
//! seam is reachable from the outside: run the joins, publish a replacement, then run the rest.
//! A pipeline that re-resolved anything would answer the second half from the new generation.
//!
//! This is also where milestone 3's generation leases stop being a storage detail. The lease is
//! what makes the held reader's files survive the publish; the borrowed snapshot is what makes both
//! stages use them. Either alone is not enough.

#[path = "common/scratch.rs"]
mod scratch;

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::analyzer::{Analyzer, LinderaAnalyzer};
use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::pipeline::Pipeline;
use serde_json::json;

/// 受付係 needs the dictionary for its rendaku, and 図書館 for its reading. A replacement that
/// drops both is therefore visible in the joins *and* in the correction — which is the point: one
/// stage reverting would be caught, and so would the other.
fn archive(dir: &Path, name: &str, complete: bool) -> PathBuf {
    let path = dir.join(format!("{name}.zip"));
    let file = std::fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);

    let mut bank = vec![
        json!(["受付", "うけつけ", "n", "", 30, ["reception"], 2, ""]),
        json!(["係", "かかり", "n", "", 30, ["person in charge"], 3, ""]),
    ];
    if complete {
        bank.push(json!([
            "受付係",
            "うけつけがかり",
            "n",
            "",
            20,
            ["receptionist"],
            1,
            ""
        ]));
        bank.push(json!([
            "図書館",
            "としょかん",
            "n",
            "",
            40,
            ["library"],
            4,
            ""
        ]));
    }

    for (member, contents) in [
        (
            "index.json",
            json!({ "title": "Snapshot Fixture", "format": 3 }).to_string(),
        ),
        ("term_bank_1.json", json!(bank).to_string()),
    ] {
        writer
            .start_file(member, zip::write::SimpleFileOptions::default())
            .expect("member");
        writer.write_all(contents.as_bytes()).expect("write");
    }
    writer.finish().expect("finish");
    path
}

const PASSAGE: &str = "受付係は図書館にいます。";

/// A replacement landing between the joins and the reading pass does not split an invocation.
#[test]
fn a_replacement_between_the_stages_cannot_split_one_invocation() {
    let scratch = scratch::Scratch::new("pipeline-snapshot", "replacement");
    let out = scratch.join("out");

    let complete = archive(&scratch, "complete", true);
    let stripped = archive(&scratch, "stripped", false);

    // The snapshot the invocation will be served from, resolved once and held throughout.
    let held = Dictionary::import(&complete, &out).expect("import");
    let dictionaries = [held];
    let pipeline = Pipeline::new(&dictionaries);

    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
    let mut analysis = analyzer.analyze(PASSAGE).expect("analyzes");

    // Stage one: the joins.
    let (joined, joins) = pipeline.segmentation(&analysis.tokens);
    analysis.tokens = joined;
    assert!(
        analysis
            .tokens
            .iter()
            .any(|token| token.surface == "受付係"),
        "the joins must fire, or the seam below proves nothing"
    );
    let _ = joins;

    // An import lands here — exactly between the stages.
    Dictionary::import(&stripped, &out).expect("replacement import");

    // Stage two, through the same pipeline. The correction must answer from the generation the
    // invocation started with, not the one that has just replaced it.
    let report = pipeline.display(&mut analysis);
    let library = analysis
        .tokens
        .iter()
        .find(|token| token.surface == "図書館")
        .expect("図書館 survives the joins");
    assert_eq!(
        library.reading, "としょかん",
        "the reading pass must use the same generation the joins did"
    );
    assert!(
        analysis
            .tokens
            .iter()
            .any(|token| token.surface == "受付係" && token.reading == "うけつけがかり"),
        "and the joined compound must keep the reading its own generation gave it"
    );
    assert!(
        report.readings.corrected + report.readings.confirmed > 0,
        "the correction pass must have reached the dictionary at all"
    );

    // Teeth: a reader opened *after* the replacement genuinely cannot do this, so the assertions
    // above are about the pinned snapshot rather than about the passage being easy.
    let fresh = Dictionary::open(&out).expect("open the replacement");
    let after = [fresh];
    let mut retried = analyzer.analyze(PASSAGE).expect("analyzes");
    Pipeline::new(&after).display(&mut retried);
    assert!(
        !retried.tokens.iter().any(|token| token.surface == "受付係"),
        "the replacement really did drop the compound, so the pinned run was not a coincidence"
    );

    drop(dictionaries);
    drop(after);
}

/// With nothing installed the pipeline is the identity, which is a supported state rather than an
/// error: a fresh install analyses before it has downloaded anything.
#[test]
fn no_dictionaries_is_a_supported_state() {
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic");
    let mut analysis = analyzer.analyze(PASSAGE).expect("analyzes");
    let before = analysis.clone();
    let report = Pipeline::new(&[]).display(&mut analysis);
    assert_eq!(analysis, before, "nothing installed changes nothing");
    assert_eq!(
        report,
        Default::default(),
        "and reports having done nothing"
    );
}
