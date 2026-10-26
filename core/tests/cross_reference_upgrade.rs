//! Does the repair work on the reader's *actual* dictionary, once its index can support it?
//!
//! # Why this is separate from the fixture tests
//!
//! `cross_reference_resolution.rs` proves the rule on dictionaries this test suite builds. That
//! establishes the logic and nothing about the data — and the data is where it turned out to
//! matter: the installed Jitendex has `gloss`, `reading` and `spelling` buckets and **no
//! `sequence` bucket**, so `has_sequence()` is false, resolution declines, and all seven
//! reader-visible cases stayed exactly as they were. The fixture tests passed the whole time.
//!
//! # What this does, and what it refuses to do
//!
//! It runs against an **isolated copy** of the installed index, never the reader's own store.
//! `SAKATSU_XREF_TRIAL` points at the copy; absent it, the test fails rather than quietly
//! measuring nothing, because "no trial copy" is not evidence that the repair works.
//!
//! The upgrade is the production backfill — `Dictionary::index_secondary`, the same call
//! `setup::backfill::rebuild_gloss_indexes` makes — so what is measured is the path a reader would
//! actually get, not a bespoke index built to suit the test.
//!
//! **Every case is recorded on its own.** One target existing says nothing about the other six:
//! each entry is a different word with a different sequence, and a cross-reference can point at a
//! target this dictionary does not hold.

use std::path::{Path, PathBuf};

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::model::Token;

/// The seven tokens whose senses were nothing but a pointer: surface, the dictionary form the
/// analyzer gave it, and the notebook line each came from.
///
/// **The lemma is carried, not re-derived.** A lookup keys on `dict_form` when there is one, and
/// 差しこんで's is 差しこむ — replaying it under its surface asked a question the reader's session
/// never asked, found nothing, and looked like the repair failing. Taken from the trace, so each
/// case is replayed as the analyzer actually presented it.
///
/// Lexical items only — the passages stay in the ignored diagnostic artifact.
const OBSERVED: [(&str, &str, usize); 7] = [
    ("アタシ", "アタシ", 6),
    ("メイカー", "メイカー", 19),
    ("くッ", "くッ", 48),
    ("生きもの", "生きもの", 62),
    ("差しこんで", "差しこむ", 65),
    ("抜きとる", "抜きとる", 65),
    ("そり", "そり", 123),
];

fn trial_copy() -> PathBuf {
    let raw = std::env::var_os("SAKATSU_XREF_TRIAL").expect(
        "SAKATSU_XREF_TRIAL must point at an isolated copy of the installed dictionaries. \
         This test never touches the reader's own store.",
    );
    PathBuf::from(raw)
}

fn token(surface: &str, dict_form: &str) -> Token {
    Token {
        surface: surface.to_string(),
        dict_form: dict_form.to_string(),
        start: Some(0),
        end: Some(surface.len()),
        ..Default::default()
    }
}

fn is_pointer(gloss: &str) -> bool {
    gloss.contains(" redirected from ") || gloss.trim_start().starts_with('\u{27f6}')
}

/// Guard: the trial copy must not be the store the application reads.
fn refuse_the_real_store(trial: &Path) {
    let real = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join("com.sakatsu.app")
        .join("dictionaries");
    let (trial, real) = (
        trial.canonicalize().unwrap_or_else(|_| trial.to_path_buf()),
        real.canonicalize().unwrap_or(real),
    );
    assert!(
        !trial.starts_with(&real),
        "{} is inside the reader's own dictionary store at {}; this test upgrades what it is \
         given and must be given a copy",
        trial.display(),
        real.display()
    );
}

#[test]
#[ignore = "needs an isolated copy of the installed dictionaries in SAKATSU_XREF_TRIAL"]
fn the_seven_observed_cases_resolve_once_the_sequence_index_exists() {
    let trial = trial_copy();
    refuse_the_real_store(&trial);

    let found = Dictionary::discover(&trial);
    assert!(
        !found.is_empty(),
        "no dictionary under {} — copy the installed index there first",
        trial.display()
    );

    // **Before**, so the upgrade is shown to be the thing that changed the answers.
    let before: Vec<(String, bool)> = found
        .iter()
        .map(|path| {
            let dictionary = Dictionary::open(path).expect("the copy opens");
            (dictionary.meta().title.clone(), dictionary.has_sequence())
        })
        .collect();
    println!("\n=== the trial copy, before the upgrade ===");
    for (title, sequence) in &before {
        println!("  {title:<30} sequence bucket: {sequence}");
    }

    println!("\n=== running the production backfill on the copy ===");
    for path in &found {
        let dictionary = Dictionary::open(path).expect("the copy opens");
        if !dictionary.needs_secondary_index() {
            println!("  {} already complete", dictionary.meta().title);
            continue;
        }
        let built = dictionary
            .index_secondary()
            .expect("the backfill publishes a generation");
        println!("  {} rebuilt: {built} entries", dictionary.meta().title);
    }

    // Reopen: a backfill publishes a **new generation**, and a handle held across one keeps its
    // own by design, so a reader that did not reopen would see the old index and no change.
    let dictionaries: Vec<Dictionary> = Dictionary::discover(&trial)
        .iter()
        .map(|path| Dictionary::open(path).expect("the upgraded copy opens"))
        .collect();
    println!("\n=== after the upgrade ===");
    for dictionary in &dictionaries {
        println!(
            "  {:<30} sequence bucket: {}",
            dictionary.meta().title,
            dictionary.has_sequence()
        );
    }
    assert!(
        dictionaries.iter().any(Dictionary::has_sequence),
        "the backfill did not produce a sequence bucket, so nothing below would mean anything"
    );

    println!("\n=== the seven observed cases, each on its own ===");
    let mut resolved = 0usize;
    let mut unresolved: Vec<String> = Vec::new();
    for (surface, dict_form, line) in OBSERVED {
        let tokens = [token(surface, dict_form)];
        let looked = match sakatsu_core::lookup::with_evidence(&tokens, 0, &dictionaries) {
            Ok(looked) => looked,
            Err(error) => {
                unresolved.push(format!("{surface} (L{line}): lookup failed: {error}"));
                continue;
            }
        };
        let word = &looked.view.word;
        let glosses: Vec<String> = word
            .inline
            .iter()
            .flat_map(|sense| sense.glosses.clone())
            .collect();
        let pointers = glosses.iter().filter(|gloss| is_pointer(gloss)).count();
        let scope = format!("{:?}", word.inline_retrieved);

        println!(
            "  {surface:<8} ({dict_form:<8}) L{line:<4} senses={:<3} pointers={pointers}              source={:?} scope={scope}",
            glosses.len(),
            word.inline_source.as_deref().unwrap_or("-")
        );
        for gloss in glosses.iter().take(2) {
            println!("        {gloss}");
        }

        // **The selected word keeps its identity.** The headword is the token's own dictionary
        // form — 差しこんで is headed 差しこむ, which is the lemma the analyzer gave it and not a
        // rename. What must never happen is the *target's* spelling replacing it: following
        // 差しこむ's pointer must not leave the reader looking at 差し込む, a word they did not
        // select and did not read.
        assert_eq!(
            word.headword, dict_form,
            "{surface}: headed as something other than its own dictionary form"
        );

        if glosses.is_empty() || pointers > 0 {
            unresolved.push(format!(
                "{surface} (L{line}): still {} pointer sense(s) of {}",
                pointers,
                glosses.len()
            ));
        } else {
            resolved += 1;
            // Attribution and scope survive resolution.
            assert!(
                word.inline_source.is_some(),
                "{surface}: a resolved sense must still name the dictionary it came from"
            );
            assert!(
                scope.contains("Whole"),
                "{surface}: retrieval scope changed to {scope}"
            );
        }
    }

    println!("\n  resolved {resolved} of {}", OBSERVED.len());
    for line in &unresolved {
        println!("    UNRESOLVED  {line}");
    }
    assert!(
        unresolved.is_empty(),
        "{} of {} observed cases still show a pointer after the upgrade: {unresolved:?}",
        unresolved.len(),
        OBSERVED.len()
    );
}
