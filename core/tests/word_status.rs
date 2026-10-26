//! Marking a word, and getting the same word back.
//!
//! Runs everywhere: no dictionary, no sidecar, no models.
//!
//! # Why this exists separately from the store's own unit tests
//!
//! Those check that a value written comes back. What they do not check is the thing
//! `WordKey::for_token` was written for: **上手 read じょうず and 上手 read うわて are
//! different words.** They are spelled identically, they mean different things, and a
//! learner who marks one as known must not be told they know the other.
//!
//! That is a quiet failure. Nothing errors, nothing looks wrong — a word simply stops
//! being offered, and the learner never finds out they were told they knew something they
//! have never seen. The key exists to prevent it and nothing tested that end to end.

use sakatsu_core::model::Token;
use sakatsu_core::store::{Store, WordKey, WordStatus};

/// A directory of its own per test, removed afterwards.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("sakatsu-status-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn token(surface: &str, dict_form: &str, lemma_reading: &str) -> Token {
    Token {
        surface: surface.into(),
        dict_form: dict_form.into(),
        lemma_reading: lemma_reading.into(),
        ..Default::default()
    }
}

/// A mark survives the app closing.
#[test]
fn a_mark_outlives_the_store_that_wrote_it() {
    let scratch = Scratch::new("roundtrip");

    let key = WordKey::new("勉強", "べんきょう");
    {
        let store = Store::open(&scratch.0).expect("opens");
        store
            .set_status(key.clone(), WordStatus::Known)
            .expect("writes");
        assert_eq!(store.status(&key), WordStatus::Known);
    } // dropped — whatever is held in memory goes with it

    let reopened = Store::open(&scratch.0).expect("reopens");
    assert_eq!(
        reopened.status(&key),
        WordStatus::Known,
        "a mark that does not survive a restart is not persistence"
    );
}

/// **The case `WordKey` exists for.**
///
/// 上手 is じょうず (skilled) or うわて (the upper hand). Same spelling, different words.
#[test]
fn two_words_spelled_the_same_are_marked_separately() {
    let scratch = Scratch::new("homograph");
    let store = Store::open(&scratch.0).expect("opens");

    let skilled = WordKey::for_token(&token("上手", "上手", "じょうず"));
    let upper_hand = WordKey::for_token(&token("上手", "上手", "うわて"));

    assert_ne!(
        skilled, upper_hand,
        "identical spelling with different readings must not collapse to one key"
    );

    store
        .set_status(skilled.clone(), WordStatus::Known)
        .expect("writes");

    assert_eq!(store.status(&skilled), WordStatus::Known);
    assert_eq!(
        store.status(&upper_hand),
        WordStatus::New,
        "marking じょうず must not claim うわて is known too"
    );

    // And the other way round, so this cannot pass by both being unmarked.
    store
        .set_status(upper_hand.clone(), WordStatus::Seen)
        .expect("writes");
    assert_eq!(store.status(&skilled), WordStatus::Known);
    assert_eq!(store.status(&upper_hand), WordStatus::Seen);
}

/// A conjugated token is marked as its dictionary form, not as the form it appeared in.
///
/// 食べた and 食べない are the same word met twice. Marking one has to mark the word, or
/// the learner is asked about 食べる again for every ending it ever takes.
#[test]
fn every_inflection_of_a_word_is_one_entry() {
    let scratch = Scratch::new("inflection");
    let store = Store::open(&scratch.0).expect("opens");

    let past = WordKey::for_token(&token("食べた", "食べる", "たべる"));
    let negative = WordKey::for_token(&token("食べない", "食べる", "たべる"));
    let causative = WordKey::for_token(&token("食べさせられた", "食べる", "たべる"));

    assert_eq!(past, negative);
    assert_eq!(past, causative);

    store.set_status(past, WordStatus::Known).expect("writes");
    assert_eq!(store.status(&negative), WordStatus::Known);
    assert_eq!(store.status(&causative), WordStatus::Known);
}

/// A token with no dictionary form falls back to its surface.
///
/// The fallback in `WordKey::for_token`, which nothing exercised. Punctuation and words
/// the analyzer could not resolve arrive this way, and keying them on an empty string
/// would file every one of them under the same entry.
#[test]
fn a_token_with_no_dictionary_form_keys_on_its_surface() {
    let unresolved = WordKey::for_token(&token("ヷヷヷ", "", ""));
    assert_eq!(unresolved.term, "ヷヷヷ");

    let other = WordKey::for_token(&token("ヸヸヸ", "", ""));
    assert_ne!(
        unresolved, other,
        "two unresolved words must not share one key"
    );
}

/// The counts the header shows follow what was marked.
#[test]
fn the_counts_follow_the_marks() {
    let scratch = Scratch::new("counts");
    let store = Store::open(&scratch.0).expect("opens");

    store
        .set_status(WordKey::new("一", "いち"), WordStatus::Known)
        .expect("writes");
    store
        .set_status(WordKey::new("二", "に"), WordStatus::Known)
        .expect("writes");
    store
        .set_status(WordKey::new("三", "さん"), WordStatus::Seen)
        .expect("writes");
    store
        .set_status(WordKey::new("四", "よん"), WordStatus::Ignored)
        .expect("writes");

    let counts = store.counts();
    assert_eq!(counts.known, 2);
    assert_eq!(counts.seen, 1);
    assert_eq!(counts.ignored, 1);

    // Back to New should remove it from the counts rather than leave a ghost.
    store
        .set_status(WordKey::new("一", "いち"), WordStatus::New)
        .expect("writes");
    assert_eq!(store.counts().known, 1, "un-marking has to decrement");
}
