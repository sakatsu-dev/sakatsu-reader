//! Whether a cached answer can outlive the tokenization it was computed for.
//!
//! # The question
//!
//! Grouping changes which words a passage has. A contextual answer — the sense ranking for word
//! *n* of a passage — is about a particular word in a particular sequence, so an entry made before
//! grouping must not be served after it. The same goes the other way: replacing a dictionary can
//! change which units are supported, so answers derived from the old dictionaries must go.
//!
//! # What makes it hold
//!
//! Two things, and neither is new work — they are why this is checked rather than built:
//!
//! - the contextual key is `(epoch, PassageDigest::of(tokens), index)`, and the digest covers the
//!   **tokens**, not the source text, so a different tokenization is a different key;
//! - `WarmupState::invalidate` bumps the epoch, and the maintenance command already calls it when
//!   dictionaries are replaced.
//!
//! There is no cache of the analysis itself, so there is no stale tokenization to serve: every
//! study and analyze call recomputes. This asserts those properties rather than trusting a reading
//! of the code, because "the key happens to include it" is the kind of thing a refactor removes.

use sakatsu_core::model::Token;
use sakatsu_desktop_lib::warmup::PassageDigest;

fn token(surface: &str, at: usize) -> Token {
    Token {
        surface: surface.into(),
        dict_form: surface.into(),
        reading: surface.into(),
        lemma_reading: surface.into(),
        start: Some(at),
        end: Some(at + surface.len()),
        ..Default::default()
    }
}

/// The same source, segmented two ways: what the analyzer gave, and what grouping made of it.
fn ungrouped() -> Vec<Token> {
    vec![token("かっこ", 0), token("いい", 9), token("曲", 15)]
}

fn grouped() -> Vec<Token> {
    let mut unit = token("かっこいい", 0);
    unit.dict_form = "格好いい".into();
    unit.lemma_reading = "かっこいい".into();
    unit.components = vec![token("かっこ", 0), token("いい", 9)];
    vec![unit, token("曲", 15)]
}

/// **A contextual answer cannot outlive the tokenization it was about.**
#[test]
fn grouping_changes_the_contextual_cache_key() {
    let before = PassageDigest::of(&ungrouped()).expect("the tokens serialize");
    let after = PassageDigest::of(&grouped()).expect("the tokens serialize");
    assert_ne!(
        before, after,
        "the same passage grouped and ungrouped shares a cache key, so an answer about \
         かっこ could be served for かっこいい"
    );
}

/// The digest is over the tokens, so anything that changes a token changes the key — including
/// the fields grouping sets on a unit while leaving its surface alone.
#[test]
fn a_units_own_fields_are_part_of_the_key() {
    let mut plain = grouped();
    let keyed = PassageDigest::of(&plain).expect("serializes");

    // The same surfaces, a different dictionary form: a different word, so a different key.
    plain[0].dict_form = "恰好良い".into();
    assert_ne!(
        keyed,
        PassageDigest::of(&plain).expect("serializes"),
        "two words spelled the same share a cache key"
    );

    // …and the components, which are what a reader selects into.
    let mut without = grouped();
    without[0].components.clear();
    assert_ne!(
        keyed,
        PassageDigest::of(&without).expect("serializes"),
        "a unit with and without its components share a cache key"
    );
}

/// Identical tokens key identically, or the cache would never hit at all.
#[test]
fn the_same_tokens_key_the_same() {
    assert_eq!(
        PassageDigest::of(&grouped()).expect("serializes"),
        PassageDigest::of(&grouped()).expect("serializes")
    );
}
