//! How a name's reading is chosen, against a dictionary shaped the way JMnedict actually is.
//!
//! # The gap this fills
//!
//! Every unit test in `dict::names` drives `merge_names` through a hand-written closure that
//! returns a `FoundName` directly. That tests the joining policy and **cannot see a selection bug**,
//! because selection happens inside `look_up_name`, which those tests never call.
//!
//! Nor did any committed fixture reach it. The three term banks in `pipeline_identity.rs`,
//! `lookup_identity.rs` and `study_identity.rs` all file 善福寺川 with a **populated reading field**,
//! which is the shape an ordinary dictionary uses. JMnedict-in-Yomitan does not: it leaves field 1
//! empty — the importer then backfills it from the term — and states the kana **in the glossary**,
//! several to an entry and several entries to a spelling, all at score 0.
//!
//! So the branch that reads those glosses, and the tie that its order creates, were exercised by
//! nothing. This file is that dictionary.
//!
//! # What went wrong there
//!
//! `Dictionary::lookup` sorts hits by score descending. JMnedict name entries all score 0, so the
//! sort is a no-op and the hits arrive **in term-bank order**. `look_up_name` took the first one
//! that qualified. For 百鬼 that is an entry tagged `fem` reading おきそ — and four readings later
//! the same dictionary offers ひゃくき, which is what the analyzer had already read those two
//! characters as.
//!
//! A reader saw おきそ over 百鬼 because of where a line sat in a term bank.

#[path = "common/mod.rs"]
mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::dict::{look_up_name, merge_names};
use sakatsu_core::model::Token;
use serde_json::json;

/// A name entry shaped the way JMnedict-in-Yomitan writes one: **no reading field**, the kana in
/// the glossary, and a category in the definition tags.
fn name_entry(term: &str, category: &str, readings: &[&str], sequence: i64) -> serde_json::Value {
    json!([term, "", category, "", 0, readings, sequence, ""])
}

/// The dictionary, with the tied entries in a chosen order so the order can be varied.
fn fixture(dir: &Path, hyakki_first: bool) -> PathBuf {
    let path = dir.join("names.zip");
    let file = std::fs::File::create(&path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);

    // Three entries, one spelling, nine readings, every one at score 0 — the real shape.
    let fem = name_entry("百鬼", "fem", &["おきそ"], 1);
    let surname = name_entry(
        "百鬼",
        "surname",
        &["おきそ", "きなり", "なきり", "ひゃくき", "もぎ", "ももき"],
        2,
    );
    let unclass = name_entry("百鬼", "unclass", &["なりき", "ひゃっき"], 3);
    let hyakki = [fem, surname, unclass];
    let mut terms: Vec<serde_json::Value> = if hyakki_first {
        hyakki.to_vec()
    } else {
        // The same three, reversed. Nothing about the *evidence* has changed.
        hyakki.iter().rev().cloned().collect()
    };
    // A name with exactly one reading, as the control: nothing to choose between.
    terms.push(name_entry("善福寺川", "place", &["ぜんぷくじがわ"], 4));
    // An ordinary word that is also spelled like a name, which must never be turned into one.
    terms.push(json!(["白子", "しらす", "n", "", 30, ["whitebait"], 5, ""]));

    let members = [
        (
            "index.json",
            json!({ "title": "Name Evidence Fixture", "format": 3 }).to_string(),
        ),
        (
            "term_bank_1.json",
            serde_json::Value::Array(terms).to_string(),
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

fn open(scratch: &Path, hyakki_first: bool) -> Dictionary {
    let archive = fixture(scratch, hyakki_first);
    let out = scratch.join(if hyakki_first { "a" } else { "b" });
    Dictionary::import(&archive, &out).expect("import")
}

fn piece(surface: &str, reading: &str, at: usize) -> Token {
    Token {
        surface: surface.to_string(),
        reading: reading.to_string(),
        start: Some(at),
        end: Some(at + surface.len()),
        ..Default::default()
    }
}

/// **Every reading the dictionary states, not the first one it happens to state.**
#[test]
fn a_name_lookup_reports_all_of_its_evidence() {
    let scratch = common::Scratch::new("name-evidence", "hermetic");
    let dictionary = open(&scratch.0, true);

    let found = look_up_name([&dictionary], "百鬼").expect("百鬼 is a name here");
    let readings: Vec<&str> = found
        .candidates
        .iter()
        .map(|candidate| candidate.reading.as_str())
        .collect();
    assert_eq!(
        readings,
        vec![
            "おきそ",
            "きなり",
            "なきり",
            "ひゃくき",
            "もぎ",
            "ももき",
            "なりき",
            "ひゃっき"
        ],
        "all nine glosses across three entries, deduplicated and in dictionary order"
    );
    // The category travels with the reading it belongs to, not with the name as a whole.
    let hyakuki = found
        .candidates
        .iter()
        .find(|candidate| candidate.reading == "ひゃくき")
        .expect("ひゃくき is among them");
    assert_eq!(hyakuki.tags, vec!["surname".to_string()]);
}

/// **The reading the analyzer already read the characters as decides it.**
///
/// This is the whole repair. 百 and 鬼 arrive from the segmenter reading ひゃく and き; ひゃくき is
/// one of the dictionary's own readings; two independent sources saying the same thing is evidence
/// where term-bank order is not.
#[test]
fn the_reading_that_agrees_with_the_analyzer_is_the_one_chosen() {
    let scratch = common::Scratch::new("name-evidence", "hermetic");
    let dictionary = open(&scratch.0, true);

    let run = vec![piece("百", "ひゃく", 0), piece("鬼", "き", 3)];
    let joined = merge_names(&run, |surface| look_up_name([&dictionary], surface));

    assert_eq!(joined.len(), 1, "the run is joined");
    assert_eq!(joined[0].surface, "百鬼");
    assert_eq!(
        joined[0].reading, "ひゃくき",
        "not おきそ, which is merely first"
    );
    assert!(
        !joined[0].reading_ambiguous,
        "the agreement settled it, so there is nothing to caveat"
    );
}

/// **Selection must not depend on where a line sits in a term bank.**
///
/// The same three entries, reversed. Nothing about the evidence changed, so nothing about the
/// answer may either. Before the repair this test would have returned なりき for one order and
/// おきそ for the other.
#[test]
fn reordering_the_tied_entries_does_not_change_the_answer() {
    let scratch = common::Scratch::new("name-evidence", "hermetic");
    let run = vec![piece("百", "ひゃく", 0), piece("鬼", "き", 3)];

    let forward = open(&scratch.0, true);
    let reversed = open(&scratch.0, false);

    let one = merge_names(&run, |surface| look_up_name([&forward], surface));
    let other = merge_names(&run, |surface| look_up_name([&reversed], surface));

    assert_eq!(one[0].reading, "ひゃくき");
    assert_eq!(
        one[0].reading, other[0].reading,
        "the term bank's order is not evidence and must not decide this"
    );
}

/// Where nothing agrees, the choice is **marked** rather than made silently.
///
/// The analyzer reads these two characters some other way, so no candidate is confirmed. A name
/// still needs a reading, so the first is shown — but the token says it is one of several, and the
/// others are carried where a reader can be shown them.
#[test]
fn an_unsupported_choice_is_marked_and_keeps_its_alternatives() {
    let scratch = common::Scratch::new("name-evidence", "hermetic");
    let dictionary = open(&scratch.0, true);

    // Readings that match none of the dictionary's candidates.
    let run = vec![piece("百", "もも", 0), piece("鬼", "おに", 3)];
    let joined = merge_names(&run, |surface| look_up_name([&dictionary], surface));

    assert_eq!(joined.len(), 1);
    assert!(
        joined[0].reading_ambiguous,
        "eight readings and nothing to choose between them is exactly an ambiguous reading"
    );
    let offered: Vec<&str> = joined[0]
        .alternatives
        .iter()
        .map(|alternative| alternative.reading.as_str())
        .collect();
    assert!(
        offered.contains(&"ひゃくき") && offered.contains(&"ひゃっき"),
        "the readings not chosen are kept, not discarded: {offered:?}"
    );
    assert!(
        joined[0]
            .alternatives
            .iter()
            .any(|alternative| alternative.form.contains("surname")),
        "each alternative carries the category it was offered under"
    );
}

/// A name with one reading is unchanged, and carries no caveat and no alternatives.
#[test]
fn a_single_reading_name_is_untouched() {
    let scratch = common::Scratch::new("name-evidence", "hermetic");
    let dictionary = open(&scratch.0, true);

    let run = vec![
        piece("善福", "ぜんぷく", 0),
        piece("寺", "じ", 6),
        piece("川", "かわ", 9),
    ];
    let joined = merge_names(&run, |surface| look_up_name([&dictionary], surface));

    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].reading, "ぜんぷくじがわ");
    assert!(!joined[0].reading_ambiguous);
    assert!(joined[0].alternatives.is_empty());
}

/// **An ordinary word spelled like a name is still a word.**
///
/// The guard that aborts the whole lookup on an exactly-spelled non-name hit has to survive all of
/// this, or the evidence-gathering above would turn 白子 into a place.
#[test]
fn a_word_that_is_also_spelled_like_a_name_is_not_made_one() {
    let scratch = common::Scratch::new("name-evidence", "hermetic");
    let dictionary = open(&scratch.0, true);

    assert!(
        look_up_name([&dictionary], "白子").is_none(),
        "白子 is whitebait here, and a word beats a name at the same spelling"
    );
}
