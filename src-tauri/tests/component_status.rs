//! Marking a part of a word, against a store of its own.
//!
//! # The claim under test
//!
//! Grouping joins かっこ and いい into one word. The reader can now open either part in the panel,
//! and marking one must be about **that one**: marking かっこ Known must not mark かっこいい
//! Known, and must not make the whole word render as a word the reader has finished with.
//!
//! That holds because a status is filed under [`WordKey`] — the dictionary form and its reading —
//! and the unit's key is its own, not any constituent's. This asserts it against a real store
//! rather than trusting the derivation, because "different keys" is exactly the kind of property
//! a later change to how a unit's `dict_form` is chosen could quietly break.
//!
//! # The store is the test's own
//!
//! A temporary directory per test. The reader's own vocabulary is never opened, never read and
//! never written.

use sakatsu_core::model::Token;
use sakatsu_core::store::{Store, WordKey, WordStatus};

fn token(surface: &str, dict_form: &str, reading: &str) -> Token {
    Token {
        surface: surface.into(),
        dict_form: dict_form.into(),
        reading: reading.into(),
        lemma_reading: reading.into(),
        ..Default::default()
    }
}

/// かっこいい as assembly builds it: its own dictionary form, with its constituents kept.
fn grouped_unit() -> Token {
    Token {
        components: vec![
            token("かっこ", "かっこ", "かっこ"),
            token("いい", "いい", "いい"),
        ],
        ..token("かっこいい", "格好いい", "かっこいい")
    }
}

/// A directory of this test's own, removed when the guard drops.
///
/// Hand-rolled rather than pulled in: the desktop crate has no temporary-directory dependency,
/// and adding one for four tests is more than this needs. The name carries the process id and the
/// test's own name, because `cargo test` runs share one temp root and a fixed name would let two
/// runs delete each other's store.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "sakatsu-component-status-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(name: &str) -> (Scratch, Store) {
    let scratch = Scratch::new(name);
    let store = Store::open(&scratch.0).expect("the store opens");
    (scratch, store)
}

/// **The whole word and its parts are three different words.**
#[test]
fn marking_a_part_does_not_mark_the_whole_word() {
    let (_scratch, store) = store("marking_a_part_does_not_mark_the_whole_word");
    let unit = grouped_unit();
    let part = &unit.components[0];

    store
        .set_status(WordKey::for_token(part), WordStatus::Known)
        .expect("the part marks");

    assert_eq!(
        store.status(&WordKey::for_token(part)),
        WordStatus::Known,
        "the part the reader marked"
    );
    assert_eq!(
        store.status(&WordKey::for_token(&unit)),
        WordStatus::New,
        "the whole word must be untouched — its key is 格好いい/かっこいい, not かっこ's"
    );
    assert_eq!(
        store.status(&WordKey::for_token(&unit.components[1])),
        WordStatus::New,
        "and so must the other part"
    );
}

/// …and the other way round, which is the case a reader actually does first.
#[test]
fn marking_the_whole_word_does_not_mark_its_parts() {
    let (_scratch, store) = store("marking_the_whole_word_does_not_mark_its_parts");
    let unit = grouped_unit();

    store
        .set_status(WordKey::for_token(&unit), WordStatus::Known)
        .expect("the unit marks");

    assert_eq!(store.status(&WordKey::for_token(&unit)), WordStatus::Known);
    for part in &unit.components {
        assert_eq!(
            store.status(&WordKey::for_token(part)),
            WordStatus::New,
            "{}: a part is not learned by learning the word it is in",
            part.surface
        );
    }
}

/// **Nested parts are reached and marked independently.**
///
/// A component can have components. The key is computed from whichever token is handed over, so
/// depth costs nothing — asserted rather than assumed, because the panel walks a path to get
/// there and a path is easy to resolve to the wrong depth.
#[test]
fn a_nested_part_is_its_own_word() {
    let (_scratch, store) = store("a_nested_part_is_its_own_word");
    let inner = token("いい", "いい", "いい");
    let middle = Token {
        components: vec![token("かっこ", "かっこ", "かっこ"), inner.clone()],
        ..token("かっこいい", "格好いい", "かっこいい")
    };
    let outer = Token {
        components: vec![middle.clone(), token("ね", "ね", "ね")],
        ..token("かっこいいね", "格好いいね", "かっこいいね")
    };

    store
        .set_status(WordKey::for_token(&inner), WordStatus::Known)
        .expect("the innermost marks");

    assert_eq!(store.status(&WordKey::for_token(&inner)), WordStatus::Known);
    assert_eq!(store.status(&WordKey::for_token(&middle)), WordStatus::New);
    assert_eq!(store.status(&WordKey::for_token(&outer)), WordStatus::New);
}

/// A part that spells a word standing elsewhere in the passage **is** that word.
///
/// Not a special case — it is what whole words already do, and the reason statuses are keyed
/// rather than indexed. Recorded so the shared behaviour is deliberate rather than incidental.
#[test]
fn a_part_and_a_standalone_word_with_one_key_are_one_word() {
    let (_scratch, store) = store("a_part_and_a_standalone_word_with_one_key_are_one_word");
    let unit = grouped_unit();
    let standalone = token("かっこ", "かっこ", "かっこ");

    store
        .set_status(WordKey::for_token(&unit.components[0]), WordStatus::Known)
        .expect("the part marks");

    assert_eq!(
        store.status(&WordKey::for_token(&standalone)),
        WordStatus::Known,
        "they are the same word, filed under the same key"
    );
}
