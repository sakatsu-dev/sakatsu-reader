//! A cross-reference is followed to the word it points at, in the dictionary that issued it.
//!
//! # What went wrong
//!
//! Jitendex files a search-only spelling as its own entry, under the **negated** sequence of the
//! word it belongs to, and gives it cross-reference text instead of senses — `⟶ 抜 ぬ き 取 と る`,
//! then `抜き取る redirected from 抜きとる`. That is how a reader *finds* 抜きとる, and it is not
//! how a reader *reads* it.
//!
//! Seven tokens in one reading notebook displayed nothing else. `merge` promotes a dictionary's
//! answer into `inline` when the analyzer has none of its own, and the only answer available was
//! the pointer. The other analyzer escaped it by supplying its own senses rather than by resolving
//! anything: for 差しこむ it shows sequence 10072172, which is not the target of `-1291170` at all.
//! So this is not one engine imitating the other; it is a promotion that should never have
//! promoted a pointer.
//!
//! # What these tests fix in place
//!
//! The resolution is **dictionary-local** — the target comes from the same dictionary that issued
//! the pointer, so a sense never arrives under another dictionary's name — and it changes only the
//! definitions. `SourceResult::retrieved` is untouched, because following a cross-reference says
//! nothing about whether the answer was found for the whole token or for a component. The hit
//! keeps its own `term` and `reading`, because the reader selected 抜きとる and that is still the
//! word on the page.
//!
//! The four ways it must decline are each checked, because a rule that fired more eagerly would
//! rewrite ordinary entries.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Token;
use serde_json::json;

/// A Yomitan term row: `[term, reading, tags, rules, score, glosses, sequence, ""]`.
fn row(term: &str, reading: &str, glosses: &[&str], sequence: i64) -> serde_json::Value {
    json!([term, reading, "n", "", 0, glosses, sequence, ""])
}

fn fixture(scratch: &Path, name: &str, terms: Vec<serde_json::Value>) -> PathBuf {
    let path = scratch.join(format!("{name}.zip"));
    let file = std::fs::File::create(&path).expect("fixture");
    let mut writer = zip::ZipWriter::new(file);
    for (member, contents) in [
        (
            "index.json",
            json!({ "title": name, "format": 3 }).to_string(),
        ),
        (
            "term_bank_1.json",
            serde_json::Value::Array(terms).to_string(),
        ),
    ] {
        writer
            .start_file(member, zip::write::SimpleFileOptions::default())
            .expect("member");
        writer.write_all(contents.as_bytes()).expect("write");
    }
    writer.finish().expect("finish");
    path
}

fn open(scratch: &Path, name: &str, terms: Vec<serde_json::Value>) -> Dictionary {
    let archive = fixture(scratch, name, terms);
    Dictionary::import(&archive, &scratch.join(name)).expect("import")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sakatsu-xref-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn token(surface: &str) -> Token {
    Token {
        surface: surface.to_string(),
        dict_form: surface.to_string(),
        start: Some(0),
        end: Some(surface.len()),
        ..Default::default()
    }
}

/// What the reader is shown for the selected word, and which dictionary it is credited to.
fn shown(dictionaries: &[Dictionary], surface: &str) -> (Vec<String>, Option<String>, String) {
    let tokens = [token(surface)];
    let looked = sakatsu_core::lookup::with_evidence(&tokens, 0, dictionaries).expect("lookup");
    let glosses = looked
        .view
        .word
        .inline
        .iter()
        .flat_map(|sense| sense.glosses.clone())
        .collect();
    let scope = format!("{:?}", looked.view.word.inline_retrieved);
    (glosses, looked.view.word.inline_source.clone(), scope)
}

/// **The defect, and the fix.** A pointer becomes the meaning it points at.
#[test]
fn a_cross_reference_is_answered_with_its_targets_senses() {
    let dir = scratch("resolves");
    let dictionary = open(
        &dir,
        "Xref",
        vec![
            // The search-only spelling, under the negated sequence, holding only a pointer.
            row(
                "抜きとる",
                "ぬきとる",
                &["⟶ 抜 ぬ き 取 と る", "抜き取る redirected from 抜きとる"],
                -1478150,
            ),
            // The word it belongs to.
            row(
                "抜き取る",
                "ぬきとる",
                &["to pull out; to extract", "to pick out"],
                1478150,
            ),
        ],
    );
    let dictionaries = [dictionary];

    let (glosses, source, scope) = shown(&dictionaries, "抜きとる");
    assert!(
        glosses.iter().any(|gloss| gloss.contains("to pull out")),
        "the reader gets the meaning, not the pointer: {glosses:?}"
    );
    assert!(
        !glosses
            .iter()
            .any(|gloss| gloss.contains("redirected from") || gloss.contains('\u{27f6}')),
        "no cross-reference text survives into the senses: {glosses:?}"
    );
    // Attribution and scope are the issuing dictionary's and the whole token's, unchanged.
    assert_eq!(source.as_deref(), Some("Xref"));
    assert!(
        scope.contains("Whole"),
        "retrieval scope is preserved: {scope}"
    );
}

/// The selected word keeps its own identity. 抜きとる is what was selected and what is headed.
#[test]
fn resolution_does_not_rename_the_selected_word() {
    let dir = scratch("identity");
    let dictionaries = [open(
        &dir,
        "Xref",
        vec![
            row("抜きとる", "ぬきとる", &["⟶ 抜 ぬ き 取 と る"], -1478150),
            row("抜き取る", "ぬきとる", &["to pull out"], 1478150),
        ],
    )];
    let tokens = [token("抜きとる")];
    let looked = sakatsu_core::lookup::with_evidence(&tokens, 0, &dictionaries).expect("lookup");
    assert_eq!(
        looked.view.word.headword, "抜きとる",
        "the reader selected this spelling and it stays the headword"
    );
    assert_eq!(
        looked.evidence[0].hits[0].entry.term, "抜きとる",
        "the hit still records the spelling that matched"
    );
}

/// **An ordinary entry is not touched.** This is the control the rule most needs.
#[test]
fn an_ordinary_entry_is_left_alone() {
    let dir = scratch("ordinary");
    let dictionaries = [open(
        &dir,
        "Xref",
        vec![
            row("水", "みず", &["water"], 1000010),
            // A second spelling of the same word, so the sequence bucket is populated and the
            // rule has something it *could* have substituted had it fired.
            row("みず", "みず", &["water (kana)"], 1000010),
        ],
    )];
    let (glosses, _, _) = shown(&dictionaries, "水");
    assert_eq!(
        glosses,
        vec!["water".to_string()],
        "an entry with real senses keeps exactly them"
    );
}

/// A pointer whose target is absent stays a pointer rather than becoming an empty answer.
#[test]
fn a_cross_reference_with_no_target_is_left_as_it_stands() {
    let dir = scratch("missing");
    let dictionaries = [open(
        &dir,
        "Xref",
        // The target is simply not in this dictionary.
        vec![row(
            "抜きとる",
            "ぬきとる",
            &["⟶ 抜 ぬ き 取 と る"],
            -1478150,
        )],
    )];
    let (glosses, source, _) = shown(&dictionaries, "抜きとる");
    assert_eq!(
        glosses,
        vec!["⟶ 抜 ぬ き 取 と る".to_string()],
        "what the dictionary said is still what is shown"
    );
    assert_eq!(source.as_deref(), Some("Xref"));
}

/// A target that is itself only a pointer is not followed into a second pointer.
#[test]
fn a_target_that_is_also_a_pointer_is_not_substituted() {
    let dir = scratch("chain");
    let dictionaries = [open(
        &dir,
        "Xref",
        vec![
            row("あ", "あ", &["⟶ い"], -2000001),
            row("い", "い", &["う redirected from い"], 2000001),
        ],
    )];
    let (glosses, _, _) = shown(&dictionaries, "あ");
    assert_eq!(
        glosses,
        vec!["⟶ い".to_string()],
        "a pointer is not replaced by another pointer"
    );
}

/// A search-only form carrying one real sense beside its pointer keeps that sense.
///
/// The predicate requires **every** gloss to be cross-reference text. A looser one would discard
/// the only thing such an entry had.
#[test]
fn an_entry_with_a_real_sense_beside_a_pointer_is_not_replaced() {
    let dir = scratch("mixed");
    let dictionaries = [open(
        &dir,
        "Xref",
        vec![
            row("あ", "あ", &["a genuine sense", "⟶ い"], -2000002),
            row("い", "い", &["the target sense"], 2000002),
        ],
    )];
    let (glosses, _, _) = shown(&dictionaries, "あ");
    assert!(
        glosses.iter().any(|gloss| gloss == "a genuine sense"),
        "the real sense survives: {glosses:?}"
    );
    assert!(
        !glosses.iter().any(|gloss| gloss == "the target sense"),
        "and nothing was substituted for it: {glosses:?}"
    );
}

/// A dictionary with no sequence bucket answers as it always did.
///
/// `has_sequence()` is false for an index built before the bucket existed. That is an absence of
/// capability, not a failure, and it must not stop a lookup.
#[test]
fn a_dictionary_without_a_sequence_index_still_answers() {
    let dir = scratch("nobucket");
    let dictionaries = [open(
        &dir,
        "Xref",
        vec![row(
            "抜きとる",
            "ぬきとる",
            &["⟶ 抜 ぬ き 取 と る"],
            -1478150,
        )],
    )];
    assert!(
        dictionaries[0].has_sequence(),
        "the importer builds the bucket, so removing it below tests the absence and not the import"
    );
    // Remove the bucket the importer built, leaving the rest of the index intact. The index is
    // published as a generation, so the bucket sits under one rather than beside the import.
    fn strip_buckets(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.file_name().is_some_and(|name| name == "sequence") {
                std::fs::remove_dir_all(&path).expect("remove the sequence bucket");
            } else {
                strip_buckets(&path);
            }
        }
    }
    strip_buckets(&dir.join("Xref"));
    let reopened = [Dictionary::open(&dir.join("Xref")).expect("reopen")];
    assert!(!reopened[0].has_sequence(), "the bucket is gone");

    let (glosses, _, _) = shown(&reopened, "抜きとる");
    assert_eq!(
        glosses,
        vec!["⟶ 抜 ぬ き 取 と る".to_string()],
        "the lookup still answers, with what it has"
    );
}

/// **Both lookup paths resolve, because both go through `sources`.**
///
/// `with_evidence` is the contextual path a reader's selection takes; `word` is the plain one the
/// desktop also uses. Resolution used to sit in the first only, so the same word answered
/// differently depending on which function asked — and nothing said so.
#[test]
fn the_contextual_and_plain_lookups_resolve_alike() {
    let dir = scratch("bothpaths");
    let dictionaries = [open(
        &dir,
        "Xref",
        vec![
            row("抜きとる", "ぬきとる", &["⟶ 抜 ぬ き 取 と る"], -1478150),
            row("抜き取る", "ぬきとる", &["to pull out"], 1478150),
        ],
    )];
    let tokens = [token("抜きとる")];

    let contextual = sakatsu_core::lookup::with_evidence(&tokens, 0, &dictionaries)
        .expect("contextual lookup")
        .view
        .word
        .inline
        .iter()
        .flat_map(|sense| sense.glosses.clone())
        .collect::<Vec<_>>();
    let plain = sakatsu_core::lookup::word(&tokens[0], &dictionaries)
        .expect("plain lookup")
        .inline
        .iter()
        .flat_map(|sense| sense.glosses.clone())
        .collect::<Vec<_>>();

    assert_eq!(plain, contextual, "the two paths must agree");
    assert!(
        plain.iter().any(|gloss| gloss.contains("to pull out")),
        "and both must have followed the pointer: {plain:?}"
    );
}

/// **A title is not an identity.** Two dictionaries may share one, and resolving by title would
/// answer out of whichever happened to be first.
///
/// Both hold the same search-only spelling under the same sequence, and each points at a different
/// target. Each answer must come from the dictionary that issued it.
#[test]
fn two_dictionaries_sharing_a_title_each_resolve_through_themselves() {
    let dir = scratch("dupetitle");
    // Two installations, side by side, each with its own store.
    std::fs::create_dir_all(dir.join("a")).expect("first store");
    std::fs::create_dir_all(dir.join("b")).expect("second store");
    let first = open(
        &dir.join("a"),
        "Same Title",
        vec![
            row("あ", "あ", &["⟶ い"], -2100001),
            row("い", "い", &["the first dictionary's sense"], 2100001),
        ],
    );
    let second = open(
        &dir.join("b"),
        "Same Title",
        vec![
            row("あ", "あ", &["⟶ い"], -2100001),
            row("い", "い", &["the second dictionary's sense"], 2100001),
        ],
    );
    assert_eq!(
        first.meta().title,
        second.meta().title,
        "the titles collide"
    );

    let dictionaries = [first, second];
    let tokens = [token("あ")];
    let looked = sakatsu_core::lookup::with_evidence(&tokens, 0, &dictionaries).expect("lookup");

    let senses: Vec<Vec<String>> = looked
        .evidence
        .iter()
        .map(|source| {
            source
                .hits
                .iter()
                .flat_map(|hit| {
                    hit.entry
                        .definitions
                        .iter()
                        .flat_map(|definition| definition.glosses.clone())
                })
                .collect()
        })
        .collect();
    assert_eq!(senses.len(), 2, "both dictionaries answered");
    assert!(
        senses[0].iter().any(|g| g.contains("first dictionary")),
        "the first resolved through itself: {senses:?}"
    );
    assert!(
        senses[1].iter().any(|g| g.contains("second dictionary")),
        "and the second through itself: {senses:?}"
    );
}

/// A dictionary that cannot be read is an **error**, not an empty answer.
///
/// The sequence bucket is emptied of its shards while the directory stays, so `has_sequence` is
/// still true and the read is genuinely attempted and genuinely fails.
#[test]
fn a_dictionary_read_failure_is_returned_rather_than_swallowed() {
    let dir = scratch("readfail");
    let dictionaries = [open(
        &dir,
        "Xref",
        vec![
            row("あ", "あ", &["⟶ い"], -2200001),
            row("い", "い", &["the target sense"], 2200001),
        ],
    )];
    assert!(dictionaries[0].has_sequence(), "the bucket exists to break");

    // Corrupt every shard in place: the path still exists, and reading it will not parse.
    fn corrupt(dir: &Path) -> usize {
        let mut broken = 0;
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                broken += corrupt(&path);
            } else {
                std::fs::write(&path, b"not json at all").expect("corrupt the shard");
                broken += 1;
            }
        }
        broken
    }
    fn find_bucket(dir: &Path) -> Option<PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.file_name().is_some_and(|name| name == "sequence") {
                return Some(path);
            }
            if let Some(found) = find_bucket(&path) {
                return Some(found);
            }
        }
        None
    }
    let bucket = find_bucket(&dir.join("Xref")).expect("the sequence bucket");
    assert!(corrupt(&bucket) > 0, "there was a shard to corrupt");

    let reopened = [Dictionary::open(&dir.join("Xref")).expect("reopen")];
    let tokens = [token("あ")];
    let outcome = sakatsu_core::lookup::with_evidence(&tokens, 0, &reopened);
    assert!(
        outcome.is_err(),
        "an unreadable shard must fail the lookup, not read as a word with no target"
    );
}
