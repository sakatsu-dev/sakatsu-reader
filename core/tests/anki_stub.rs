//! The AnkiConnect client against a stub server.
//!
//! Hermetic — it binds a loopback port and speaks just enough HTTP/1.1 to answer.
//! That is worth the ~40 lines: the unit tests cover the reply envelope, but only a
//! real request proves what 屠つ actually *sends*, which is the part AnkiConnect
//! judges. A typo in an action name or a field would pass every other test and fail
//! against real Anki.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

use sakatsu_core::anki::{Anki, AnkiError, Note, DECK_PRESET, FIELDS, NOTE_TYPE};

/// A stub AnkiConnect that replies to each request from a scripted list, and reports
/// the request bodies it received.
struct Stub {
    url: String,
    requests: mpsc::Receiver<String>,
}

impl Stub {
    /// `replies` are the JSON bodies to send, in order.
    fn start(replies: Vec<String>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let (sender, requests) = mpsc::channel();

        std::thread::spawn(move || {
            for reply in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                match serve(stream, &reply) {
                    Ok(body) => {
                        if sender.send(body).is_err() {
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        });

        Stub { url, requests }
    }

    fn client(&self) -> Anki {
        Anki::new(&self.url)
    }

    /// The next request body the stub received, as JSON.
    fn received(&self) -> serde_json::Value {
        let body = self
            .requests
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("a request should have arrived");
        serde_json::from_str(&body).expect("the request body is JSON")
    }

    /// The next request, or `None` if nothing else arrived.
    ///
    /// For proving a call was *not* made, which for the destructive deck actions is the
    /// assertion that matters: a refusal that still sent `deleteDecks` would pass every test
    /// written the other way round.
    fn try_received(&self) -> Option<serde_json::Value> {
        self.requests
            .recv_timeout(std::time::Duration::from_millis(250))
            .ok()
            .map(|body| serde_json::from_str(&body).expect("the request body is JSON"))
    }
}

/// Read one HTTP request, answer with `reply`, and hand back the request body.
fn serve(mut stream: TcpStream, reply: &str) -> std::io::Result<String> {
    let mut reader = BufReader::new(stream.try_clone()?);

    // Headers, up to the blank line. Only Content-Length matters here.
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
    }

    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;

    stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            reply.len(),
            reply
        )
        .as_bytes(),
    )?;
    stream.flush()?;

    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// One deck entry point, so the name check can be asserted across all of them at once.
type DeckCall = dyn Fn(&Anki, &str) -> Result<(), AnkiError>;

fn note() -> Note {
    Note {
        expression: "食べる".into(),
        reading: "たべる".into(),
        meaning: "to eat".into(),
        // As the field is written now: Anki's furigana markup, with the word the card is about
        // marked. See `anki::sentence`.
        sentence: "ご飯を <b>食[た]べる</b>。".into(),
        source: "Jitendex".into(),
        part_of_speech: "ichidan verb".into(),
    }
}

#[test]
fn the_version_call_sends_the_action_and_api_generation() {
    let stub = Stub::start(vec![r#"{"result": 6, "error": null}"#.into()]);
    assert_eq!(stub.client().version().expect("version"), 6);

    let sent = stub.received();
    assert_eq!(sent["action"], "version");
    // AnkiConnect refuses a request whose version it does not recognise.
    assert_eq!(sent["version"], 6);
    // **And `params` must be an object, even with nothing in it.** AnkiConnect schema-checks
    // this field as an object, so the `null` that a parameterless action used to send came
    // back as "None is not of type 'object'" — every one of `version`, `deckNames` and
    // `modelNames`, which between them are the liveness probe and the note-type check.
    //
    // It failed invisibly: a failed `deckNames` reads to the interface as "Anki is not
    // running", so the button was simply never drawn and nothing said why. Asserted on the
    // wire rather than through a reply, because a stub will answer a malformed request
    // happily — which is exactly how this survived having tests at all.
    assert!(
        sent["params"].is_object(),
        "params must be an object, not {}: AnkiConnect rejects null",
        sent["params"]
    );
}

#[test]
fn adding_a_note_creates_the_note_type_only_when_it_is_missing() {
    let stub = Stub::start(vec![
        // modelNames: 屠つ's type is not there yet.
        r#"{"result": ["Basic"], "error": null}"#.into(),
        // createModel
        r#"{"result": {"id": 1}, "error": null}"#.into(),
        // addNote
        r#"{"result": 1496198395707, "error": null}"#.into(),
    ]);

    let id = stub.client().add("日本語", &note()).expect("added");
    assert_eq!(id, 1496198395707);

    assert_eq!(stub.received()["action"], "modelNames");

    let created = stub.received();
    assert_eq!(created["action"], "createModel");
    assert_eq!(created["params"]["modelName"], NOTE_TYPE);
    let ordered: Vec<String> =
        serde_json::from_value(created["params"]["inOrderFields"].clone()).expect("fields");
    assert_eq!(ordered, FIELDS);
    // The card and its styling travel with the model. Without these the note type arrives with
    // Anki's default template, which is what it used to do.
    let created_card = &created["params"]["cardTemplates"][0];
    let front = created_card["Front"].as_str().expect("a front");
    let back = created_card["Back"].as_str().expect("a back");
    // **The question must not print the reading it is asking for.** These two filters are one word
    // apart and swapping them silently turns the card back into one that shows you the answer.
    assert!(
        front.contains("{{kanji:Sentence}}") && !front.contains("{{furigana:"),
        "the front is giving the reading away: {front}"
    );
    assert!(
        back.contains("{{furigana:Sentence}}"),
        "the answer never shows the reading: {back}"
    );
    // Reusing the front would print the sentence twice on the answer, once bare and once with ruby.
    assert!(
        !back.contains("FrontSide"),
        "the sentence will appear twice on the answer: {back}"
    );
    assert!(
        created["params"]["css"]
            .as_str()
            .expect("styling")
            .contains(".nightMode"),
        "a card that only works on white is half a card"
    );

    let added = stub.received();
    assert_eq!(added["action"], "addNote");
    assert_eq!(added["params"]["note"]["deckName"], "日本語");
    assert_eq!(added["params"]["note"]["modelName"], NOTE_TYPE);
    assert_eq!(added["params"]["note"]["fields"]["Expression"], "食べる");
    assert_eq!(added["params"]["note"]["fields"]["Reading"], "たべる");
    // The furigana markup Anki's own filters read, with the word the card is about marked.
    assert_eq!(
        added["params"]["note"]["fields"]["Sentence"],
        "ご飯を <b>食[た]べる</b>。"
    );
    // Attribution is a condition of both shipped dictionaries' licences, not a nicety.
    assert_eq!(added["params"]["note"]["fields"]["Source"], "Jitendex");
    assert_eq!(
        added["params"]["note"]["fields"]["PartOfSpeech"],
        "ichidan verb"
    );
    // Anki's own duplicate check does the work rather than 屠つ guessing.
    assert_eq!(added["params"]["note"]["options"]["allowDuplicate"], false);
}

/// An existing note type is brought into step rather than left alone.
///
/// **This reverses what the test here used to pin.** The old rule was that an existing note type
/// was never touched, because rewriting one could damage a collection built on it — true, and its
/// cost was worse: every improvement to the card reached only people who had never used the
/// feature, so anyone with a card in their collection was frozen on the first design forever.
///
/// What is checked is the bound on the damage as much as the update itself: fields are **added**
/// and never removed, because removing a field deletes its content from every note that has it.
#[test]
fn an_existing_note_type_is_brought_into_step() {
    let stub = Stub::start(vec![
        // modelNames: it is already there, so createModel must not be called.
        format!(r#"{{"result": ["Basic", "{NOTE_TYPE}"], "error": null}}"#),
        // modelFieldNames: an old collection, missing everything added since.
        r#"{"result": ["Expression", "Reading", "Meaning", "Sentence"], "error": null}"#.into(),
        // modelFieldAdd × 2, then the template and the styling.
        r#"{"result": null, "error": null}"#.into(),
        r#"{"result": null, "error": null}"#.into(),
        r#"{"result": null, "error": null}"#.into(),
        r#"{"result": null, "error": null}"#.into(),
        r#"{"result": 42, "error": null}"#.into(),
    ]);

    assert_eq!(stub.client().add("Default", &note()).expect("added"), 42);

    assert_eq!(stub.received()["action"], "modelNames");
    assert_eq!(stub.received()["action"], "modelFieldNames");

    // Only what is missing, and only ever added.
    let first = stub.received();
    assert_eq!(first["action"], "modelFieldAdd");
    assert_eq!(first["params"]["fieldName"], "Source");
    let second = stub.received();
    assert_eq!(second["action"], "modelFieldAdd");
    assert_eq!(second["params"]["fieldName"], "PartOfSpeech");

    let templates = stub.received();
    assert_eq!(templates["action"], "updateModelTemplates");
    assert_eq!(templates["params"]["model"]["name"], NOTE_TYPE);

    let styling = stub.received();
    assert_eq!(styling["action"], "updateModelStyling");
    assert!(
        styling["params"]["model"]["css"]
            .as_str()
            .expect("styling")
            .contains(".nightMode"),
        "night mode is half the work"
    );

    assert_eq!(stub.received()["action"], "addNote");
}

#[test]
fn a_duplicate_is_refused_with_ankis_own_words() {
    let stub = Stub::start(vec![
        format!(r#"{{"result": ["{NOTE_TYPE}"], "error": null}}"#),
        r#"{"result": null, "error": "cannot create note because it is a duplicate"}"#.into(),
    ]);

    let error = stub
        .client()
        .add("Default", &note())
        .expect_err("duplicate");
    assert_eq!(
        error,
        AnkiError::Refused("cannot create note because it is a duplicate".into())
    );
    // It arrived as HTTP 200, so anything checking only the status would have missed it.
    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn deck_names_come_back_as_a_list() {
    let stub = Stub::start(vec![
        r#"{"result": ["Default", "日本語::語彙"], "error": null}"#.into(),
    ]);
    assert_eq!(
        stub.client().deck_names().expect("decks"),
        ["Default", "日本語::語彙"]
    );
}

const OURS: &str = r#"{"result": {"id": 7, "name": "屠つ (Sakatsu)"}, "error": null}"#;
const THEIRS: &str = r#"{"result": {"id": 1, "name": "Default"}, "error": null}"#;
const NOTHING: &str = r#"{"result": [], "error": null}"#;
const OK: &str = r#"{"result": null, "error": null}"#;
const APPLIED: &str = r#"{"result": true, "error": null}"#;

#[test]
fn ownership_is_asked_by_note_type_with_the_deck_name_escaped() {
    let stub = Stub::start(vec![
        r#"{"result": ["N_5"], "error": null}"#.into(),
        r#"{"result": [1, 2], "error": null}"#.into(),
        NOTHING.into(),
        OURS.into(),
    ]);

    let decks = stub.client().decks().expect("decks");
    assert_eq!((decks[0].ours, decks[0].foreign), (2, 0));
    assert!(decks[0].made_here);
    assert!(decks[0].managed());

    assert_eq!(stub.received()["action"], "deckNames");

    let ours = stub.received();
    assert_eq!(ours["action"], "findCards");
    let query = ours["params"]["query"].as_str().expect("a query");
    // `_` is a one-character wildcard in Anki's search **even inside quotes**, so an unescaped
    // `N_5` also counts `N5`'s cards — and a deck could then be judged safe to delete on the
    // strength of what is in a different deck.
    assert!(
        query.contains(r"N\_5"),
        "the deck name is not escaped: {query}"
    );
    // Ownership of *cards* is the note type, never the tag: `sakatsu` is one keystroke for a
    // learner to put on a card of their own.
    assert!(
        query.contains(NOTE_TYPE) && !query.contains("tag:"),
        "card ownership must be decided by note type: {query}"
    );

    assert!(stub.received()["params"]["query"]
        .as_str()
        .expect("a query")
        .contains(&format!("-\"note:{NOTE_TYPE}\"")));
    // And ownership of the *deck* is the options preset, which is where "we made this" lives.
    assert_eq!(stub.received()["action"], "getDeckConfig");
}

#[test]
fn a_deck_this_app_did_not_make_is_never_touched() {
    // The tighter half of the gate: every card in here is ours, and it is still refused, because
    // the deck itself was made in Anki. Content is not provenance.
    let stub = Stub::start(vec![
        r#"{"result": ["Core 2k"], "error": null}"#.into(),
        THEIRS.into(),
        OK.into(),
    ]);

    let error = stub.client().delete_deck("Core 2k").expect_err("refused");
    assert_eq!(error, AnkiError::NotManaged("Core 2k".into()));

    assert_eq!(stub.received()["action"], "deckNames");
    assert_eq!(stub.received()["action"], "getDeckConfig");
    // Refused on the mark alone — it never even counted the cards, let alone deleted any.
    assert!(
        stub.try_received().is_none(),
        "a deck we did not make was acted on"
    );
}

#[test]
fn a_deck_holding_cards_we_did_not_make_is_never_deleted() {
    // The other half: we made this deck, and the learner has since filed their own cards in it.
    // Making a deck does not make its contents ours.
    let stub = Stub::start(vec![
        r#"{"result": ["Japanese"], "error": null}"#.into(),
        OURS.into(),
        r#"{"result": [1502098034045], "error": null}"#.into(),
        OK.into(),
    ]);

    let error = stub.client().delete_deck("Japanese").expect_err("refused");
    assert_eq!(error, AnkiError::NotManaged("Japanese".into()));

    assert_eq!(stub.received()["action"], "deckNames");
    assert_eq!(stub.received()["action"], "getDeckConfig");
    assert_eq!(stub.received()["action"], "findCards");
    assert!(
        stub.try_received().is_none(),
        "deleteDecks went out for a deck holding cards we did not make"
    );
}

#[test]
fn deleting_a_deck_takes_its_subtree_but_not_a_deck_that_merely_starts_the_same() {
    let stub = Stub::start(vec![
        r#"{"result": ["Japanese", "Japanese::Vocab", "Japanese2"], "error": null}"#.into(),
        OURS.into(),
        NOTHING.into(),
        OK.into(),
    ]);

    stub.client().delete_deck("Japanese").expect("deleted");

    assert_eq!(stub.received()["action"], "deckNames");
    assert_eq!(stub.received()["action"], "getDeckConfig");
    assert_eq!(stub.received()["action"], "findCards");

    let deleted = stub.received();
    assert_eq!(deleted["action"], "deleteDecks");
    let decks: Vec<String> =
        serde_json::from_value(deleted["params"]["decks"].clone()).expect("decks");
    // A subtree is a match on `Japanese::`, not on `Japanese` — otherwise deleting one deck
    // silently takes an unrelated one whose name happens to begin the same way.
    assert_eq!(decks, ["Japanese", "Japanese::Vocab"]);
    // Every card in there is one of ours, which is the only reason this may be true.
    assert_eq!(deleted["params"]["cardsToo"], true);
}

#[test]
fn the_default_deck_is_never_touched_whatever_it_holds() {
    // Refused before a single question about it: no preset read, no card count. Anki keeps this
    // deck whatever happens, and every other deck's cards fall back to it.
    let stub = Stub::start(vec![
        r#"{"result": ["Default"], "error": null}"#.into(),
        OK.into(),
    ]);
    let error = stub.client().delete_deck("Default").expect_err("refused");
    assert!(matches!(error, AnkiError::Refused(_)));
    assert_eq!(stub.received()["action"], "deckNames");
    assert!(stub.try_received().is_none(), "Anki always keeps Default");
}

#[test]
fn a_new_deck_is_marked_as_ours() {
    let stub = Stub::start(vec![
        r#"{"result": ["Core 2k"], "error": null}"#.into(),
        THEIRS.into(),
        r#"{"result": 7, "error": null}"#.into(),
        OK.into(),
        APPLIED.into(),
    ]);

    stub.client().create_deck("Sakatsu-Deck").expect("created");

    assert_eq!(stub.received()["action"], "deckNames");
    assert_eq!(stub.received()["action"], "getDeckConfig");

    let made = stub.received();
    assert_eq!(made["action"], "cloneDeckConfigId");
    assert_eq!(made["params"]["name"], DECK_PRESET);
    // Cloned from Default, so a marked deck schedules exactly as an unmarked one would. The
    // preset is a label here, not an opinion about how to study.
    assert_eq!(made["params"]["cloneFrom"], 1);

    assert_eq!(stub.received()["params"]["deck"], "Sakatsu-Deck");

    let marked = stub.received();
    assert_eq!(marked["action"], "setDeckConfigId");
    assert_eq!(marked["params"]["configId"], 7);
    // **Without this the deck is one the app can never touch again**, so it is part of creating
    // a deck rather than a flourish afterwards.
    assert_eq!(marked["params"]["decks"][0], "Sakatsu-Deck");
}

#[test]
fn renaming_moves_the_cards_and_carries_the_mark_to_the_new_name() {
    let stub = Stub::start(vec![
        r#"{"result": ["日本語"], "error": null}"#.into(),
        OURS.into(),
        NOTHING.into(),
        OURS.into(),
        OK.into(),
        APPLIED.into(),
        r#"{"result": [1, 2], "error": null}"#.into(),
        OK.into(),
        NOTHING.into(),
        OK.into(),
    ]);

    stub.client()
        .rename_deck("日本語", "Japanese")
        .expect("renamed");

    assert_eq!(stub.received()["action"], "deckNames");
    assert_eq!(stub.received()["action"], "getDeckConfig");
    assert_eq!(stub.received()["action"], "findCards");
    assert_eq!(stub.received()["action"], "getDeckConfig");
    assert_eq!(stub.received()["params"]["deck"], "Japanese");

    // **The new name has to carry the mark.** A rename here is a create-and-move, so without
    // this the learner would find that renaming their deck is what made the app disown it.
    let marked = stub.received();
    assert_eq!(marked["action"], "setDeckConfigId");
    assert_eq!(marked["params"]["decks"][0], "Japanese");
    assert_eq!(marked["params"]["configId"], 7);

    let own = stub.received();
    // Its own cards, not the subtree's: taking the subtree here would land every child deck's
    // cards in the top deck and then delete the children as "empty".
    assert!(
        own["params"]["query"]
            .as_str()
            .expect("a query")
            .contains("-\"deck:日本語::*\""),
        "the move must exclude subdecks: {}",
        own["params"]["query"]
    );

    let moved = stub.received();
    assert_eq!(moved["action"], "changeDeck");
    assert_eq!(moved["params"]["deck"], "Japanese");

    assert_eq!(stub.received()["action"], "findCards");
    assert_eq!(stub.received()["action"], "deleteDecks");
}

#[test]
fn a_rename_that_did_not_move_everything_leaves_the_old_deck_alone() {
    let stub = Stub::start(vec![
        r#"{"result": ["日本語"], "error": null}"#.into(),
        OURS.into(),
        NOTHING.into(),
        OURS.into(),
        OK.into(),
        APPLIED.into(),
        r#"{"result": [1, 2], "error": null}"#.into(),
        OK.into(),
        // Anki still reports a card in the old deck. The move did not finish.
        r#"{"result": [2], "error": null}"#.into(),
        OK.into(),
    ]);

    let error = stub
        .client()
        .rename_deck("日本語", "Japanese")
        .expect_err("aborted");
    assert!(matches!(error, AnkiError::Refused(_)));

    for _ in 0..9 {
        stub.received();
    }
    // **The whole point of the check.** Deleting here would destroy the card that failed to
    // move, and the rename would report success while losing it.
    assert!(
        stub.try_received().is_none(),
        "the old deck was deleted with cards still in it"
    );
}

#[test]
fn duplicating_copies_the_container_and_never_the_cards() {
    let stub = Stub::start(vec![
        r#"{"result": ["A"], "error": null}"#.into(),
        OURS.into(),
        NOTHING.into(),
        OURS.into(),
        OK.into(),
        APPLIED.into(),
    ]);

    stub.client().duplicate_deck("A", "B").expect("duplicated");

    assert_eq!(stub.received()["action"], "deckNames");
    assert_eq!(stub.received()["action"], "getDeckConfig");
    assert_eq!(stub.received()["action"], "findCards");
    assert_eq!(stub.received()["action"], "getDeckConfig");
    assert_eq!(stub.received()["params"]["deck"], "B");

    // The copy wears our preset — which is also the source's, since only our decks can be
    // duplicated. So it genuinely has the source's settings, without a second preset that would
    // drift from the first.
    let marked = stub.received();
    assert_eq!(marked["action"], "setDeckConfigId");
    assert_eq!(marked["params"]["configId"], 7);

    // No `findCards` for contents, no `notesInfo`, no `addNote`. The cards stay where they are,
    // because re-adding them would mint new ones with none of their scheduling.
    assert!(stub.try_received().is_none(), "the cards were touched");
}

#[test]
fn a_deck_name_anki_cannot_address_is_refused_before_anything_is_sent() {
    for bad in ["", "   ", "A::", "::A", "A::::B", "say \"hi\""] {
        // Every entry point that takes a name, including the destructive one. The check runs
        // before the first request, which is what lets a test — or a mistyped argument — poke
        // these with rubbish while a real Anki is listening and have nothing leave the process.
        let attempts: [&DeckCall; 4] = [
            &|anki, name| anki.create_deck(name),
            &|anki, name| anki.duplicate_deck("A", name),
            &|anki, name| anki.rename_deck("A", name),
            &|anki, name| anki.delete_deck(name),
        ];

        for attempt in attempts {
            let stub = Stub::start(vec![r#"{"result": null, "error": null}"#.into()]);
            let error = attempt(&stub.client(), bad).expect_err("refused");
            assert!(
                matches!(error, AnkiError::BadDeckName(_)),
                "{bad:?} should be refused, got {error:?}"
            );
            assert!(
                stub.try_received().is_none(),
                "{bad:?} reached Anki instead of being refused here"
            );
        }
    }
}
