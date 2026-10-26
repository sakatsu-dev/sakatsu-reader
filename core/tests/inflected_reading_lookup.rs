//! An inflected word's dictionary entries are told apart by its dictionary form's reading.
//!
//! ```text
//! cargo test -p sakatsu-core --test inflected_reading_lookup -- --ignored
//! ```
//!
//! Needs the installed Jitendex (`SAKATSU_DICT_DIR`, or the app's own folder under `%APPDATA%`).
//!
//! **What went wrong.** The lookup asks for the dictionary form — 弾く for 弾いている — and then
//! puts the entry whose reading matches the token first. It matched against the *surface* reading,
//! ひいている, which no entry has, so for every inflected word the dictionary's own order stood:
//! 弾く【はじく】 "to flip; to snap" ahead of 弾く【ひく】 "to play (an instrument)". A sister
//! playing the guitar was shown flicking it. Found while taking README screenshots, 2026-10-19.
//!
//! `lookup.rs`'s unit tests pin the reading that is chosen. This checks the outcome a reader sees,
//! against the real dictionary.

mod common;

use sakatsu_core::dict::index::Dictionary;
use sakatsu_core::lookup;
use sakatsu_core::model::Token;

fn first_gloss(
    dictionary: &Dictionary,
    surface: &str,
    dict_form: &str,
    reading: &str,
    lemma: &str,
) -> String {
    let token = Token {
        surface: surface.into(),
        dict_form: dict_form.into(),
        reading: reading.into(),
        lemma_reading: lemma.into(),
        ..Token::default()
    };
    let word = lookup::word(&token, std::slice::from_ref(dictionary)).expect("lookup");
    word.inline
        .first()
        .map(|sense| sense.glosses.join("; "))
        .unwrap_or_default()
}

#[test]
#[ignore = "needs the installed Jitendex"]
fn an_inflected_heteronym_gets_the_entry_for_its_own_reading() {
    let dictionary = Dictionary::open(&common::dictionaries_dir().join("jitendex"))
        .expect("Jitendex installed (set SAKATSU_DICT_DIR)");

    // The case itself: 弾いている read ひいている is 弾く read ひく — playing an instrument.
    let playing = first_gloss(&dictionary, "弾いている", "弾く", "ひいている", "ひく");
    assert!(
        playing.contains("to play"),
        "弾いている (ひく) should play, got: {playing}"
    );

    // The other reading still wins when it is the one read: 弾いた read はじいた is はじく.
    let flicked = first_gloss(&dictionary, "弾いた", "弾く", "はじいた", "はじく");
    assert!(
        flicked.contains("to flip"),
        "弾いた (はじく) should flip, got: {flicked}"
    );

    // An uninflected word was never affected, and still is not.
    let plain = first_gloss(&dictionary, "弾く", "弾く", "ひく", "ひく");
    assert!(
        plain.contains("to play"),
        "弾く (ひく) should play, got: {plain}"
    );
}
