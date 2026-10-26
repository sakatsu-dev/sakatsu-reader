//! Does every inflected word in the suite reach a dictionary entry?
//!
//! ```text
//! cargo test -p sakatsu-core --test conjugated_lookup -- --ignored --nocapture
//! ```
//!
//! # Why the spread is the point
//!
//! `jitendex_live.rs` already checks that 食べる can be looked up. What it cannot check is
//! the case that actually bites: the reader makes **every** word clickable, so a form the
//! analyzer resolves to a dictionary form nothing indexes is invisible until someone
//! clicks that exact word — and then it says "Definition unavailable" for a word that is
//! plainly in the dictionary.
//!
//! Running every word of fifty native sentences through the lookup is what finds those,
//! because a corpus contains the inflections a hand-written test never thinks of.

mod common;
#[path = "common/sentences.rs"]
mod sentences;

use std::collections::BTreeSet;

use sakatsu_core::analyzer::lindera::LinderaAnalyzer;
use sakatsu_core::analyzer::Analyzer;
use sakatsu_core::dict::Dictionary;
use sakatsu_core::model::Token;

use sentences::SENTENCES;

/// The term the app would look a token up by — `commands::lookup` uses the dictionary
/// form, falling back to the surface.
fn lookup_term(token: &Token) -> &str {
    if token.dict_form.is_empty() {
        &token.surface
    } else {
        &token.dict_form
    }
}

#[test]
#[ignore = "needs the imported Jitendex"]
fn every_word_in_the_suite_reaches_an_entry() {
    let dir = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&dir) {
        println!("SKIP: no imported Jitendex at {}", dir.display());
        return;
    }
    let dictionary = Dictionary::open(&dir).expect("opens");
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    let mut found = 0;
    let mut missing: BTreeSet<String> = BTreeSet::new();
    let mut inflected = 0;

    for sentence in SENTENCES {
        let analysis = analyzer.analyze(sentence.text).expect("analyses");
        for token in &analysis.tokens {
            if !token.is_word() {
                continue;
            }
            let term = lookup_term(token);
            if term.is_empty() {
                continue;
            }
            // Only count the interesting half: a word already in its dictionary form is
            // `jitendex_live`'s territory, and padding the number with those would make
            // this look better than it is.
            if token.surface != token.dict_form {
                inflected += 1;
            }

            if dictionary
                .lookup(term)
                .map(|hits| !hits.is_empty())
                .unwrap_or(false)
            {
                found += 1;
            } else {
                missing.insert(format!("{} (from {})", term, token.surface));
            }
        }
    }

    let total = found + missing.len();
    println!("\n=== lookups across the suite ===");
    println!("  words             {total}");
    println!("  of those inflected {inflected}");
    println!("  reached an entry  {found}");
    println!("  reached nothing   {}", missing.len());

    if !missing.is_empty() {
        println!("\n  no entry for:");
        for word in &missing {
            println!("    {word}");
        }
        println!("  (each is a word the reader will offer and then have nothing to say about)");
    }

    let rate = if total == 0 {
        1.0
    } else {
        found as f64 / total as f64
    };
    println!("\n  reached {:.1}%", rate * 100.0);
    assert!(
        rate >= FLOOR,
        "only {:.1}% of {total} words reached an entry, floor is {:.0}%",
        rate * 100.0,
        FLOOR * 100.0
    );
}

/// Measured at **99.4% — 662 of 666 words**, so the floor sits below that. A ratchet.
///
/// The four that reached nothing were アンジェラ, デイヴィド, モントリオール and a stray
/// ペ: three personal or place names and a fragment. Names are not what a general
/// dictionary is for.
///
/// And the real figure is probably higher than this test can see, which is worth stating
/// rather than quietly banking. This searches **Jitendex alone**; the app looks through
/// Jitendex *and* JMnedict, which is a dictionary of exactly the names listed above. So
/// 99.4% is a lower bound on what a learner actually experiences, not a measurement of it.
const FLOOR: f64 = 0.95;

/// A productive する-verb reaches the noun JMdict files it under.
///
/// **Found by using the app, not by this suite.** 請求させていただきます showed a reader no
/// definitions at all: the analyzer reports a suru-verb's dictionary form as `<noun>する` and
/// JMdict files those under the bare noun with a `vs` tag, so 請求する was a term nothing indexed.
/// 勉強します and 確認した did the same. 19,520 entries are tagged that way — 4.5% of the
/// dictionary — and every one of them was unreachable.
///
/// This suite did surface them, inside its "reached nothing" list, and 98.0% read as comfortable
/// enough that nobody read which words. The rate is now 99.0% and the remainder are proper nouns.
///
/// **愛する is the control and matters more than the rest.** It is genuinely its own JMdict term,
/// so it must match exactly and never reach the fallback — 愛する means "to love", while its stem
/// 愛 is the noun "love", and a fallback that fired here would quietly answer a different word.
#[test]
#[ignore = "needs the imported Jitendex"]
fn a_suru_verb_reaches_the_noun_it_is_built_on() {
    let dir = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&dir) {
        println!("SKIP: no imported Jitendex");
        return;
    }
    let dictionary = Dictionary::open(&dir).expect("opens");

    for (query, expected) in [
        ("請求する", "請求"),
        ("勉強する", "勉強"),
        ("確認する", "確認"),
    ] {
        let hits = dictionary.lookup(query).expect("looks up");
        assert!(
            !hits.is_empty(),
            "{query} reached nothing; JMdict files it under {expected}"
        );
        assert_eq!(hits[0].entry.term, expected);
        assert_eq!(
            hits[0].matched,
            sakatsu_core::dict::index::MatchKind::SuruStem,
            "{query} should be marked as reaching its stem, not as written"
        );
    }

    // The control: a する-verb that is its own entry is answered exactly.
    let hits = dictionary.lookup("愛する").expect("looks up");
    assert_eq!(hits[0].entry.term, "愛する");
    assert_ne!(
        hits[0].matched,
        sakatsu_core::dict::index::MatchKind::SuruStem,
        "愛する is its own term and must not be answered with 愛"
    );
}

/// The specific ladder, spelled out, because it is the one a learner meets constantly.
#[test]
#[ignore = "needs the imported Jitendex"]
fn every_inflection_of_taberu_finds_taberu() {
    let dir = common::dictionaries_dir().join("jitendex");
    if !sakatsu_core::dict::index::Dictionary::exists_in(&dir) {
        println!("SKIP: no imported Jitendex");
        return;
    }
    let dictionary = Dictionary::open(&dir).expect("opens");
    let analyzer = LinderaAnalyzer::new().expect("embedded UniDic loads");

    for surface in [
        "食べた",
        "食べない",
        "食べたい",
        "食べられる",
        "食べさせられた",
        "食べています",
    ] {
        let analysis = analyzer.analyze(surface).expect("analyses");
        let first = analysis
            .tokens
            .iter()
            .find(|token| token.is_word())
            .unwrap_or_else(|| panic!("{surface} has no word token"));

        let term = lookup_term(first);
        let hits = dictionary.lookup(term).expect("lookup");
        assert!(
            !hits.is_empty(),
            "{surface} resolved to {term:?}, which is in no dictionary — the reader would \
             offer this word and then have nothing to show"
        );
        println!("  {surface:<16} → {term:<8} {} hit(s)", hits.len());
    }
}
