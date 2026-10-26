//! Anki, against a **real** running collection.
//!
//! Ignored by default: needs Anki open with the AnkiConnect add-on installed, which CI does
//! not have.
//!
//! ```text
//! cargo test -p sakatsu-core --test anki_live -- --ignored --nocapture
//! ```
//!
//! `anki_stub.rs` proves the protocol — what bytes go out, how a refusal is read — against a
//! socket this repo controls. That is the right shape for the parsing rules and says nothing
//! about whether the export works: the stub answers whatever it is told to, including things
//! a real AnkiConnect never would. These tests are the other half, and they exist because
//! "does the Anki button work" turned out not to be answerable from the unit tests at all.
//!
//! **Only one test writes, and it will not run unless you name a deck.** Everything else is a
//! read — `version`, `deckNames`. The add is gated on `SAKATSU_ANKI_TEST_DECK` on top of
//! `--ignored`, because a test that picks its own deck puts cards in somebody's collection for
//! the crime of running the suite, and it leaves them there: this client cannot delete, and a
//! test that could delete cards is a worse thing to keep in a repo than a card somebody asked
//! for.
//!
//! ```text
//! SAKATSU_ANKI_TEST_DECK="scratch" cargo test -p sakatsu-core --test anki_live -- --ignored --nocapture
//! ```

use sakatsu_core::anki::{Anki, AnkiError, Note};
use sakatsu_core::dict::merge::Sense;
use sakatsu_core::dict::WordView;

/// Whether Anki is there at all, so the rest can skip rather than fail on a machine
/// without it. The same shape the dictionary live tests use.
fn reachable(anki: &Anki) -> bool {
    match anki.version() {
        Ok(_) => true,
        Err(error) => {
            eprintln!("SKIP: no Anki on the default port ({error})");
            false
        }
    }
}

#[test]
#[ignore = "needs Anki running with AnkiConnect"]
fn ankiconnect_answers_on_the_port_the_app_uses() {
    let anki = Anki::default();
    let Ok(version) = anki.version() else {
        eprintln!("SKIP: no Anki");
        return;
    };
    // The app sends `"version": 6` on every call, so a collection speaking something older
    // would refuse actions with an error the reader cannot act on.
    assert!(
        version >= 6,
        "AnkiConnect speaks generation {version}; the app sends 6"
    );
    println!("AnkiConnect generation {version}");
}

/// **The liveness probe the whole feature hangs off.**
///
/// `anki_decks` is what the UI calls at startup, and `ankiReady` — which decides whether the
/// "+ Anki" button is drawn at all — is just "did this come back non-empty". So a failure
/// here is invisible in the interface: no button, no error, nothing to click.
#[test]
#[ignore = "needs Anki running with AnkiConnect"]
fn the_deck_list_the_ui_gates_on_comes_back_non_empty() {
    let anki = Anki::default();
    if !reachable(&anki) {
        return;
    }

    let decks = anki.deck_names().expect("deckNames");
    assert!(
        !decks.is_empty(),
        "an empty list is indistinguishable from Anki being closed, and hides the button"
    );
    println!("{} decks: {:?}", decks.len(), &decks[..decks.len().min(5)]);
}

/// **The whole export, end to end, against a real collection.**
///
/// Opt-in twice over: `--ignored` like the rest, *and* it refuses to run without
/// `SAKATSU_ANKI_TEST_DECK` naming a deck to write to. A test that picks its own deck is a
/// test that puts cards in somebody's collection because they ran the suite.
///
/// ```text
/// SAKATSU_ANKI_TEST_DECK="屠つ test" cargo test -p sakatsu-core --test anki_live -- --ignored --nocapture
/// ```
///
/// It adds, then adds the same word again to see the duplicate refused. It does **not** clean
/// up: deleting notes is not something this client can do, and a test that could delete cards
/// is a worse thing to have in the repo than two cards in a deck somebody named on purpose.
#[test]
#[ignore = "needs Anki running, and a deck named in SAKATSU_ANKI_TEST_DECK"]
fn a_word_becomes_a_card_and_the_same_word_twice_does_not() {
    let Some(deck) = std::env::var_os("SAKATSU_ANKI_TEST_DECK") else {
        eprintln!("SKIP: SAKATSU_ANKI_TEST_DECK is not set");
        return;
    };
    let deck = deck.to_string_lossy().to_string();

    let anki = Anki::default();
    if !reachable(&anki) {
        return;
    }

    // Exactly what `add_to_anki` builds: the looked-up word, and the passage it was met in.
    let word = WordView {
        headword: "気持ち".into(),
        reading: Some("きもち".into()),
        inline: vec![Sense {
            tags: vec!["n".into()],
            glosses: vec!["feeling".into(), "sensation".into(), "mood".into()],
            sequence: None,
        }],
        inline_source: None,
        inline_retrieved: sakatsu_core::dict::Retrieved::Unknown,
        also_in: Vec::new(),
        examples: Vec::new(),
        name: None,
        likely_name: false,
        romaji: None,
    };
    let note = Note::from_word(&word, "気持ちが伝わってきます。", "Jitendex");
    assert_eq!(note.expression, "気持ち");
    assert_eq!(note.meaning, "feeling; sensation; mood");
    assert_eq!(note.sentence, "気持ちが伝わってきます。");

    let id = anki.add(&deck, &note).expect("the card should be added");
    println!("added note {id} to {deck:?}");

    // **The duplicate is the half most likely to rot.** `allowDuplicate: false` is only
    // honoured if the note type and scope line up, and getting it wrong means a reader
    // quietly accumulating the same card every time they look a word up.
    match anki.add(&deck, &note) {
        Err(AnkiError::Refused(said)) => println!("the second add was refused: {said}"),
        Ok(second) => panic!("the same word was added twice: {id} and {second}"),
        Err(other) => panic!("expected a refusal, got {other}"),
    }
}

/// A note with nothing on it is refused **before** anything reaches the network.
///
/// Worth checking against the real thing rather than only the stub, because the stub cannot
/// tell the difference: it answers whatever it was told to whether or not a request arrived.
/// Here a refusal that came back without Anki being consulted is the only way to get
/// `NothingToAdd`, since a real AnkiConnect would answer `addNote` with something else.
#[test]
#[ignore = "needs Anki running with AnkiConnect"]
fn an_empty_note_never_reaches_the_collection() {
    let anki = Anki::default();
    if !reachable(&anki) {
        return;
    }
    assert_eq!(
        anki.add("Default", &Default::default()),
        Err(AnkiError::NothingToAdd),
        "an empty note must be refused here, not sent to Anki to be refused there"
    );
}
