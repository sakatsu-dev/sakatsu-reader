//! Export to Anki through AnkiConnect (`project notes` §3).
//!
//! AnkiConnect is an add-on that exposes an HTTP endpoint on loopback. Every call is
//! a POST of `{action, version, params}` to the same URL, and every reply is
//! `{result, error}` — where `error` is a *string or null*, not an HTTP status. A 200
//! response carrying an error is the normal way it refuses something, so the status
//! code alone tells you nothing.
//!
//! ## One note type, kept in step
//!
//! 屠つ owns a single note type. That keeps the export predictable — a learner should not
//! have to map fields — and means cards made today still match cards made a year ago.
//!
//! **It now updates one that already exists, where it used to leave it alone.** The old policy
//! was that rewriting someone's note type could damage a collection built on it, which is true
//! and is the cost being accepted here: the alternative is that every improvement to the card
//! reaches only people who have never used the feature, which is nobody who would notice. What
//! it does is add fields it needs and rewrite the template and styling — see [`Anki::sync_note_type`],
//! which says plainly what that costs anyone who has hand-edited theirs.
//!
//! Field *contents* are never touched, so notes made before a change keep everything they had.

pub mod sentence;

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::dict::WordView;

/// Where AnkiConnect listens by default.
pub const DEFAULT_URL: &str = "http://127.0.0.1:8765";

/// The API generation this speaks.
const API_VERSION: u32 = 6;
const TIMEOUT: Duration = Duration::from_secs(5);
const BODY_LIMIT: u64 = 1024 * 1024;

/// The note type 屠つ creates and writes to.
pub const NOTE_TYPE: &str = "屠つ (Sakatsu)";

/// The fields of [`NOTE_TYPE`], in order.
///
/// Deliberately few. A note with fifteen fields is a note nobody reviews.
///
/// `Sentence` carries Anki's own furigana markup — `気持[きも]ち` — with the word the card is
/// about wrapped in `<b>`. One field serves every rendering because Anki's filters choose:
/// `{{furigana:Sentence}}` draws ruby, `{{kanji:Sentence}}` draws the text alone. Notes written
/// before that was true hold plain text, which passes through both filters unchanged.
pub const FIELDS: &[&str] = &[
    "Expression",
    "Reading",
    "Meaning",
    "Sentence",
    "Source",
    "PartOfSpeech",
];

/// How many senses a card is allowed to carry.
///
/// **A card you cannot grade is not a card.** Every inline sense used to reach the Meaning field,
/// so a common word arrived as eight glosses of near-synonyms — and there is no honest answer to
/// "did I know it" against a wall of them. Three is enough to fix a word's range without turning
/// the answer into a page to read.
///
/// The dictionary is not being truncated, only the flashcard: the reference page still shows every
/// sense, with the sources that add more behind their own disclosure. This is the oldest rule in
/// spaced repetition — keep what a single card asks small enough to answer.
const MEANINGS_ON_A_CARD: usize = 3;

/// The deck Anki always keeps.
///
/// **Never editable from here, whatever else is true of it.** Anki will not delete it, renaming it
/// is a special case in the scheduler, and a learner's collection always has one — so the app has
/// no business touching the deck every other deck falls back to.
const DEFAULT_DECK: &str = "Default";

/// The options preset 屠つ marks its own decks with.
///
/// **This is how a deck says it was made here, and it is stored in Anki rather than by us.**
/// The collection records nothing about what created a deck, and a list of names we remember
/// creating parts ways with reality the moment somebody renames one in Anki. A preset does not:
/// it is attached to the deck itself, it survives renaming, it survives reinstalling this app, and
/// it travels with an AnkiWeb sync.
///
/// It is also visible and reversible in Anki's own interface — a learner can see this name in the
/// deck options and detach it, which cleanly says "stop treating this as yours". Anki uses presets
/// the same way for its own `Custom Study Session`.
///
/// Named like [`NOTE_TYPE`] so the two read as one app's marks.
pub const DECK_PRESET: &str = "屠つ (Sakatsu)";

/// The options preset every deck starts on.
const DEFAULT_DECK_CONFIG: i64 = 1;

/// A deck, and what 屠つ is allowed to do with it.
///
/// The counts cover the deck **and everything filed under it**, because Anki takes a subtree
/// away with its root: a parent that looks empty can still be holding a learner's own subdeck,
/// and deleting the parent would take that with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckInfo {
    pub name: String,
    /// Cards in the subtree that this app wrote.
    pub ours: u64,
    /// Cards in the subtree that anything else wrote.
    pub foreign: u64,
    /// Whether the deck wears [`DECK_PRESET`] — that is, whether this app made it.
    pub made_here: bool,
}

impl DeckInfo {
    /// Whether 屠つ may rename or delete this deck.
    ///
    /// **Two conditions, and they answer different questions.** `made_here` asks whether this is
    /// ours to act on at all; `foreign == 0` asks whether acting on it would destroy anything.
    /// Neither implies the other, and dropping either one is a way to lose somebody's work:
    ///
    /// - Content alone would let the app delete any empty deck in the collection, including ones
    ///   Anki makes for itself, because an empty deck looks the same whoever made it.
    /// - The mark alone would let it delete a deck we created and the learner then filled with
    ///   their own cards. Making the deck does not make its contents ours.
    ///
    /// `Default` is excluded outright: Anki keeps it whatever happens, and it is the deck every
    /// other deck's cards fall back to.
    pub fn managed(&self) -> bool {
        self.made_here && self.foreign == 0 && self.name != DEFAULT_DECK
    }
}

/// Quote a value for Anki's search syntax.
///
/// `*` and `_` are wildcards **even inside quotes**, so a deck called `N_5` would otherwise
/// match `N5` and `Na5` as well as itself. `::` is deliberately left alone: it is how a subdeck
/// is addressed, not a character to hide.
fn escape_search(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '"' | '\\' | '*' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// A search matching this deck and everything under it.
fn deck_term(name: &str) -> String {
    format!("\"deck:{}\"", escape_search(name))
}

/// A search matching the cards this app wrote.
///
/// **The note type, not the `sakatsu` tag.** Both are stamped on every export, but only the
/// note type is structural — 屠つ is the only thing that creates it, and a note cannot be
/// casually detached from it. A tag is one keystroke to add, and it is also something a learner
/// might apply themselves, so trusting it would let a hand-tagged card make somebody's own deck
/// look like ours. That is precisely the mistake the gate exists to prevent, so the looser
/// signal does not get a vote.
fn ours_term() -> String {
    format!("\"note:{}\"", escape_search(NOTE_TYPE))
}

/// Reject a name Anki would refuse or quietly mangle.
fn check_deck_name(name: &str) -> Result<(), AnkiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AnkiError::BadDeckName("a deck needs a name".into()));
    }
    if name.contains('"') {
        return Err(AnkiError::BadDeckName(
            "a deck name cannot contain a quotation mark".into(),
        ));
    }
    // `::` nests one deck inside another. An empty part makes a deck Anki will show and then
    // cannot address, which is worse than refusing the name.
    if name.split("::").any(|part| part.trim().is_empty()) {
        return Err(AnkiError::BadDeckName(
            "a deck name cannot have an empty part between `::`".into(),
        ));
    }
    Ok(())
}

/// A deck and everything filed under it, parents before children.
///
/// Anki nests by name, so a subtree is a prefix match on `::` — and matching `root::` rather
/// than `root` is what keeps `Japanese2` out of `Japanese`'s subtree. Sorted, because that
/// order puts a parent before the children that need it to exist first.
fn subtree(names: &[String], root: &str) -> Vec<String> {
    let prefix = format!("{root}::");
    let mut found: Vec<String> = names
        .iter()
        .filter(|name| name.as_str() == root || name.starts_with(&prefix))
        .cloned()
        .collect();
    found.sort();
    found
}

/// Re-root a deck name from one subtree onto another.
fn rename_prefix(name: &str, from: &str, to: &str) -> String {
    match name.strip_prefix(from) {
        Some(rest) => format!("{to}{rest}"),
        None => name.to_string(),
    }
}

/// One card's worth of content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Note {
    /// The headword, as written.
    pub expression: String,
    /// Its reading in kana. Empty when nothing supplied one.
    pub reading: String,
    /// The senses, one per line.
    pub meaning: String,
    /// The sentence it was met in — the reason the word is worth learning.
    ///
    /// In Anki's furigana markup, with the word itself marked. See [`sentence::markup`].
    pub sentence: String,
    /// Which dictionary the meaning came from.
    ///
    /// On the card because both shipped dictionaries are CC BY-SA and attribution is a
    /// condition of using them, not a courtesy — and because a definition a learner cannot
    /// attribute is one they cannot check.
    pub source: String,
    /// How the word behaves — *noun*, *godan verb* — written out rather than coded.
    pub part_of_speech: String,
}

impl Note {
    /// Build a note from a looked-up word and the passage it came from.
    ///
    /// Only the senses that show inline are used. The "also in …" sources are a
    /// disclosure for reading, not material for a card: putting four dictionaries on
    /// one card is how a card becomes unreviewable.
    ///
    /// **A name counts when there are no senses.** Names never enter the sense list — see
    /// `dict::merge::SourceResult::definitions` — so a character's name would otherwise have
    /// an empty Meaning field and be refused as `NothingToAdd`, which is the wrong answer for
    /// exactly the words a reader of manga most wants to keep. It is only ever a fallback:
    /// a word that means something puts its meaning on the card.
    ///
    /// `source` is the dictionary that answered, resolved by the caller: `inline_source` names it
    /// when the definition was promoted from a secondary, and only the caller knows the primary's
    /// title otherwise.
    pub fn from_word(word: &WordView, sentence: &str, source: &str) -> Note {
        let mut meaning = word
            .inline
            .iter()
            .map(|sense| sense.glosses.join("; "))
            .filter(|line| !line.trim().is_empty())
            .take(MEANINGS_ON_A_CARD)
            .collect::<Vec<_>>()
            .join("\n");
        if meaning.trim().is_empty() {
            if let Some(name) = &word.name {
                meaning = name.glosses.join("; ");
            }
        }

        Note {
            expression: word.headword.clone(),
            reading: word.reading.clone().unwrap_or_default(),
            meaning,
            sentence: sentence.trim().to_string(),
            source: source.trim().to_string(),
            part_of_speech: part_of_speech(word),
        }
    }

    /// Whether this note is worth sending. A card with no meaning teaches nothing.
    pub fn is_usable(&self) -> bool {
        !self.expression.trim().is_empty() && !self.meaning.trim().is_empty()
    }

    fn fields(&self) -> Value {
        json!({
            "Expression": self.expression,
            "Reading": self.reading,
            "Meaning": self.meaning,
            "Sentence": self.sentence,
            "Source": self.source,
            "PartOfSpeech": self.part_of_speech,
        })
    }
}

/// How the word behaves, written out.
///
/// **The first sense's tags, not every sense's.** A word with six senses often carries a
/// different set on each, and a card headed *noun, godan verb, transitive, intransitive,
/// expression* has said nothing — the reference page hoists only the tags every sense shares, for
/// the same reason. The first sense is the one whose meaning leads the card, so its tags are the
/// ones that describe what the card is teaching.
///
/// Expanded through [`crate::terms`], which is compiled into the binary, so this works on a
/// machine with no dictionary installed. A code the table does not know is kept as it is: an
/// unexplained code is still information, and it is the dictionary's own word for something.
fn part_of_speech(word: &WordView) -> String {
    let Some(sense) = word.inline.first() else {
        return String::new();
    };
    let mut written: Vec<String> = Vec::new();
    for code in &sense.tags {
        let label = crate::terms::get(code)
            .map(|term| term.label.to_lowercase())
            .unwrap_or_else(|| code.clone());
        if !label.is_empty() && !written.contains(&label) {
            written.push(label);
        }
    }
    written.join(", ")
}

/// The endpoints [`Anki::sync_note_type`] has already run to completion against.
///
/// **Keyed by URL rather than a bare flag, and not a field on [`Anki`].** A field would never be
/// read: `Anki::default()` is built fresh for every call, so it would be false every time. A
/// single global would be worse than it looks — two collections reached on different ports share
/// nothing, and in the test suite every stub runs in one process, so the first sync would silence
/// every test after it and they would pass by not doing anything.
static SYNCED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn synced() -> &'static Mutex<HashSet<String>> {
    SYNCED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The one card this note type makes.
///
/// Recognition only, which is what a reader needs and what keeps the review count to one per
/// word. A production card asks a different skill and doubles the load for it.
const CARD_NAME: &str = "Recognition";

/// The word, then the sentence it was met in — **and no readings**.
///
/// Kaishi's hierarchy — the word is the subject, the sentence is the evidence — over the
/// sentence-first framing the grammar decks use. Both agree on the part that matters: a card
/// carrying only a word teaches a definition, and a card carrying only a sentence buries what it
/// is asking about.
///
/// **`kanji:` here and `furigana:` on the back, and that asymmetry is the point.** This side used
/// to render the ruby, which printed the reading above the word every time the card came up — the
/// one thing a reader most needs to recall, given away on the question side. A card cannot test
/// what it is already showing you. `{{kanji:Sentence}}` is Anki's own filter for the same field
/// with the readings stripped, so the sentence reads exactly as it did on the page.
///
/// Both filters are **built into Anki**, not the Japanese Support add-on. Only *generating*
/// `気持[きも]ち` markup needs one, and [`sentence::markup`] does that here — which matters,
/// because these notes are written for people who have never installed an add-on. A note written
/// before the field carried markup has no brackets in it and passes through either filter
/// unchanged.
const FRONT: &str = "\
<div class=\"word\">{{Expression}}</div>
{{#Sentence}}<div class=\"sentence\">{{kanji:Sentence}}</div>{{/Sentence}}";

/// The word answered, then the sentence given back with its readings.
///
/// **Written out rather than `{{FrontSide}}`, and it has to be.** Reusing the front would print
/// the sentence twice — once bare from the question, once with ruby — and there is no way to
/// suppress the first. Writing the side out is also what lets it choose its own order: the word is
/// what the card asked, so the word is answered first and the sentence follows as evidence.
///
/// `<hr id=answer>` sits after the restated word. Above it is the question echoed back; below it
/// is everything that was hidden, and Anki scrolls to that id on long cards.
///
/// The conditionals matter: a word with no reading, no tags or no named source must not leave an
/// empty row behind it. Anki's `{{#Field}}…{{/Field}}` is the whole mechanism.
const BACK: &str = "\
<div class=\"word\">{{Expression}}</div>
<hr id=answer>
{{#Reading}}<div class=\"reading\">{{Reading}}</div>{{/Reading}}
{{#PartOfSpeech}}<div class=\"pos\">{{PartOfSpeech}}</div>{{/PartOfSpeech}}
<div class=\"meaning\">{{Meaning}}</div>
{{#Sentence}}<div class=\"sentence given\">{{furigana:Sentence}}</div>{{/Sentence}}
{{#Source}}<div class=\"source\">{{Source}}</div>{{/Source}}";

/// The card's styling.
///
/// Self-contained, because an Anki card cannot see `theme.css` — none of the app's tokens exist
/// here and every value has to be written out.
///
/// **Night mode is half the work.** Anki puts `.nightMode` on the card element or an ancestor
/// depending on version and platform, so both forms are written; a card that only works on a
/// white background is half a card, and this app is dark.
///
/// `white-space: pre-line` on the meaning is not decoration. `Note::from_word` joins the senses
/// with newlines, and HTML collapses those to spaces — so every sense of a six-sense word ran
/// together into one paragraph on the old card. The senses are a list and have to read as one.
const STYLING: &str = "\
.card {
  font-family: -apple-system, \"Segoe UI\", \"Hiragino Sans\", \"Noto Sans JP\", sans-serif;
  font-size: 20px;
  line-height: 1.7;
  text-align: center;
  padding: 1.4em 0.9em;
  color: #1b1820;
  background: #fbfafc;
}
.nightMode.card,
.nightMode .card {
  color: #e7e1ea;
  background: #16141a;
}

.word {
  font-size: 2.1em;
  font-weight: 600;
  line-height: 1.25;
}

.sentence {
  margin-top: 0.9em;
  font-size: 0.95em;
  color: #56505e;
  /* **Room for ruby that is not there yet.** The question side prints the sentence bare and the
     answer side prints it with readings above, so the block is taller on one side than the other
     and everything under it moved when the card turned — including the buttons being aimed at.
     Reserving the annotation's height on both sides costs a little space above the sentence and
     buys an answer that appears where the question was. */
  line-height: 2.1;
}
.nightMode .sentence,
.nightMode.card .sentence {
  color: #a49dad;
}
/* The word the card is about, inside the sentence it came from. Colour rather than weight: the
   sentence is already small, and bolding a run of kanji at this size thickens it into a blob. */
.sentence b {
  font-weight: 600;
  color: #6d4bb8;
}
.nightMode .sentence b,
.nightMode.card .sentence b {
  color: #a683e8;
}
/* Furigana, quiet enough to ignore when you do not need it.
 *
 * **Only on the answer.** The question side renders through `{{kanji:}}` and has no ruby at all,
 * which is the point of the design rather than a styling choice — see `FRONT`. Anki tags what it
 * renders with a `furigana` class of its own, which is a steadier hook than the element. */
.sentence ruby rt,
.sentence .furigana rt {
  font-size: 0.55em;
  opacity: 0.75;
  font-weight: 400;
}
/* The sentence on the answer, below the meaning rather than under the word. Set apart, because by
   then it is evidence for an answer already given rather than the question. */
.sentence.given {
  margin-top: 1.3em;
}

hr#answer {
  margin: 1.2em auto;
  border: 0;
  border-top: 1px solid #d9d4de;
  max-width: 24em;
}
.nightMode hr#answer,
.nightMode.card hr#answer {
  border-top-color: #2c2734;
}

.reading {
  font-size: 1.15em;
  color: #56505e;
}
.nightMode .reading,
.nightMode.card .reading {
  color: #a49dad;
}

/* Small, uppercase, spaced — a label on the answer rather than part of it. */
.pos {
  margin-top: 0.5em;
  font-size: 0.62em;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: #837c8e;
}

.meaning {
  margin-top: 0.9em;
  font-size: 1.05em;
  /* The senses arrive newline-separated and HTML would run them together. */
  white-space: pre-line;
}

/* Attribution, and a licence condition rather than a courtesy: both shipped dictionaries are
   CC BY-SA. Quiet, because it is not what you are being asked. */
.source {
  margin-top: 1.4em;
  font-size: 0.6em;
  letter-spacing: 0.06em;
  color: #9a93a3;
}
";

/// Why an export did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnkiError {
    /// Anki is not running, or AnkiConnect is not installed.
    Unreachable(String),
    /// Anki answered, and said no. Includes the duplicate case.
    Refused(String),
    /// Anki answered with something unexpected.
    BadResponse(String),
    /// The note has nothing worth sending.
    NothingToAdd,
    /// The deck holds cards this app did not write, so it will not be changed.
    NotManaged(String),
    /// No deck by that name.
    NoSuchDeck(String),
    /// A deck by that name is already there.
    DeckExists(String),
    /// A name Anki would refuse or mangle.
    BadDeckName(String),
}

impl std::fmt::Display for AnkiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnkiError::Unreachable(message) => write!(
                f,
                "Anki is not reachable — is it running with AnkiConnect installed? ({message})"
            ),
            AnkiError::Refused(message) => write!(f, "Anki refused: {message}"),
            AnkiError::BadResponse(message) => write!(f, "unexpected reply from Anki: {message}"),
            AnkiError::NothingToAdd => write!(f, "nothing to add: this word has no definition"),
            AnkiError::NotManaged(deck) => write!(
                f,
                "{deck} holds cards 屠つ did not make — change it in Anki instead"
            ),
            AnkiError::NoSuchDeck(deck) => write!(f, "Anki has no deck called {deck}"),
            AnkiError::DeckExists(deck) => write!(f, "Anki already has a deck called {deck}"),
            AnkiError::BadDeckName(why) => write!(f, "that name will not work: {why}"),
        }
    }
}

impl std::error::Error for AnkiError {}

/// A client for a running Anki.
pub struct Anki {
    url: String,
    agent: ureq::Agent,
    /// [`DECK_PRESET`]'s id once found, so it is looked up once a session rather than per deck.
    preset: Mutex<Option<i64>>,
}

impl std::fmt::Debug for Anki {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Anki").field("url", &self.url).finish()
    }
}

impl Default for Anki {
    fn default() -> Self {
        Anki::new(DEFAULT_URL)
    }
}

impl Anki {
    pub fn new(url: impl Into<String>) -> Anki {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        Anki {
            url: url.into(),
            agent,
            preset: Mutex::new(None),
        }
    }

    /// Whether Anki is there, and which API generation it speaks.
    pub fn version(&self) -> Result<u32, AnkiError> {
        let result = self.call("version", Value::Null)?;
        result
            .as_u64()
            .map(|version| version as u32)
            .ok_or_else(|| AnkiError::BadResponse(format!("version was {result}")))
    }

    /// The decks the collection has, so the UI can offer them rather than guess.
    pub fn deck_names(&self) -> Result<Vec<String>, AnkiError> {
        let result = self.call("deckNames", Value::Null)?;
        serde_json::from_value(result)
            .map_err(|error| AnkiError::BadResponse(format!("deckNames: {error}")))
    }

    /// The id of [`DECK_PRESET`], making it if the collection has not got one.
    ///
    /// Found by name rather than remembered by id, because an id we stored could outlive the
    /// preset a learner deleted in Anki. The scan costs one call per deck and runs only when a
    /// deck is being made, so it is cached for the session afterwards.
    fn our_preset(&self, names: &[String]) -> Result<i64, AnkiError> {
        if let Some(id) = *self
            .preset
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            return Ok(id);
        }

        let mut found = None;
        for name in names {
            let config = self.call("getDeckConfig", json!({ "deck": name }))?;
            if config.get("name").and_then(Value::as_str) == Some(DECK_PRESET) {
                found = config.get("id").and_then(Value::as_i64);
                break;
            }
        }

        let id = match found {
            Some(id) => id,
            None => {
                // Cloned from Default so a new deck schedules exactly as it would have without
                // this mark. The preset is a label here, not an opinion about studying.
                let made = self.call(
                    "cloneDeckConfigId",
                    json!({ "name": DECK_PRESET, "cloneFrom": DEFAULT_DECK_CONFIG }),
                )?;
                made.as_i64().ok_or_else(|| {
                    AnkiError::BadResponse(format!("cloneDeckConfigId returned {made}"))
                })?
            }
        };

        *self
            .preset
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(id);
        Ok(id)
    }

    /// Whether a deck wears our preset.
    fn made_here(&self, deck: &str) -> Result<bool, AnkiError> {
        let config = self.call("getDeckConfig", json!({ "deck": deck }))?;
        Ok(config.get("name").and_then(Value::as_str) == Some(DECK_PRESET))
    }

    /// Every deck, with the counts that decide what may be done to it.
    ///
    /// Two searches a deck rather than one pass over the collection: `findCards` answers with
    /// bare IDs, which stays inside [`BODY_LIMIT`] where `cardsInfo` over a real collection
    /// would not. They are loopback calls against a local add-on, so the count of them matters
    /// much less than the size of what comes back.
    pub fn decks(&self) -> Result<Vec<DeckInfo>, AnkiError> {
        let names = self.deck_names()?;
        let mut decks = Vec::with_capacity(names.len());
        for name in names {
            let scope = deck_term(&name);
            let ours = self.count(&format!("{scope} {}", ours_term()))?;
            let foreign = self.count(&format!("{scope} -{}", ours_term()))?;
            // Asked for every deck, including ones no action could reach.
            //
            // It was skipped where the answer could not change what the *manage* buttons allow,
            // which was true and became false the moment the export list started reading this
            // too: a field named `made_here` that quietly reports `false` for decks it never
            // asked about is worse than the call it saves. One extra loopback request a deck, on
            // a read that happens when the panel opens and every fifteen seconds while it is
            // watching.
            let made_here = self.made_here(&name)?;
            decks.push(DeckInfo {
                name,
                ours,
                foreign,
                made_here,
            });
        }
        Ok(decks)
    }

    /// How many cards a search matches.
    fn count(&self, query: &str) -> Result<u64, AnkiError> {
        let result = self.call("findCards", json!({ "query": query }))?;
        result
            .as_array()
            .map(|cards| cards.len() as u64)
            .ok_or_else(|| AnkiError::BadResponse(format!("findCards returned {result}")))
    }

    /// Refuse unless this deck is one 屠つ made **and** holds nothing it did not write.
    ///
    /// Both halves, for the reasons on [`DeckInfo::managed`]. Enforced here rather than left to
    /// the interface, so a second caller cannot forget it.
    fn require_managed(&self, deck: &str) -> Result<(), AnkiError> {
        if deck == DEFAULT_DECK {
            return Err(AnkiError::Refused(
                "Anki always keeps a deck called Default, so it is never changed from here".into(),
            ));
        }
        if !self.made_here(deck)? {
            return Err(AnkiError::NotManaged(deck.to_string()));
        }
        let foreign = self.count(&format!("{} -{}", deck_term(deck), ours_term()))?;
        if foreign == 0 {
            Ok(())
        } else {
            Err(AnkiError::NotManaged(deck.to_string()))
        }
    }

    /// Make a deck and mark it as ours. Anki makes any missing parents on the way.
    ///
    /// **The mark is the point, not a side effect.** A deck without it is one this app will not
    /// touch again, so failing to apply it would quietly create a deck the learner cannot rename
    /// or delete from here — which is why the preset failing is an error rather than a shrug.
    fn make_deck(&self, name: &str, preset: i64) -> Result<(), AnkiError> {
        self.call("createDeck", json!({ "deck": name }))?;
        self.call(
            "setDeckConfigId",
            json!({ "decks": [name], "configId": preset }),
        )?;
        Ok(())
    }

    /// Create an empty deck. Anki makes any missing parents on the way.
    pub fn create_deck(&self, name: &str) -> Result<(), AnkiError> {
        check_deck_name(name)?;
        let name = name.trim();
        let names = self.deck_names()?;
        if names.iter().any(|deck| deck == name) {
            return Err(AnkiError::DeckExists(name.to_string()));
        }
        let preset = self.our_preset(&names)?;
        self.make_deck(name, preset)
    }

    /// Copy a deck's structure and its options to a new name.
    ///
    /// **The cards do not come, and that is not an omission.** AnkiConnect can only re-add
    /// notes, which mints new cards carrying none of their scheduling — a deck that looks
    /// identical and has forgotten everything the learner taught it — while leaving the
    /// collection holding two notes per word for Anki's own duplicate check to complain about.
    /// Copying the container is something this can do honestly. Copying the study history is
    /// not, and Anki's own `.apkg` export is where that belongs.
    ///
    /// Refused unless the source is one of ours, like every other deck action.
    pub fn duplicate_deck(&self, source: &str, target: &str) -> Result<(), AnkiError> {
        check_deck_name(target)?;
        let target = target.trim();
        let names = self.deck_names()?;
        if !names.iter().any(|name| name == source) {
            return Err(AnkiError::NoSuchDeck(source.to_string()));
        }
        if names.iter().any(|name| name == target) {
            return Err(AnkiError::DeckExists(target.to_string()));
        }
        self.require_managed(source)?;

        // Every copy is one of ours and wears our preset — which is also the preset the source
        // wears, since only our decks can be duplicated now. So the copy genuinely has the
        // source's settings without cloning a second preset that would drift from the first.
        let preset = self.our_preset(&names)?;
        for deck in subtree(&names, source) {
            self.make_deck(&rename_prefix(&deck, source, target), preset)?;
        }
        Ok(())
    }

    /// Rename a deck, with its subdecks and every card in them.
    ///
    /// **AnkiConnect has no rename.** This is three steps — create the new names, move the
    /// cards across, remove the old names — and the middle one is where a half-finish would
    /// hurt. So the old decks are removed only after Anki itself reports nothing left in them.
    /// A failure anywhere leaves the cards where they already were, and at worst an empty deck
    /// behind: tidying up, rather than hunting for cards.
    pub fn rename_deck(&self, from: &str, to: &str) -> Result<(), AnkiError> {
        check_deck_name(to)?;
        let to = to.trim();
        if from == to {
            return Ok(());
        }
        let names = self.deck_names()?;
        if !names.iter().any(|name| name == from) {
            return Err(AnkiError::NoSuchDeck(from.to_string()));
        }
        if names.iter().any(|name| name == to) {
            return Err(AnkiError::DeckExists(to.to_string()));
        }
        self.require_managed(from)?;

        // **The new names have to carry the mark too.** A rename here is really a create-and-move,
        // so a deck made with `createDeck` alone would arrive on Anki's Default preset — and the
        // learner would find that renaming their deck was what made this app stop recognising it.
        let preset = self.our_preset(&names)?;
        let moving = subtree(&names, from);
        for deck in &moving {
            let target = rename_prefix(deck, from, to);
            self.make_deck(&target, preset)?;

            // This deck's own cards, not its subdecks' — those move on their own turn, and
            // taking them here would land the whole subtree in the top deck. The `*` is a
            // wildcard on purpose, so it is appended rather than escaped.
            let own = format!("{} -\"deck:{}::*\"", deck_term(deck), escape_search(deck));
            let cards = self.call("findCards", json!({ "query": own }))?;
            let ids = cards.as_array().cloned().unwrap_or_default();
            if !ids.is_empty() {
                self.call("changeDeck", json!({ "cards": ids, "deck": target }))?;
            }
        }

        // Only now, and only because Anki says so rather than because the moves returned Ok.
        let left = self.count(&deck_term(from))?;
        if left != 0 {
            return Err(AnkiError::Refused(format!(
                "{left} cards are still in {from}, so it has been left in place"
            )));
        }
        self.call("deleteDecks", json!({ "decks": moving, "cardsToo": true }))?;
        Ok(())
    }

    /// Delete a deck, its subdecks, and the cards in them.
    ///
    /// Gated on [`DeckInfo::managed`]: this destroys cards, and the only cards it may destroy
    /// are ones that exporting again can put back.
    pub fn delete_deck(&self, name: &str) -> Result<(), AnkiError> {
        // Checked before anything is sent. A name Anki cannot address is not a deck anyone has,
        // so there is nothing to gain by asking — and stopping here keeps the destructive path
        // entirely inside this process for the inputs that were never going to work.
        check_deck_name(name)?;
        let names = self.deck_names()?;
        if !names.iter().any(|deck| deck == name) {
            return Err(AnkiError::NoSuchDeck(name.to_string()));
        }
        self.require_managed(name)?;

        // `cardsToo` is true because the gate above has already established that every card in
        // here is one of ours. The alternative — false — does not spare anything; it tips them
        // into Default, where they are somebody's problem later.
        let going = subtree(&names, name);
        self.call("deleteDecks", json!({ "decks": going, "cardsToo": true }))?;
        Ok(())
    }

    /// Add a note, creating 屠つ's note type first if the collection lacks it.
    pub fn add(&self, deck: &str, note: &Note) -> Result<i64, AnkiError> {
        // **The deck is checked before anything is sent.** The interface will not offer an export
        // without a deck chosen, but this is the boundary and an empty name reaching AnkiConnect
        // is a card filed somewhere nobody asked for. The same check every deck command runs.
        check_deck_name(deck)?;
        if !note.is_usable() {
            return Err(AnkiError::NothingToAdd);
        }
        self.sync_note_type()?;

        let result = self.call(
            "addNote",
            json!({
                "note": {
                    "deckName": deck,
                    "modelName": NOTE_TYPE,
                    "fields": note.fields(),
                    "tags": ["sakatsu"],
                    "options": {
                        // Anki's own duplicate check, scoped to this note type. Adding
                        // the same word twice is the most likely mistake, and it is
                        // better refused than silently duplicated.
                        "allowDuplicate": false,
                        "duplicateScope": "deck",
                    }
                }
            }),
        )?;

        result
            .as_i64()
            .ok_or_else(|| AnkiError::BadResponse(format!("addNote returned {result}")))
    }

    /// Bring the collection's note type up to what this version of the app writes.
    ///
    /// Absent, it is created. Present, it is **brought into step**: fields this version needs are
    /// added, and the template and styling are rewritten.
    ///
    /// # What this costs, said plainly
    ///
    /// **It overwrites a template the learner has edited by hand.** That is a real loss and there
    /// is no way to have both — the alternative, which this replaced, was to leave an existing note
    /// type untouched, and its cost was worse: every improvement to the card reached only people
    /// who had never used the feature. Anyone with a card in their collection was frozen on the
    /// first design forever, including the person asking for the change.
    ///
    /// Two things bound the damage. Field *contents* are never touched, so no note loses anything
    /// it holds. And fields are only ever **added** — `modelFieldRemove` is not called here and
    /// should not be, because removing one deletes its content from every note that has it.
    ///
    /// # Once per run
    ///
    /// A note type cannot drift while the app is running, so doing this on every add is three
    /// round trips per card for an answer that cannot have changed. The flag is set only after a
    /// success: a sync that failed because Anki was closed must be retried when it opens.
    fn sync_note_type(&self) -> Result<(), AnkiError> {
        // A poisoned lock is not a reason to refuse an export: the worst it costs is a sync
        // that runs twice.
        let done = |set: &HashSet<String>| set.contains(&self.url);
        if done(&synced().lock().unwrap_or_else(|held| held.into_inner())) {
            return Ok(());
        }

        let existing: Vec<String> =
            serde_json::from_value(self.call("modelNames", Value::Null)?)
                .map_err(|error| AnkiError::BadResponse(format!("modelNames: {error}")))?;

        if existing.iter().any(|name| name == NOTE_TYPE) {
            self.update_note_type()?;
        } else {
            self.call(
                "createModel",
                json!({
                    "modelName": NOTE_TYPE,
                    "inOrderFields": FIELDS,
                    "css": STYLING,
                    "cardTemplates": [{
                        "Name": CARD_NAME,
                        "Front": FRONT,
                        "Back": BACK,
                    }],
                }),
            )?;
        }

        synced()
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .insert(self.url.clone());
        Ok(())
    }

    /// Add whatever fields are missing, then rewrite the template and styling.
    ///
    /// The field pass comes first and is not optional: `updateModelTemplates` will happily install
    /// a template referring to `{{Source}}` on a note type that has no Source field, and what
    /// arrives on the card is the literal text rather than an error.
    fn update_note_type(&self) -> Result<(), AnkiError> {
        let present: Vec<String> = serde_json::from_value(
            self.call("modelFieldNames", json!({ "modelName": NOTE_TYPE }))?,
        )
        .map_err(|error| AnkiError::BadResponse(format!("modelFieldNames: {error}")))?;

        for field in FIELDS {
            if !present.iter().any(|name| name == field) {
                self.call(
                    "modelFieldAdd",
                    json!({ "modelName": NOTE_TYPE, "fieldName": field }),
                )?;
            }
        }

        self.call(
            "updateModelTemplates",
            json!({
                "model": {
                    "name": NOTE_TYPE,
                    "templates": { CARD_NAME: { "Front": FRONT, "Back": BACK } },
                }
            }),
        )?;
        self.call(
            "updateModelStyling",
            json!({ "model": { "name": NOTE_TYPE, "css": STYLING } }),
        )?;
        Ok(())
    }

    /// One AnkiConnect call.
    ///
    /// **`params` is an object or it is absent — never `null`.** AnkiConnect validates the
    /// request against a JSON schema that types `params` as an object, so a parameterless
    /// action sent as `"params": null` is refused outright:
    ///
    /// ```text
    /// None is not of type 'object'
    /// Failed validating 'type' in schema['properties']['params']
    /// ```
    ///
    /// Every call that takes no parameters went out that way — `version`, `deckNames`,
    /// `modelNames` — so the whole feature was dark against a current AnkiConnect. It failed
    /// *silently*, which is why it survived: `deckNames` is the liveness probe, the interface
    /// reads a failed probe as "Anki is not running", and a reader who has Anki open in front
    /// of them sees no button and no error to explain its absence.
    ///
    /// The stub tests could not have caught it. A stub answers whatever it was told to
    /// whether or not the request made sense, so the one thing that mattered here — what a
    /// real AnkiConnect does with this body — was the one thing they could not report.
    fn call(&self, action: &str, params: Value) -> Result<Value, AnkiError> {
        let params = match params {
            Value::Null => json!({}),
            given => given,
        };
        let body = json!({ "action": action, "version": API_VERSION, "params": params });

        let response = self
            .agent
            .post(&self.url)
            .send_json(&body)
            .map_err(|error| AnkiError::Unreachable(error.to_string()))?
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_string()
            .map_err(|error| AnkiError::BadResponse(error.to_string()))?;

        parse_reply(&response)
    }
}

/// Interpret AnkiConnect's `{result, error}` envelope.
///
/// The `error` field is where refusals live — a duplicate note comes back as HTTP 200
/// with an error string, so treating a 200 as success would silently drop it.
fn parse_reply(body: &str) -> Result<Value, AnkiError> {
    let reply: Value = serde_json::from_str(body)
        .map_err(|error| AnkiError::BadResponse(format!("{error}: {body}")))?;

    match reply.get("error") {
        Some(Value::String(message)) if !message.is_empty() => {
            Err(AnkiError::Refused(message.clone()))
        }
        _ => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::merge::{AlsoIn, Retrieved, Sense};

    fn word() -> WordView {
        WordView {
            headword: "食べる".into(),
            reading: Some("たべる".into()),
            inline: vec![
                Sense {
                    tags: vec!["v1".into(), "vt".into()],
                    glosses: vec!["to eat".into(), "to consume".into()],
                    sequence: None,
                },
                Sense {
                    tags: vec![],
                    glosses: vec!["to live on".into()],
                    sequence: None,
                },
            ],
            inline_source: None,
            inline_retrieved: Retrieved::Unknown,
            also_in: vec![AlsoIn {
                title: "Another Dict".into(),
                retrieved: Retrieved::Unknown,
                senses: vec![Sense {
                    tags: vec![],
                    glosses: vec!["to have a meal".into()],
                    sequence: None,
                }],
            }],
            examples: Vec::new(),
            name: None,
            likely_name: false,
            romaji: None,
        }
    }

    #[test]
    fn a_note_carries_the_word_its_reading_and_its_sentence() {
        let note = Note::from_word(&word(), "  ご飯を食べる。  ", "Jitendex");
        assert_eq!(note.expression, "食べる");
        assert_eq!(note.reading, "たべる");
        assert_eq!(note.meaning, "to eat; to consume\nto live on");
        assert_eq!(note.sentence, "ご飯を食べる。");
        assert!(note.is_usable());
    }

    #[test]
    fn only_the_inline_senses_reach_the_card() {
        // The "also in …" sources are a disclosure for reading, not card material.
        let note = Note::from_word(&word(), "", "Jitendex");
        assert!(
            !note.meaning.contains("to have a meal"),
            "a disclosed source leaked onto the card: {:?}",
            note.meaning
        );
    }

    #[test]
    fn a_word_with_no_definition_is_not_worth_a_card() {
        let mut empty = word();
        empty.inline.clear();
        let note = Note::from_word(&empty, "文", "Jitendex");
        assert!(!note.is_usable());
        assert_eq!(
            Anki::new("http://127.0.0.1:1").add("Default", &note),
            Err(AnkiError::NothingToAdd),
            "it should refuse before touching the network"
        );
    }

    /// A name is a card's meaning when nothing else is.
    ///
    /// Names are kept out of the sense list entirely, so without this the one kind of word a
    /// manga reader most often wants to keep — the character being addressed — makes an
    /// unusable note.
    #[test]
    fn a_name_is_worth_a_card_when_there_are_no_senses() {
        let mut named = word();
        named.inline.clear();
        named.headword = "セイア".into();
        named.name = Some(crate::dict::merge::NameHit {
            glosses: vec!["Seia".into()],
            tags: vec!["fem".into()],
            source: "JMnedict".into(),
            retrieved: Retrieved::Unknown,
            leads: true,
        });

        let note = Note::from_word(&named, "セイアちゃーん", "JMnedict");
        assert_eq!(note.meaning, "Seia");
        assert!(note.is_usable());

        // And it stays a fallback: a word that means something puts its meaning on the card.
        let mut both = word();
        both.name = named.name.clone();
        assert_eq!(
            Note::from_word(&both, "", "Jitendex").meaning,
            "to eat; to consume\nto live on"
        );
    }

    /// Codes are for dictionaries; a card is read by a person.
    #[test]
    fn the_part_of_speech_is_written_out_rather_than_coded() {
        let note = Note::from_word(&word(), "", "Jitendex");
        assert!(
            !note.part_of_speech.contains("v1") && !note.part_of_speech.contains("vt"),
            "raw codes reached the card: {:?}",
            note.part_of_speech
        );
        assert!(
            note.part_of_speech.contains("verb"),
            "nothing was expanded: {:?}",
            note.part_of_speech
        );
    }

    /// The first sense's tags, not the union of every sense's.
    ///
    /// 食べる's second sense carries none at all, and a card headed with everything six senses
    /// between them happen to be tagged says nothing about any of them.
    #[test]
    fn only_the_leading_senses_tags_describe_the_card() {
        let mut many = word();
        many.inline.push(Sense {
            tags: vec!["exp".into(), "adv".into()],
            glosses: vec!["a later sense".into()],
            sequence: None,
        });
        let note = Note::from_word(&many, "", "Jitendex");
        assert!(
            !note.part_of_speech.contains("adverb"),
            "a later sense's tags came too: {:?}",
            note.part_of_speech
        );
    }

    #[test]
    fn a_word_with_no_senses_claims_no_part_of_speech() {
        let mut named = word();
        named.inline.clear();
        named.name = Some(crate::dict::merge::NameHit {
            glosses: vec!["Seia".into()],
            tags: vec!["fem".into()],
            source: "JMnedict".into(),
            retrieved: Retrieved::Unknown,
            leads: true,
        });
        assert_eq!(Note::from_word(&named, "", "JMnedict").part_of_speech, "");
    }

    /// **The question side must not print the reading it is asking for.**
    ///
    /// The card rendered ruby on the front for a while, which put the reading above the word every
    /// time it came up — a card cannot test what it is already showing. The two filters are the
    /// whole of the fix and they are one word apart, so this pins which side each is on.
    #[test]
    fn the_front_hides_the_reading_and_the_back_gives_it() {
        assert!(
            FRONT.contains("{{kanji:Sentence}}") && !FRONT.contains("{{furigana:"),
            "the front is giving the reading away: {FRONT}"
        );
        assert!(
            BACK.contains("{{furigana:Sentence}}") && !BACK.contains("{{kanji:"),
            "the answer never shows the reading: {BACK}"
        );
    }

    /// **`{{FrontSide}}` would print the sentence twice**, once bare from the question and once
    /// with ruby, and nothing in a template can suppress the first. Writing the back out is also
    /// what lets it answer word-first instead of inheriting the front's order — so this one line
    /// guards the whole arrangement.
    #[test]
    fn the_back_is_written_out_rather_than_reusing_the_front() {
        assert!(
            !BACK.contains("FrontSide"),
            "the sentence will appear twice on the answer: {BACK}"
        );
        // Renderings, not mentions: `{{#Sentence}}…{{/Sentence}}` wraps the one that draws it, and
        // counting those too would make this assert three and mean nothing.
        assert_eq!(
            BACK.matches("{{furigana:Sentence}}").count(),
            1,
            "the sentence is drawn on the answer more than once: {BACK}"
        );
    }

    /// A card carrying every sense of a common word cannot be graded honestly.
    #[test]
    fn a_card_takes_a_few_senses_rather_than_all_of_them() {
        let mut many = word();
        many.inline = (0..8)
            .map(|n| Sense {
                tags: vec![],
                glosses: vec![format!("sense {n}")],
                sequence: None,
            })
            .collect();
        let note = Note::from_word(&many, "", "Jitendex");
        assert_eq!(
            note.meaning.lines().count(),
            MEANINGS_ON_A_CARD,
            "the whole dictionary reached the card: {:?}",
            note.meaning
        );
        // And it is the leading senses that survive, not an arbitrary few.
        assert!(note.meaning.starts_with("sense 0"), "{:?}", note.meaning);
    }

    /// Which dictionary answered is on the card because both shipped ones are CC BY-SA and
    /// attribution is a condition of the licence, not a courtesy.
    #[test]
    fn the_dictionary_that_answered_reaches_the_card() {
        assert_eq!(
            Note::from_word(&word(), "", "  Jitendex  ").source,
            "Jitendex"
        );
    }

    #[test]
    fn a_missing_reading_is_empty_rather_than_absent() {
        let mut nameless = word();
        nameless.reading = None;
        assert_eq!(Note::from_word(&nameless, "", "Jitendex").reading, "");
    }

    #[test]
    fn an_error_in_the_envelope_is_a_refusal_even_though_the_call_succeeded() {
        // How AnkiConnect reports a duplicate: HTTP 200, with the error in the body.
        let duplicate =
            r#"{"result": null, "error": "cannot create note because it is a duplicate"}"#;
        assert_eq!(
            parse_reply(duplicate),
            Err(AnkiError::Refused(
                "cannot create note because it is a duplicate".into()
            ))
        );

        // A null error is success.
        assert_eq!(
            parse_reply(r#"{"result": 1496198395707, "error": null}"#),
            Ok(json!(1496198395707i64))
        );
        // So is an empty-string error, which older versions send.
        assert_eq!(parse_reply(r#"{"result": 6, "error": ""}"#), Ok(json!(6)));
    }

    #[test]
    fn a_reply_that_is_not_json_is_reported_with_what_arrived() {
        let error = parse_reply("<html>not anki</html>").expect_err("not json");
        assert!(matches!(error, AnkiError::BadResponse(_)));
        assert!(error.to_string().contains("not anki"));
    }

    #[test]
    fn anki_not_running_is_reported_as_unreachable() {
        // Port 1 is not going to answer.
        let error = Anki::new("http://127.0.0.1:1")
            .version()
            .expect_err("nothing is listening");
        assert!(matches!(error, AnkiError::Unreachable(_)), "{error:?}");
        assert!(error.to_string().contains("AnkiConnect"));
    }
}
