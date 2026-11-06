//! What the app remembers between sessions: word status and settings
//! (`project notes` §2).
//!
//! ## Why plain files rather than a database
//!
//! `project notes` allows redb or SQLite. Neither is needed for this: word status is one
//! small map read on every passage and written when the learner marks a word, and
//! settings are a handful of fields. A JSON file loaded once and rewritten atomically
//! is the whole requirement — and it stays inspectable, which a binary store does not.
//! The API here is deliberately narrow so a real database can take over if history
//! grows into something a flat file cannot serve.
//!
//! Writes go to a temporary file and are renamed over the target, so an interrupted
//! save cannot leave a half-written file where a learner's marked words used to be.
//!
//! History (§2's third item) is not here: nothing consumes it yet, and inventing a
//! shape for it now would be guesswork.

pub mod settings_file;

pub use settings_file::SETTINGS_VERSION;

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::model::Token;

/// How well the learner knows a word.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WordStatus {
    /// Never marked. The default, and what most of a first passage will be.
    #[default]
    New,
    /// Met before — no longer unfamiliar, not settled either.
    ///
    /// **`alias` is load-bearing.** This shipped as `Learning` and serialises
    /// lowercase, so saved files hold `"learning"`. Without the alias `Store::open`
    /// fails the whole `Vec<WordRecord>` and sets every word status aside.
    #[serde(alias = "learning")]
    Seen,
    /// Known. The reward for progress is a page that stops shouting.
    Known,
    /// Deliberately not tracked: names, or words the learner does not want counted.
    Ignored,
}

/// Identifies a word for study purposes.
///
/// Term **and** reading, because homographs are different words to a learner: 上手
/// read じょうず is not 上手 read うわて, and giving them one status would mark a word
/// the learner has never met.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WordKey {
    pub term: String,
    #[serde(default)]
    pub reading: String,
}

impl WordKey {
    pub fn new(term: impl Into<String>, reading: impl Into<String>) -> WordKey {
        WordKey {
            term: term.into(),
            reading: reading.into(),
        }
    }

    /// The key for a token: its dictionary form, or its surface when the analyzer
    /// gave no lemma. The same rule the dictionary lookup uses, so the status shown
    /// belongs to the word whose definition is shown.
    pub fn for_token(token: &Token) -> WordKey {
        let term = if token.dict_form.is_empty() {
            &token.surface
        } else {
            &token.dict_form
        };
        WordKey::new(term.clone(), token.lemma_reading.clone())
    }
}

/// One stored record, as it appears on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WordRecord {
    #[serde(flatten)]
    key: WordKey,
    status: WordStatus,
}

/// User settings. Every field defaults, so an older file keeps working when a new
/// setting is added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Show furigana over words containing kanji.
    pub furigana: bool,
    /// Lay the passage out with the line breaks the capture had, rather than as one paragraph.
    ///
    /// **Off by default, which is a deliberate reversal.** The reader used to be strict about
    /// this — what it showed was what you pasted, breaks included — and for pasted prose that
    /// is right. For the material this app is aimed at it is not: a manga panel or a game box
    /// arrives broken by the *bubble's* width, so honouring those breaks spends vertical space
    /// on a shape that belongs to the artwork and not to the sentence, and pushes the reader
    /// into scrolling a passage that would otherwise fit.
    ///
    /// Display only. Analysis, translation and Anki all read the text and the tokens, neither
    /// of which this touches — see `passageLines` in `ui/src/lib/passage.js`.
    #[serde(default)]
    pub keep_line_breaks: bool,
    /// Where each line of the passage starts in the reader: the left edge, the middle, or the
    /// right edge.
    ///
    /// **Left by default**, which is how the reader has always set a passage. Offered in the quick
    /// panel beside the passage's size, because it is judged the same way — by looking at the text
    /// while it changes.
    ///
    /// Display only, like `keep_line_breaks`: analysis, translation and Anki read the text and the
    /// tokens, and this touches neither.
    pub passage_align: PassageAlign,
    /// Draw the dotted rule under a reading the app is not sure of.
    ///
    /// **Off by default, which is a deliberate reversal.** The mark is the honest half of showing
    /// furigana — a learner cannot tell a guessed reading from a certain one, and `dict::readings`
    /// knows which is which. It shipped on for that reason. In use it reads as clutter: even at
    /// 12.3% of kanji-bearing tokens, after the margin in `dict::readings` cut it from 32.9%, it is
    /// a mark on roughly one word in eight of a page somebody is trying to *read*.
    ///
    /// So the caveat is offered rather than imposed. Nothing about the measurement changed and
    /// nothing about the honesty argument is withdrawn — a reader who wants it turns it on, and it
    /// is one toggle away in General settings.
    ///
    /// Display only. The flag is still computed and still reaches the vocabulary panel; this
    /// decides whether `Reader.svelte` draws it over the kana.
    pub mark_reading_guesses: bool,
    /// Offer a grouped word's parts under the selected word, as buttons that look each one up.
    ///
    /// **Off by default.** Grouping exists so the *whole* word is the easy lookup — かっこいい
    /// rather than かっこ + いい — and the parts row is for the times that is not what you wanted.
    /// On a page where most words are grouped it is a row of buttons under every selection,
    /// which is clutter in front of the definitions somebody opened the panel to read.
    ///
    /// Display only, and it takes nothing away: the parts are still analysed, still carry their
    /// own learning status, and a reader who wants to reach them turns this on in General
    /// settings, beside the guess mark.
    ///
    /// Switching it off while a part is open returns the selection to the whole word. The reader
    /// does that, not the store; it is recorded here because the two cannot be separated —
    /// hiding the row alone would strand every action on a word nothing on screen names.
    pub show_word_parts: bool,
    /// Put every captured passage on the clipboard as soon as it is recognised.
    ///
    /// **Off by default: it takes over the clipboard, which belongs to whatever else is running.**
    /// On, it is for reading with the app open beside something else and wanting the text out —
    /// collecting the sentences an analyzer reads badly, without stopping to select and copy each
    /// one. The recognised text goes over verbatim, before any analysis touches it.
    ///
    /// Note it can fight [`Self::read_clipboard`], which watches the clipboard for text to read:
    /// with both on the app writes what it then picks up. Harmless — the passage is already the
    /// one on screen — but worth knowing before turning both on.
    pub copy_capture_to_clipboard: bool,
    /// Mark a new word as seen when the selector lands on it.
    ///
    /// On by default: the mark records that the learner has reached the word. **Whatever put the
    /// selector there counts**, including analysis landing on the first word of a passage — that
    /// used to be excluded on the grounds that analysis finishing is not a reading gesture, and
    /// the exclusion was withdrawn because nothing on screen distinguishes that word from one
    /// reached with the arrow keys. It wears the same highlight; it now earns the same mark.
    ///
    /// The consequence worth knowing: with [`Self::read_clipboard`] on, every line the watcher
    /// picks up marks its first word, without a keypress.
    ///
    /// Known and Ignored are never moved backwards — only `New` is promoted.
    pub mark_seen_on_focus: bool,
    /// Require a card to be focused before a click opens it.
    ///
    /// Two models for the same disclosure tab, kept side by side so they can be compared in use.
    ///
    /// On, a result card takes two clicks: the first focuses it, which lightens it and shows the
    /// tab; the second opens it. Off, the tab appears when the pointer rests on a card and opening
    /// is the one click that lands on it.
    ///
    /// **The card is content either way** — selectable and copyable — so the click that opens it
    /// ignores anything landing on a control inside the card or completing a text selection. The
    /// tab remains the only focusable thing in a card and the only keyboard route into it, in both
    /// models; this setting changes what a pointer has to do, never what assistive technology sees.
    pub focus_before_open: bool,
    /// Use conventional Windows Tab traversal and retain focus on clicked controls.
    ///
    /// Off preserves the reader-first interaction the app was designed around. On is an
    /// accessibility mode, not a second focus implementation: the webview's native traversal
    /// resumes, while the passage exposes one roving Tab stop for its selected word.
    pub tab_navigation: bool,
    /// The reader's share of the split, 0–1.
    pub split_ratio: f64,
    /// What fires a capture. See [`Binding`].
    pub capture_bind: Binding,
    /// What shows the frame, and hides it while held.
    ///
    /// Separate from [`Settings::capture_bind`] so "read this" and "get out of my way"
    /// can live on different keys — the second is wanted precisely when the frame is in
    /// the way of the thing being read.
    pub overlay_bind: Binding,
    /// Where the capture frame was left, in logical desktop pixels: `[x, y, w, h]`.
    ///
    /// A frame that reopens somewhere else every session is not persistent in the way
    /// that matters — the whole point is to put it over a column of text and leave it.
    pub frame_rect: Option<[f64; 4]>,
    /// How long the show/hide binding must be held to count as a hold rather than a tap.
    pub hold_ms: u64,
    /// How quickly a second tap of the show/hide binding must follow the first to count as
    /// a double tap, which locks the frame — see [`Settings::frame_locked`].
    ///
    /// **Zero turns the gesture off**, and that is worth having rather than tidy: a tap
    /// already *does* something, so two taps in a row is a thing people do by accident —
    /// show the frame, move the pointer, tap again to bring it over — and on a machine where
    /// that is somebody's habit the lock would keep flipping for no reason they can see.
    ///
    /// Longer than the mouse's double-click default would be, because this is a chord on two
    /// hands rather than two clicks of one finger, and the second press of `Ctrl+Shift+E`
    /// lands later than the second click of a mouse.
    pub lock_tap_ms: u64,
    /// Hide the frame while a capture is taken, instead of relying on its transparent
    /// interior.
    ///
    /// The frame's middle is genuinely transparent, so in principle the desktop composites
    /// the window beneath straight through it and nothing of ours is in the shot. Turning
    /// this on stops arguing and hides the window for the duration, which is certain but
    /// flickers on every read.
    pub hide_during_capture: bool,
    /// The interface's root size in pixels. Everything else is in `rem`, so this one
    /// number scales the reader and the panels together.
    pub font_size: f64,
    /// The analyzed passage's size, as a multiple of what [`Self::font_size`] gives it.
    ///
    /// A multiplier rather than a pixel size so that a settings file written before this
    /// existed renders identically: 1.0 is exactly today. A pixel default would have to
    /// be derived from the saved root size to avoid resizing everyone's app on upgrade,
    /// and `#[serde(default)]` cannot derive.
    pub reader_scale: f64,
    /// The same, for the definitions and grammar panels.
    pub panel_scale: f64,
    /// Watch the clipboard and analyze Japanese as it is copied.
    ///
    /// For reading with a text hooker, where every line arrives on the clipboard and the
    /// alternative is a button press per line.
    pub read_clipboard: bool,
    /// Keep the analyzer sidecar's console window off the screen. See
    /// [`crate::analyzer::ichiran::set_quiet`].
    pub quiet: bool,
    /// Keep one analyzer sidecar alive rather than starting one per passage. See
    /// [`crate::analyzer::ichiran::server`].
    ///
    /// **On by default, and this used to be off.** The old reasoning was that a default holding a
    /// process open "has to be asked for rather than assumed". The asking turns out to have
    /// already happened somewhere else: Ichiran is `Tier::Advanced`, installed through
    /// `scripts/setup-ichiran.ps1` and a WSL distribution, so nobody has it by accident — and
    /// [`crate::store::AnalyzerChoice::Auto`], the default, uses it whenever it answers. Everyone
    /// who has it was paying a full SBCL image load per passage.
    ///
    /// Measured 2.8–3×: 1428 ms a passage against 469 ms when this was written, 714 ms against
    /// 252 ms on a later run. **The analysis is identical either way** —
    /// `the_persistent_sidecar_answers_exactly_what_the_one_shot_one_answers` pins that — so the
    /// only thing traded is memory for latency, and only the latency is visible.
    ///
    /// What it costs is an SBCL image and a PostgreSQL connection held for as long as the app
    /// runs. Turning this off releases them, and so does exit.
    ///
    /// Reaches Ichiran only. The built-in analyzer is compiled in and has no process to keep
    /// alive, so this does nothing at all when it is the one selected.
    ///
    /// **No `#[serde(default)]` of its own, deliberately.** A field-level attribute would fill a
    /// missing value from `bool::default()` — `false` — overriding the struct-level one and
    /// quietly excluding every settings file written before this field from the default above.
    /// [`Settings::anki_our_decks_only`] spells the trap out; this is the same one.
    pub ichiran_server: bool,
    /// Start Ichiran after the window opens. Missing older values inherit the enabled default.
    /// Independent of persistence: off defers all analyzer startup work until a reading intent.
    pub preload_ichiran_on_startup: bool,
    /// Load the selected OCR engine after the window opens, and again when a settings change
    /// selects a new one. Missing older values inherit the enabled default. Off, nothing loads
    /// until a capture asks for it — which is what keeps PaddleOCR-VL's local server, the one
    /// engine that takes graphics memory, from starting for a session that only reads text.
    pub preload_ocr_on_startup: bool,
    /// Check release metadata daily; downloading and installing always require a choice.
    pub check_updates_on_startup: bool,
    /// Write a diagnostic log file. See [`crate::log`].
    ///
    /// Off by default. A packaged build has no console, so without this there is nothing
    /// to attach to a bug report — and with it, there is a file the app is constantly
    /// writing to that nobody asked for.
    pub log: bool,
    /// Keep the Japanese you capture — in a corpus file, and in the log when logging is on.
    ///
    /// A separate switch rather than part of the log's, because this app reads whatever is on
    /// screen and on the clipboard: a file holding that is a transcript of what someone has been
    /// reading. Worth having, worth being asked about.
    ///
    /// **Was `log_content`, and the alias is why an existing install keeps its answer.** It only
    /// put text in the diagnostic log, which is the right shape for chasing a misread and the
    /// wrong one for everything else — a corpus wants one passage per line with no duplicates and
    /// no prose around it. It now writes that file as well, so the switch is a superset of what it
    /// was and nobody loses the diagnostic. See [`crate::corpus`].
    #[serde(alias = "log_content")]
    pub collect_sentences: bool,
    /// Which analyzer to use. Takes effect on restart — the analyzer is chosen once and
    /// held as a trait object.
    pub analyzer: AnalyzerChoice,
    /// Which recognition engine reads a capture.
    pub ocr: OcrChoice,
    /// Where the vision-language model is served, when `ocr` is [`OcrChoice::Vlm`].
    ///
    /// A URL rather than a model path: the guided path runs a local llama.cpp server,
    /// while an advanced user may point at any compatible server. See [`crate::ocr::vlm`].
    pub vlm_endpoint: String,
    /// Which model to ask that server for.
    ///
    /// Empty for llama-server, which serves the one file it was started with and ignores
    /// this. Ollama and LM Studio hold several at once and route on it, so leaving it
    /// unreachable would make them unusable — which the endpoint field alone implies they
    /// are not.
    pub vlm_model: String,
    /// Preparation applied to a crop before it reaches the engine.
    ///
    /// All off by default. `local documentation` argues each from the literature and
    /// then says the quiet part — none of it is measurable without a corpus — so these
    /// change nothing until someone turns them on and `core/tests/ocr_corpus.rs` says
    /// they helped.
    pub preprocess: PreprocessSettings,
    /// The Anki deck new cards go to.
    pub anki_deck: String,
    /// Show only decks worth exporting to, hiding the rest of the collection.
    ///
    /// **On by default.** A reader's collection is mostly other people's shared decks — thirty-odd
    /// of them is ordinary — and a dropdown listing all of them to choose between three is a list
    /// you read rather than a list you use. What stays is decks holding cards from here plus empty
    /// ones, which is every deck an export could sensibly go to.
    ///
    /// It remains a preference rather than a rule, and the interface ignores it while the filtered
    /// list would be empty, so it can never leave a learner with nothing to choose.
    ///
    /// This is a *convenience*, and deliberately not the same question as whether a deck may be
    /// renamed or deleted. A deck holding a learner's own work alongside some exported cards
    /// belongs in this list and must never be writable — see
    /// [`sakatsu_core::anki::DeckInfo::managed`].
    ///
    /// **Defaults to on, and [`Settings::default`] is the only place that says so.** `Settings`
    /// carries `#[serde(default)]` at struct level, so a field absent from an older file is
    /// filled from `Settings::default()` — not from `bool::default()`. That matters to get right
    /// in a comment: a `bool` here needs no attribute of its own, and a reader who believes
    /// otherwise will either add a redundant one or, worse, assume a bare `#[serde(default)]`
    /// would have given them `false` and work around a problem that does not exist.
    pub anki_our_decks_only: bool,
    /// The palette, as the few inputs every colour is derived from.
    ///
    /// **Inputs rather than the thirty tokens they produce.** A stored palette is frozen at the
    /// token set of the day it was written, so any token added later would arrive at a default
    /// belonging to a different theme — the same reason the audio cache stores answers rather
    /// than a filtered list. `ui/src/lib/theme.js` does the deriving; nothing here computes
    /// colour.
    #[serde(default)]
    pub theme: Theme,
    /// Palettes the learner saved.
    ///
    /// **The first user-authored content this app persists.** Everything else in here is a choice
    /// among options the app defines, so a schema change can rewrite it; a palette is something
    /// somebody made. That is why it stores the same inputs [`Theme`] does rather than resolved
    /// colours — a saved palette holding thirty tokens would be frozen at the token set of the day
    /// it was written, and every token added later would arrive at a default from a different
    /// theme. The information needed to re-derive it would simply not be there.
    ///
    /// The shipped palettes are deliberately **not** in here. They are code, so they can be
    /// corrected in a later release; a copy in somebody's settings file could not be.
    #[serde(default)]
    pub theme_presets: Vec<NamedTheme>,
    /// Whether the interface animates at all.
    pub animations: bool,
    /// How long an animation runs, in milliseconds.
    pub animation_ms: u64,
    /// Vocabulary's share of the width, 0–1.
    ///
    /// There used to be a `panels_split` beside this, choosing between the two columns and a
    /// tab bar, defaulting to tabs. A word's meaning and the grammar around it are one
    /// question asked twice, and tabs made you put one down to pick up the other. The divider
    /// goes down to a quarter, which is what reaching for a tab was really asking for.
    ///
    /// Separate from [`Self::split_ratio`], which divides the passage from the study area
    /// below it. Both are dragged, and a single number could not remember two positions.
    pub panels_ratio: f64,
    /// The kasumi band at the foot of the reading area: its opacity. `0.0` still draws nothing.
    pub decor_cloud: f64,
    pub decor_cloud_tint: DecorTint,
    /// Whether the cloud band is drawn at all.
    ///
    /// **A switch beside the opacity, and that reverses an earlier decision on purpose.** The
    /// opacity used to be the only control, on the grounds that zero *is* off. It is — but turning
    /// the clouds off that way threw away the strength somebody had tuned, so turning them back
    /// on meant finding it again by eye. Off is now its own fact, and the strength waits beneath
    /// it untouched. Asked for on 2026-10-19.
    ///
    /// Not part of [`ThemeDecor`]: a palette says how the watermarks look, and whether you want
    /// them at all is yours. Applying a palette with clouds in it does not turn them back on.
    pub decor_cloud_on: bool,
    /// The sakura down the rail, the same way.
    pub decor_sakura: f64,
    pub decor_sakura_tint: DecorTint,
    /// Whether the sakura is drawn, for the same reason as [`Self::decor_cloud_on`].
    pub decor_sakura_on: bool,
    /// One multiplier over both motifs' size.
    ///
    /// Their scale is otherwise pinned to the interface, deliberately — it used to follow
    /// the window, which meant a wide window got *bigger* clouds rather than more of them.
    /// This is the knob that was missing once that was fixed.
    pub decor_scale: f64,
    /// How far the cloud band stops above the footer's rule, in `rem`.
    pub decor_cloud_pad: f64,
    /// How far apart the motifs sit, as a multiplier over their placements.
    ///
    /// Separate from [`Self::decor_scale`], which is how big each one is drawn. Size and
    /// spacing are different questions — a band of small clouds packed tight and a band of
    /// small clouds strung out are both things somebody might want, and one slider cannot
    /// say which.
    ///
    /// Two of them, because the two patterns run on different axes through very differently
    /// shaped space: the clouds spread sideways along a band as wide as the window, the
    /// sakura down a rail a couple of centimetres across. A single number would be a
    /// compromise in both.
    ///
    /// **Not a CSS token, unlike every other value here.** These multiply coordinates
    /// *inside* the SVGs — the placements and the viewBox — which no custom property can
    /// reach, so they travel to `Kasumi` and `Sakura` as props.
    pub decor_cloud_spread: f64,
    pub decor_sakura_spread: f64,
    /// Soften both motifs, so they read as further away rather than merely fainter.
    ///
    /// **A switch, against the rule [`Self::decor_cloud`] sets three fields up** — and the
    /// exception is real rather than a lapse. `filter: blur(0px)` is not the same as no filter:
    /// any non-`none` value promotes the element to its own composited layer and re-rasterises
    /// it, so a slider dragged to zero would leave that cost running on a machine getting
    /// nothing back for it. Off has to mean `filter: none`, and only a separate switch can say
    /// so. See `local documentation`.
    pub decor_blur: bool,
    /// How soft each is, in pixels on the display.
    ///
    /// **Screen-space, and deliberately not multiplied by [`Self::decor_scale`].** Blur is how
    /// sharp an edge looks to the eye, so the number beside the slider is what lands on the
    /// glass — a bigger cloud at the same setting is no softer. The alternative, scaling with
    /// the pattern, keeps the look constant across sizes at the cost of the printed number
    /// meaning something other than it says.
    ///
    /// Two of them, and their ranges differ by a factor of nearly three, because the motifs are
    /// drawn at nothing like the same size: the cloud bars are solid fills about seven pixels
    /// tall, where a blossom is a 1.4px `non-scaling-stroke` outline. Spreading a line that thin
    /// over more than about three pixels drops its peak alpha to nothing, so past there the
    /// sakura slider stops being softness and becomes a second opacity.
    pub decor_cloud_blur: f64,
    pub decor_sakura_blur: f64,
    /// Which installed frequency list orders search results, by directory name.
    ///
    /// Empty means none, which is the default and not a failure — nothing ships a corpus,
    /// and without one the results keep the dictionary's own order and say so.
    pub frequency_source: String,
    /// Whether that list's numbers are ranks or counts.
    ///
    /// Declared rather than detected. Nothing about an integer says which it is, and
    /// reading a count list as a rank orders the results exactly backwards while looking
    /// like it worked. See `dict::frequency::Direction`.
    pub frequency_direction: crate::dict::frequency::Direction,
    /// Name the active frequency list beneath ranked Reference results.
    ///
    /// Off by default. The list still orders the results when this is off; this controls
    /// only the small explanatory line. The corpus remains credited in About and its
    /// installation metadata, so hiding repeated in-context provenance is not hiding a
    /// licence obligation.
    pub show_frequency_source: bool,
    /// Which settings section was last open, by id.
    ///
    /// Settings is a page showing one section at a time, so there is one thing to remember
    /// rather than a list of what was folded shut. **An id this build does not know is not an
    /// error** — the interface falls back to its first section, which is what a settings file
    /// written by a newer version should do to an older one.
    pub settings_section: String,
    /// A hotkey that switches the overlay off and back on, if one has been set.
    ///
    /// `None` by default and legitimately absent — the only binding that can be. The
    /// header button is the other way to reach [`Self::overlay_enabled`], and most people
    /// will never want a global chord for it.
    pub switch_bind: Option<Binding>,
    /// How long the overlay button waits before acting on a click, in milliseconds.
    ///
    /// A double click delivers two `click` events before its `dblclick`, so the click is
    /// held this long and cancelled if a double one follows. Floored at 100 in
    /// [`Settings::click_delay`]: below that the double click has no room to arrive and
    /// the button quietly loses half of what it does.
    pub click_delay_ms: u64,
    /// Whether the capture overlay answers at all.
    ///
    /// Off, the frame cannot be shown and **both bindings stop responding** — no capture,
    /// no show/hide. For a reader who works from pasted text or the clipboard and does not
    /// want two global chords live for a feature they never use.
    ///
    /// It stays off across restarts, which is the point and also the risk: two keys that
    /// silently do nothing is a bad way to find out. The control that sets it is drawn in
    /// the attention colour for exactly as long as this is false.
    pub overlay_enabled: bool,
    /// Leave the capture frame where it is when the show/hide binding is tapped.
    ///
    /// A tap normally brings the frame to the pointer, which is the point of the binding —
    /// point at the text and tap. Once the frame is parked over a column and being read
    /// repeatedly, that is the wrong half of the feature, and this turns it off without
    /// giving up the binding: a tap still shows, a hold still hides, and the frame is
    /// still dragged and resized by hand. Only the trip to the cursor stops.
    pub frame_locked: bool,
    /// How Reference previews are arranged when there is room for either presentation.
    ///
    /// `dictionary_show` was the name used by the earlier Cards/Compact control. Keeping it as
    /// a deserialize-only alias brings that preference forward; serialisation always writes the
    /// clearer current name.
    #[serde(alias = "dictionary_show")]
    pub reference_layout: BrowseLayout,
    /// How the glossary of grammar terms is ordered.
    pub glossary_sort: GlossarySort,
    /// The reference page's open tabs, by query, left to right.
    ///
    /// **The questions, not the answers.** Results are derived data: storing them would
    /// bloat the settings file and hand back an answer built against dictionaries that may
    /// since have been added, removed or updated. A stale answer presented as a current one
    /// is worse than the moment it takes to ask again, so a restored tab re-runs its query
    /// — and only when it is selected, or opening six tabs would fire six searches at
    /// launch.
    ///
    /// Trimmed to [`Settings::REFERENCE_TABS_MAX`] by [`Settings::normalise`], so a long
    /// session cannot grow this without bound.
    pub reference_tabs: Vec<String>,
    /// Which of them is in front. Clamped to the list by [`Settings::normalise`].
    pub reference_tab: usize,
    /// Name the dictionary each definition came from, on the analysis page.
    ///
    /// **Off by default, and that is not a licence question.** CC BY-SA is satisfied by the
    /// credit in the footer (`project notes` §8); the per-entry chip answers "where did *this*
    /// one come from", which is a question most reading does not raise. It earns its place
    /// for someone comparing sources and costs a chip to everyone else.
    pub source_on_analysis: bool,
    /// The same, on the reference page.
    ///
    /// Separate rather than one switch because the pages have different densities: a search
    /// returns eighteen results and repeats the same dictionary's name on every one of them,
    /// where the analysis page shows a single word. Somebody may well want it in one place
    /// and not the other.
    pub source_on_reference: bool,
    /// A wash over the region, so it can be seen at a glance. `0.0` is off.
    ///
    /// **Raising this forces `hide_during_capture` on, in [`Settings::normalise`].** The
    /// frame does not photograph itself only because it paints nothing over the region;
    /// a tint is paint, and the one way it does not reach the OCR is the window being gone
    /// when the pixels are taken. That costs a flicker per read, which is the price of the
    /// feature and is said plainly in the interface.
    ///
    /// Clamped well below 1: past a light wash you cannot see what you are aiming at, which
    /// is the entire job of the window.
    pub frame_tint: f64,
    /// Let clicks reach whatever is behind the capture frame.
    ///
    /// For reading over something you also want to *use* — a game, a page you are
    /// scrolling. The frame keeps its chrome solid, so it can still be moved, resized and
    /// closed; only the part you look through stops taking the pointer.
    pub frame_clickthrough: bool,
    /// Keep the window above every other one.
    ///
    /// For reading alongside the thing being read: a game, a manga viewer, a browser. The
    /// capture frame is always on top already — it has to be, it is a window you look
    /// *through* — and this is the same wish for the window that holds the analysis.
    ///
    /// Stored, because it is a working arrangement rather than a momentary one: someone who
    /// reads this way reads this way every session.
    pub always_on_top: bool,
    /// Keep the installation panel off the main screen.
    ///
    /// Once everything is installed that panel says so and nothing else, forever, above
    /// the reader. It is already collapsible, so this means gone rather than folded.
    ///
    /// The panel overrides this whenever it has something to say — see `Setup.svelte`. A
    /// preference that could hide a broken install would be hiding the only route to
    /// repairing it, and this app is meant to be set up by someone non-technical.
    pub hide_setup: bool,
}

/// Which recognition engine reads a capture.
///
/// The conventional engines remain directly selectable because no classifier can promise
/// which reading is correct. Auto follows PP-OCRv6's detected writing direction and confidence
/// rejection, then consults manga-ocr only when that evidence says it should.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OcrChoice {
    /// Conservative automatic selection between the installed conventional engines.
    #[default]
    Auto,
    /// Force manga-ocr: the domain-specific option for manga and hand lettering.
    Manga,
    /// PP-OCRv6 (small) since 2026-10-09; PP-OCRv5 before. The stored value is unchanged.
    Paddle,
    /// A vision-language model over an OpenAI-compatible endpoint.
    ///
    /// **Paused until 2026-08-25, and un-paused by measurement rather than by opinion.** It was
    /// held back for two stated reasons, and the corpus answered both:
    ///
    /// - *Seconds per capture.* Measured at 49 s for one read on CPU. On a 4070 Ti SUPER it is
    ///   **0.1 s**, so the objection was about hardware and not about the model. `vlm_setup`
    ///   times a read and says which of those a given machine is.
    /// - *Confident hallucination* — a generative model answering fluently when it is wrong,
    ///   which for a lookup tool is the worst available failure. Its six worst samples on the
    ///   141-capture corpus were read by hand: two were ellipsis spelling and four omitted a
    ///   role label above a name. **None invented text.**
    ///
    /// It scored 0.0297 mean character error against 0.0975 for Auto, and below the 0.0652 that
    /// no router over the two conventional engines can beat. `corpus/RESULTS.md` has the run.
    ///
    /// **Selectable, never automatic.** Six hand-checked samples are not enough to let `Auto`
    /// enter a generative model unattended, so [`crate::ocr::auto`] still never does.
    Vlm,
}

/// How the glossary of grammar terms is ordered.
///
/// An enum rather than a string so the two spellings cannot drift: these serialize to
/// exactly `"grouped"` and `"alpha"`, which are the values the interface's own union type
/// already uses, and a third ordering added on one side fails to compile on the other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GlossarySort {
    /// Under the headings the terms are filed by — forms, concepts, parts of speech.
    #[default]
    Grouped,
    /// One list, A to Z.
    Alpha,
}

/// Where each line of a passage starts in the reader. See [`Settings::passage_align`].
///
/// An enum rather than a string so the two spellings cannot drift: these serialize to exactly
/// `"left"`, `"center"` and `"right"`, the values `PassageAlign` in `ui/src/model.ts` uses and the
/// reader hands straight to `text-align`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PassageAlign {
    /// From the left edge, as the reader has always set a passage.
    #[default]
    Left,
    /// Each line centred.
    Center,
    /// Each line set against the right edge.
    Right,
}

/// How a browse collection spends the width available to it.
///
/// Narrow pages override this presentation to rows without rewriting the stored preference, so
/// a card reader gets cards back when the window becomes wide again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrowseLayout {
    #[default]
    Cards,
    /// `compact` was the persisted spelling of the former row presentation.
    #[serde(alias = "compact")]
    Rows,
}

/// What colour the decorative watermarks are drawn in.
///
/// `Blossom` is `--red`, which already means *ignored* on a word and *discard* on a
/// button. Those never meet today, which is the only reason one colour carries both; a
/// wash across the page is a third use of the same hue. At the strengths this is drawn at
/// it is unlikely to read as either, and it was asked for, so it ships — but that is the
/// reason to be careful about raising its opacity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecorTint {
    /// The reading colour, and the quietest of the three.
    #[default]
    Ink,
    Blossom,
    Accent,
}

/// Which palette the interface wears.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// The palette the app has always had, and still the default.
    #[default]
    Dark,
    Light,
    /// Whatever the operating system is asking for, followed as it changes.
    ///
    /// **Not readable from the webview alone.** `tauri.conf.json` used to pin the window's theme
    /// to `Dark`, which forces `prefers-color-scheme` inside the webview to agree with it whatever
    /// the desktop is doing — so this is resolved from Tauri's window theme, not from a media
    /// query. See `commands::apply_window_theme`.
    System,
}

/// The palette, as inputs.
///
/// **No longer `Copy`** — the hand-set overrides are owned data, so it is cloned where it used to
/// be duplicated for free. It is still small and cheap.
///
/// Small on purpose: a mode and two hues, from which `ui/src/lib/theme.js` derives every colour
/// token. The four word-status hues are deliberately **not** here — the app owns them, so that
/// "new", "seen", "known" and "ignored" cannot be tuned into each other. They still change between
/// light and dark, because a blue that reads on near-black does not read on near-white.
/// **`#[serde(default)]` at struct level, for the reason spelled out on [`ThemeDecor`].** `mode`,
/// `surface_hue` and `accent_hue` carried no filler, so a theme object missing any one of them —
/// hand-edited, or written by a build that renamed a field — failed `Theme`, which failed
/// [`Settings`], which falls back to [`Settings::default`]: one absent hue silently discarding
/// every hotkey, deck and preference in the file. At struct level rather than per field so the
/// next field added here is safe without anybody having to know that.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub mode: ThemeMode,
    /// The faint cast that keeps the greys sitting with the accents rather than fighting them.
    pub surface_hue: f64,
    pub accent_hue: f64,
    /// How far round the circle the six part-of-speech hues are turned, in degrees.
    ///
    /// **One number for all six, so they cannot be turned into each other.** The kinds are read
    /// against *each other* — a sense list is legible because adverb and noun are visibly
    /// different — and the app owns that spacing for the same reason it owns the four status
    /// hues. What a palette gets to say is where the whole wheel sits, which is what stops a
    /// kind landing on the accent: at `0` the `Function` hue is 11° from Ai's accent and `Verb`
    /// is 5° from Kohi's, near enough to be the same colour.
    ///
    /// Individual kinds can still be set outright through [`Self::overrides`], the same way the
    /// accent can. This is the dial; that is the escape hatch.
    pub kind_spin: f64,
    /// How strong that cast is. `1.0` is as designed, `0.0` is neutral grey.
    pub tint: f64,
    /// The gap between text and what it sits on. `1.0` is as designed.
    pub contrast: f64,
    /// More ink, in either mode — blacker on dark, greyer on light. `0.0` is as designed.
    pub depth: f64,
    /// Individual tokens the learner set by hand, as `#rrggbb`.
    ///
    /// **The only part of a theme that is not derived**, and the reason `themeWarnings` exists:
    /// a hand-set colour is taken at its word, so the contrast floors the dials could not break
    /// become breakable here. Sorted, so the settings file is stable between writes.
    pub overrides: BTreeMap<String, String>,
}

impl Default for Theme {
    fn default() -> Self {
        // The hues measured out of the palette that was written by hand, so the default theme
        // derives back to it.
        Theme {
            mode: ThemeMode::Dark,
            surface_hue: 300.0,
            accent_hue: 297.0,
            // Sumi's kinds already sit as far from its accent as the set allows, so the default
            // palette turns nothing. See `PRESETS` in `theme.js` for the ones that do.
            kind_spin: 0.0,
            tint: 1.0,
            contrast: 1.0,
            depth: 0.0,
            overrides: BTreeMap::new(),
        }
    }
}

/// Hold a number to a range, with a fallback for anything that is not a number at all.
fn clamp_finite(value: f64, low: f64, high: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback
    }
}

/// The look a palette carries beyond its colours: the watermarks, and how the interface moves.
///
/// **Kept apart from [`Theme`] on purpose.** `Theme` is the *input to the colour derivation* —
/// `theme.js` takes it and produces thirty tokens — and decoration is not a colour input. Folding
/// these in would put eight numbers in front of a function that has no use for them, and every
/// test of the derivation would carry them.
///
/// Sizes are deliberately absent. A palette that resized somebody's interface when they tried it
/// is the reason presets started out as colour only; watermarks and animation are how the app
/// *looks*, where a font size is how well they can read, and only one of those is a matter of
/// taste.
///
/// **`#[serde(default)]` at struct level, and that is load-bearing.** Without it, adding a field
/// here breaks every palette already on disk that carries decoration: the missing key fails
/// `ThemeDecor`, which fails [`NamedTheme`], which fails [`Settings`] — and a `Settings` that
/// will not parse falls back to [`Settings::default`], so one new field silently discards the
/// whole file rather than just the palettes. It is at struct level rather than on the three
/// fields that needed it so that the next field somebody adds is safe without their having to
/// know any of this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeDecor {
    pub cloud: f64,
    pub cloud_tint: DecorTint,
    pub sakura: f64,
    pub sakura_tint: DecorTint,
    pub scale: f64,
    pub cloud_pad: f64,
    pub cloud_spread: f64,
    pub sakura_spread: f64,
    pub blur: bool,
    pub cloud_blur: f64,
    pub sakura_blur: f64,
    pub animations: bool,
    pub animation_ms: u64,
}

/// The same values [`Settings::default`] holds, and `a_palette_defaults_to_what_a_fresh_install_has`
/// is what keeps the two lists honest. This exists only because `#[serde(default)]` above needs
/// it — a palette is always written whole, so nothing constructs one from nothing in practice.
///
/// **One exception: softness stays off.** Here a default fills a key missing from a palette on
/// disk, and the only palettes missing `blur` are ones saved before softness existed — so the
/// truthful answer is that they never had it. When a fresh install started softened, copying that
/// would have made every such palette blur the watermarks the first time it was applied.
impl Default for ThemeDecor {
    fn default() -> Self {
        let fresh = Settings::default();
        Self {
            cloud: fresh.decor_cloud,
            cloud_tint: fresh.decor_cloud_tint,
            sakura: fresh.decor_sakura,
            sakura_tint: fresh.decor_sakura_tint,
            scale: fresh.decor_scale,
            cloud_pad: fresh.decor_cloud_pad,
            cloud_spread: fresh.decor_cloud_spread,
            sakura_spread: fresh.decor_sakura_spread,
            blur: false,
            cloud_blur: fresh.decor_cloud_blur,
            sakura_blur: fresh.decor_sakura_blur,
            animations: fresh.animations,
            animation_ms: fresh.animation_ms,
        }
    }
}

impl ThemeDecor {
    /// Hold every value to the range the sliders offer, so a hand-edited file cannot apply a
    /// palette that puts a decoration somewhere the interface cannot undo.
    fn normalise(&mut self) {
        self.cloud = clamp_finite(self.cloud, 0.0, 0.16, 0.06);
        self.sakura = clamp_finite(self.sakura, 0.0, 0.34, 0.14);
        self.scale = clamp_finite(self.scale, 0.6, 1.8, 1.0);
        self.cloud_pad = clamp_finite(self.cloud_pad, 0.0, 4.0, 1.0);
        self.cloud_spread = clamp_finite(self.cloud_spread, 0.5, 2.5, 1.0);
        self.sakura_spread = clamp_finite(self.sakura_spread, 0.5, 2.5, 1.0);
        self.cloud_blur = clamp_finite(self.cloud_blur, 0.0, 8.0, 3.0);
        self.sakura_blur = clamp_finite(self.sakura_blur, 0.0, 3.0, 1.0);
        self.animation_ms = self.animation_ms.clamp(60, 400);
    }
}

/// A palette the learner named and kept.
///
/// `#[serde(default)]` for the same reason as [`Theme`] and [`ThemeDecor`]: a palette that will
/// not parse must cost at most that palette, never the whole settings file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NamedTheme {
    pub name: String,
    pub theme: Theme,
    /// The decoration and motion saved with it, if any.
    ///
    /// **`Option`, and that is the migration.** Palettes kept before this existed carry no
    /// decoration, and applying one leaves the watermarks exactly where they are rather than
    /// resetting them to a default nobody chose. Anything saved from now on carries both.
    #[serde(default)]
    pub decor: Option<ThemeDecor>,
}

impl Theme {
    /// How many hand-set colours are kept.
    ///
    /// There are thirty tokens; this is room for all of them several times over and still a
    /// ceiling, so a hand-edited file cannot grow `settings.json` without bound.
    const OVERRIDES_MAX: usize = 128;

    /// Bring the hues onto the circle.
    ///
    /// Wrapped rather than clamped: 400° is 40°, and refusing it would be refusing a number that
    /// has an obvious meaning. A hand-edited settings file is the case this exists for.
    fn normalise(&mut self) {
        for hue in [
            &mut self.surface_hue,
            &mut self.accent_hue,
            &mut self.kind_spin,
        ] {
            if !hue.is_finite() {
                *hue = 0.0;
            }
            *hue = hue.rem_euclid(360.0);
        }

        // The dials, held to the same ranges `theme.js` holds them to. Stated in both places
        // because neither trusts the other: a hand-edited file reaches Rust, and a slider reaches
        // the derivation, and each has to be safe on its own.
        self.tint = clamp_finite(self.tint, 0.0, 2.5, 1.0);
        self.contrast = clamp_finite(self.contrast, 0.7, 1.6, 1.0);
        self.depth = clamp_finite(self.depth, -0.08, 0.08, 0.0);

        // **A value that is not a colour is dropped, not corrected.** There is no sensible repair
        // for `"blue-ish"`, and writing one in would be inventing a choice nobody made.
        self.overrides.retain(|_, value| {
            value.len() == 7
                && value.starts_with('#')
                && value[1..].chars().all(|c| c.is_ascii_hexdigit())
        });
        while self.overrides.len() > Self::OVERRIDES_MAX {
            let last = self.overrides.keys().next_back().cloned();
            match last {
                Some(key) => {
                    self.overrides.remove(&key);
                }
                None => break,
            }
        }
    }
}

/// Preparation applied to a crop before recognition. See [`crate::ocr::preprocess`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreprocessSettings {
    /// Trim uniform margin from around the content.
    pub tighten: bool,
    /// Local contrast enhancement (CLAHE).
    pub contrast: bool,
    /// Enlarge a small crop before recognition.
    pub upscale: bool,
}

/// Which analyzer the app should use.
///
/// `Auto` is what everyone should want: try Ichiran, fall back to the built-in one. The
/// other two exist for saying "use the built-in one even though the sidecar works" and
/// for finding out *why* the sidecar is being skipped, which is otherwise invisible.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalyzerChoice {
    #[default]
    Auto,
    Ichiran,
    BuiltIn,
    /// Legacy persisted value for the withdrawn ichi.moe analyzer.
    ///
    /// **Kept deserializable, and that is the whole reason it is still here.** Removing the
    /// variant outright would make `"analyzer": "ichimoe"` fail to parse, and a settings file
    /// that fails to parse is *set aside whole* — the reader would lose their palettes, their
    /// bindings, their deck and everything else, to retire one dropdown entry.
    ///
    /// [`Settings::normalise`] migrates it to [`AnalyzerChoice::Auto`], which is where it should
    /// land: ichi.moe ran the same Ichiran engine, so Auto is the nearest thing that is still
    /// offered. The migration runs on load *and* on save, so the stale value is written out of
    /// the file the first time anything is saved. Same shape as [`OcrChoice::Vlm`].
    IchiMoe,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            furigana: true,
            keep_line_breaks: false,
            passage_align: PassageAlign::Left,
            mark_reading_guesses: false,
            show_word_parts: false,
            copy_capture_to_clipboard: false,
            mark_seen_on_focus: true,
            focus_before_open: true,
            tab_navigation: false,
            split_ratio: 0.45,
            capture_bind: Binding::default(),
            overlay_bind: Binding::Key {
                chord: "CmdOrCtrl+Shift+E".into(),
            },
            frame_rect: None,
            hold_ms: 500,
            // Tight enough that "tap, look, tap again" is not a double tap, loose enough
            // that a two-handed chord pressed twice on purpose is.
            lock_tap_ms: 400,
            hide_during_capture: false,
            font_size: 15.0,
            // 1.0 is "exactly as it was", which is what an existing install must get.
            reader_scale: 1.0,
            panel_scale: 1.0,
            read_clipboard: false,
            // The one default that changes behaviour rather than preserving it, because
            // the behaviour it changes has no upside — see `ichiran::QUIET`.
            quiet: true,
            // On, like `quiet` above: it holds a process open, but only for someone who already
            // installed a sidecar deliberately, and it changes latency rather than answers.
            ichiran_server: true,
            preload_ichiran_on_startup: true,
            preload_ocr_on_startup: true,
            check_updates_on_startup: true,
            log: false,
            collect_sentences: false,
            analyzer: AnalyzerChoice::Auto,
            ocr: OcrChoice::default(),
            // The app's own local server. Nothing is contacted until the VLM engine is
            // selected, and existing settings keep any custom endpoint they already hold.
            //
            // Deliberately not 11434: that is Ollama's, which this used to install and no longer
            // does, and a machine running Ollama for its own reasons must not be mistaken for
            // this app's server.
            vlm_endpoint: "http://127.0.0.1:8140".into(),
            vlm_model: "paddleocr-vl-manga".into(),
            preprocess: PreprocessSettings::default(),
            // **Nothing, not `Default`.** `Default` is Anki's own built-in deck and the last
            // place a learner wants 屠つ vocabulary to land; shipping it as the export target
            // meant a fresh install was pointed at somebody else's collection before they had
            // chosen anything. Empty means "no deck chosen", which the interface states and the
            // export button refuses to act on until it is answered.
            anki_deck: String::new(),
            anki_our_decks_only: true,
            theme: Theme::default(),
            theme_presets: Vec::new(),
            animations: true,
            animation_ms: 180,
            panels_ratio: 0.5,
            // **The maintainer's own tuning, taken as the default on 2026-10-19** — read from
            // their settings file rather than chosen here: a faint ink band of clouds packed
            // close, blossom-red sakura at a fifth, both half as large again, and the clouds
            // softened while the sakura stays sharp. Both shown. Existing installs keep whatever
            // they already have; these are what a fresh one, or a missing field, gets.
            decor_cloud: 0.02,
            decor_cloud_tint: DecorTint::Ink,
            decor_cloud_on: true,
            decor_sakura: 0.2,
            decor_sakura_tint: DecorTint::Blossom,
            decor_sakura_on: true,
            decor_scale: 1.5,
            decor_cloud_pad: 1.5,
            decor_cloud_spread: 0.5,
            decor_sakura_spread: 0.7,
            decor_blur: true,
            decor_cloud_blur: 3.0,
            decor_sakura_blur: 0.0,
            frequency_source: String::new(),
            frequency_direction: crate::dict::frequency::Direction::Rank,
            show_frequency_source: false,
            settings_section: "reading".into(),
            switch_bind: None,
            click_delay_ms: 250,
            overlay_enabled: true,
            frame_locked: false,
            reference_layout: BrowseLayout::default(),
            glossary_sort: GlossarySort::default(),
            reference_tabs: Vec::new(),
            reference_tab: 0,
            // Off on both pages: the footer carries the attribution, and a chip on every
            // definition answers a question most reading does not ask.
            source_on_analysis: false,
            source_on_reference: false,
            // Off: it cannot be had without the flicker, so it is not chosen for anyone.
            frame_tint: 0.0,
            frame_clickthrough: false,
            always_on_top: false,
            hide_setup: false,
        }
    }
}

/// What fires a capture: a key chord, or a mouse button.
///
/// Two variants rather than one string, because the two are found by entirely different
/// machinery — the global-shortcut plugin has no notion of a mouse at all — and conflating
/// them would hide that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Binding {
    /// An accelerator as the global-shortcut plugin parses it, e.g. `Ctrl+Shift+Q`.
    Key { chord: String },
    /// A mouse button. Only the three in [`MouseButton`] are offered.
    Mouse { button: MouseButton },
}

impl Binding {
    /// Whether this binding may be set. A keyboard chord always may; a mouse button depends.
    pub fn bindable(&self) -> bool {
        match self {
            Binding::Key { .. } => true,
            Binding::Mouse { button } => button.bindable(),
        }
    }
}

impl Default for Binding {
    fn default() -> Self {
        Binding::Key {
            chord: "Ctrl+Shift+Q".into(),
        }
    }
}

impl Settings {
    /// How long to hold a click on the overlay button before acting on it.
    ///
    /// Clamped rather than trusted. A settings file is a file, and a zero here would
    /// leave the button unable to tell a click from a double one — the failure would show
    /// up as the button doing the wrong thing, with nothing pointing back at the number
    /// that caused it.
    pub fn click_delay(&self) -> u64 {
        self.click_delay_ms.clamp(100, 600)
    }

    /// How long a second tap has to arrive in to lock the frame — **or zero, meaning the
    /// gesture is off**.
    ///
    /// Zero is passed through rather than clamped up, because it is a real answer here and
    /// not a broken number: off is a state somebody may want, and the floor exists to stop a
    /// value so small the second tap can never land in it. Clamping zero up to 150 would turn
    /// "I do not want this" into "I want it, very briefly", which is the failure the floor was
    /// meant to prevent in the first place.
    pub fn lock_tap(&self) -> u64 {
        if self.lock_tap_ms == 0 {
            0
        } else {
            self.lock_tap_ms.clamp(150, 1_000)
        }
    }

    /// The most reference tabs that will be kept across a restart.
    ///
    /// A ceiling rather than a rule about how many may be open at once: the tab strip
    /// scrolls and the app does not police it while running. This only bounds what reaches
    /// the settings file.
    pub const REFERENCE_TABS_MAX: usize = 12;
    /// How many palettes a learner may keep.
    ///
    /// Generous, and a ceiling all the same. `reference_tabs` is capped because a session grows it
    /// silently; this only grows by deliberate acts, so the bound is about keeping `settings.json`
    /// a file somebody could still read rather than about runaway growth.
    pub const THEME_PRESETS_MAX: usize = 64;

    /// Bring stored values that can be out of range back into it.
    ///
    /// Called on load *and* on save, which is not redundant: save covers what this app
    /// writes, and load covers a file that was hand-edited, written by an older version, or
    /// half-written by a crash. Both paths hand the same struct to the interface, so the
    /// interface never has to defend against an index past the end of a list.
    ///
    /// Deliberately not a `Result`. Nothing here is worth refusing to start over — a bad
    /// index means the reference page opens on a different tab than it did last time.
    pub fn normalise(&mut self) {
        // ichi.moe was withdrawn; a file naming it opens on Auto rather than being rejected.
        if self.analyzer == AnalyzerChoice::IchiMoe {
            self.analyzer = AnalyzerChoice::Auto;
        }
        // `OcrChoice::Vlm` used to be rewritten to Auto here, because the engine was in source
        // but not offered and a stored choice would otherwise run invisibly behind a selector
        // showing Auto's label. It is offered now — see the variant's own note for what
        // changed — so the choice is kept.

        // **The retired Ollama endpoint.** This app installed Ollama until 2026-08-27 and wrote
        // its address here; it serves a llama.cpp build of its own now, on a port of its own. A
        // settings file still naming the old one would point the reader at somebody else's
        // service — or at nothing — so exactly that pair is moved on, and any endpoint a person
        // chose themselves is left alone.
        //
        // Matched as a pair rather than on the port: 11434 with a hand-picked model is a
        // deliberate choice to keep using Ollama, and this has no business overruling it.
        if self.vlm_endpoint.trim().trim_end_matches('/') == "http://127.0.0.1:11434"
            && self.vlm_model == "hf.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF"
        {
            let fresh = Settings::default();
            self.vlm_endpoint = fresh.vlm_endpoint;
            self.vlm_model = fresh.vlm_model;
        }

        // **Left and right click are no longer bindable**, and a file that names one is a file
        // written before that rule. Retired here rather than refused, for the reason the whole
        // function exists: a settings file is a file, and one unbindable value is not worth
        // failing to start over.
        //
        // The two required bindings go back to their defaults, which the settings page then
        // shows — a silent rewrite is only silent if nothing displays the result. The optional
        // one clears, because absent is what it defaults to and inventing a global chord
        // nobody asked for is worse than having none.
        if !self.capture_bind.bindable() {
            self.capture_bind = Settings::default().capture_bind;
        }
        if !self.overlay_bind.bindable() {
            self.overlay_bind = Settings::default().overlay_bind;
        }
        if self
            .switch_bind
            .as_ref()
            .is_some_and(|bind| !bind.bindable())
        {
            self.switch_bind = None;
        }

        // Both patterns tile a fixed space, so spread has a floor and a ceiling that are
        // about the artwork rather than about taste. Under 0.5 the clouds overlap into one
        // continuous bar and the slots between them — which *are* the motif — close up. Over
        // 2.5 a wide window shows more gap than cloud, and the rail's sprig thins to one
        // blossom nobody would read as a pattern.
        self.decor_cloud_spread = self.decor_cloud_spread.clamp(0.5, 2.5);
        self.decor_sakura_spread = self.decor_sakura_spread.clamp(0.5, 2.5);

        // The two softness ranges, which differ because the motifs do. `clamp_finite` rather
        // than the bare `clamp` above: `f64::clamp` returns `NaN` for a `NaN` input, and a
        // `NaN` reaching the interface becomes `blur(NaNpx)` — an invalid filter, which drops
        // to no filter at all and looks exactly like the setting not working.
        self.decor_cloud_blur = clamp_finite(self.decor_cloud_blur, 0.0, 8.0, 3.0);
        self.decor_sakura_blur = clamp_finite(self.decor_sakura_blur, 0.0, 3.0, 0.0);

        // A wash past this stops being a marker and starts being a curtain over the thing
        // it marks.
        self.frame_tint = self.frame_tint.clamp(0.0, 0.5);

        // **The tint's precondition, enforced where every reader passes.**
        //
        // The frame does not photograph itself only because it paints nothing over the
        // region. A tint *is* paint, and the sole reason it does not reach the OCR is the
        // window being hidden when the pixels are taken. Left to the interface this would be
        // a rule a checkbox handler knew and a hand-edited settings file walked straight
        // past — arriving as a tint that silently washes every capture.
        if self.frame_tint > 0.0 {
            self.hide_during_capture = true;
        }

        // Hues onto the circle, so a hand-edited file cannot ask for a 400-degree accent.
        self.theme.normalise();

        // **Every saved palette, not only the live one.** A palette is only checked when it is
        // applied, so one that arrived broken would sit in the file waiting rather than being
        // caught — the same argument the frame tint's precondition makes. Nameless entries go: a
        // palette nobody can pick is a row in a list that does nothing.
        self.theme_presets.truncate(Self::THEME_PRESETS_MAX);
        for preset in &mut self.theme_presets {
            if let Some(decor) = preset.decor.as_mut() {
                decor.normalise();
            }
        }
        self.theme_presets
            .retain(|saved| !saved.name.trim().is_empty());
        for saved in &mut self.theme_presets {
            saved.name = saved.name.trim().to_string();
            saved.theme.normalise();
        }

        self.reference_tabs.truncate(Self::REFERENCE_TABS_MAX);
        // `saturating_sub` so an empty list gives 0 rather than underflowing to `usize::MAX`
        // — which is the exact shape of bug this method exists to stop.
        self.reference_tab = self
            .reference_tab
            .min(self.reference_tabs.len().saturating_sub(1));
    }
}

/// The mouse buttons a binding can name.
///
/// All five parse, and [`MouseButton::bindable`] decides which may actually be chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// The back button, "Mouse4".
    X1,
    /// The forward button, "Mouse5".
    X2,
}

impl MouseButton {
    /// Whether this button may be bound to an action.
    ///
    /// **Left and right may not, and that is a reversal.** They used to be allowed, on the
    /// argument that the hook never swallows an event so a bound button goes on doing whatever
    /// it normally does — the cost of a bad choice being a noisy binding rather than a broken
    /// machine.
    ///
    /// That argument was right about the machine and wrong about the app. Not swallowing is
    /// exactly what makes these two unusable here: every left click in the reader selects a
    /// word, and every right click marks one Known, so binding either means a capture fires on
    /// top of a reading gesture the user made for a different reason. There is no click left
    /// over to give them. The other three carry nothing in this interface.
    ///
    /// They still deserialize, because a settings file written before this rule existed names
    /// one — [`Settings::normalise`] is what retires it.
    pub fn bindable(self) -> bool {
        !matches!(self, MouseButton::Left | MouseButton::Right)
    }

    /// How it is written in the UI.
    pub fn label(self) -> &'static str {
        match self {
            MouseButton::Left => "Left click",
            MouseButton::Middle => "Middle click",
            MouseButton::Right => "Right click",
            MouseButton::X1 => "Mouse 4 (back)",
            MouseButton::X2 => "Mouse 5 (forward)",
        }
    }
}

/// How many words are at each status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub seen: usize,
    pub known: usize,
    pub ignored: usize,
}

/// Why the store could not be read or written.
#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    BadFormat(String),
    /// The write was **declined**, and nothing on disk was touched.
    ///
    /// Distinct from the two above, and structurally rather than by wording: those are a write
    /// that went wrong, this is a write the store refused in order to protect what is already
    /// there. Two cases reach it — a settings file from a newer build, which saving would
    /// silently downgrade, and an unpaid migration backup, where overwriting would destroy an
    /// original nothing has preserved.
    ///
    /// **It exists so callers can tell without reading the message.** The interface has to say
    /// something different for "this session cannot save at all" than for "this file is being
    /// protected", and deciding that by matching prose is how the degraded-store banners came to
    /// nest three prefixes deep.
    Refused(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(error) => write!(f, "{error}"),
            StoreError::BadFormat(message) => write!(f, "stored data unreadable: {message}"),
            // Whole already, and never prefixed: the two constructors below write a sentence.
            StoreError::Refused(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        StoreError::Io(error)
    }
}

/// A settings file that was read successfully and preserved unsuccessfully.
#[derive(Debug, Clone)]
struct SettingsDebt {
    /// The version the file declared, which names the backup: `settings.json.v{version}`.
    version: u32,
    /// The bytes as they were on disk, kept so the backup is the original and not a re-serialised
    /// approximation of it.
    original: String,
}

/// Word status and settings, persisted under one directory.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    /// What was set aside on the way in. See [`Store::note`].
    note: Option<String>,
    words: Mutex<HashMap<WordKey, WordStatus>>,
    settings: Mutex<Settings>,
    /// Original extension fields survive saves even when the caller cannot echo them.
    settings_source: Option<String>,
    /// What is known about which words have a recording. A cache — see [`Store::open`].
    audio: Mutex<HashMap<WordKey, bool>>,
    /// A settings file this store read but could not preserve, and still owes a backup to.
    ///
    /// **The backup guarantee used to hold for exactly as long as nobody touched a setting.**
    /// `Store::open` records a failed migration backup as a note and carries on — which is right,
    /// because the settings were readable and the app should start. But `set_settings` then wrote
    /// the envelope with no backup and no retry, so the *first ordinary save* overwrote an
    /// original that had never been preserved.
    ///
    /// So the debt is carried, not just reported. A save pays it first and refuses if it still
    /// cannot.
    settings_debt: Mutex<Option<SettingsDebt>>,
    /// The settings file was written by a newer build, so writing would downgrade it.
    ///
    /// **Refusing is the safe answer, not the cautious one.** This build cannot see the fields a
    /// newer one added, so a write here does not merely lose the version number — it drops
    /// preferences the learner set, with nothing on disk left to recover them from.
    settings_from_the_future: bool,
}

/// Where the audio-availability cache lives.
const AUDIO_FILE: &str = "audio-availability.json";

/// The first version of that cache, which listed only the words with no recording.
///
/// Read on open and folded into the current one. Left on disk rather than deleted: the store
/// removes nothing it did not write this run, and an orphaned 500-byte file is a smaller cost
/// than a delete path that could ever point at the wrong name.
const LEGACY_SILENT_FILE: &str = "silent-audio.json";

/// One word, and whether the audio endpoint has a recording of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioRecord {
    pub key: WordKey,
    pub available: bool,
}

impl Store {
    /// How many words the audio cache remembers before it stops growing.
    ///
    /// Generous, because each entry is two short strings and a flag — but it does need a
    /// ceiling: every word the reader displays is checked now, not only the ones somebody
    /// pressed, so this fills far faster than a cache of presses would.
    const AUDIO_MAX: usize = 4096;

    /// Open the store in `dir`, creating it if it is not there.
    ///
    /// A file that cannot be *parsed* is an error rather than a silent reset: losing
    /// a learner's marked words without saying so would be the worst possible
    /// outcome. A file that is simply absent is a new store.
    pub fn open(dir: &Path) -> Result<Store, StoreError> {
        fs::create_dir_all(dir)?;

        // Each file on its own, and each recovered on its own. Both of those matter.
        //
        // A parse failure used to abort the whole `open`, so the caller got no store at all —
        // and with no store, settings fall back to their defaults and nothing can be written.
        // One unreadable `words.json` therefore looked exactly like every setting in the app
        // resetting itself, which is what it was reported as.
        //
        // So a file that cannot be read is **set aside, never deleted**, and the rest carries
        // on. Refusing to save protects the data on disk, which was the right instinct; it
        // just cannot be the end of it, because the file that is protected is one no version
        // of this app can read, and staying broken forever protects nothing.
        let mut notes = Vec::new();

        let words = match read_optional(&dir.join("words.json"))? {
            Some(json) => match serde_json::from_str::<Vec<WordRecord>>(&json) {
                Ok(records) => records
                    .into_iter()
                    .map(|record| (record.key, record.status))
                    .collect(),
                Err(error) => {
                    notes.push(set_aside(
                        dir,
                        "words.json",
                        &error.to_string(),
                        "word statuses",
                    ));
                    HashMap::new()
                }
            },
            None => HashMap::new(),
        };

        // **Versioned, repaired field by field, and never downgraded.** See `settings_file` for
        // why the version is an envelope rather than a field, and for what each shape costs.
        // Only a document that is not JSON at all is still set aside whole; a file with one
        // value of the wrong shape used to cost every other value in it.
        let raw = read_optional(&dir.join("settings.json"))?;
        let loaded = match &raw {
            Some(json) => match settings_file::read(json) {
                Ok(loaded) => loaded,
                Err(error) => {
                    notes.push(set_aside(
                        dir,
                        "settings.json",
                        &error.to_string(),
                        "settings",
                    ));
                    settings_file::Loaded {
                        settings: Settings::default(),
                        found: settings_file::SETTINGS_VERSION,
                        repaired: Vec::new(),
                    }
                }
            },
            None => settings_file::Loaded {
                settings: Settings::default(),
                found: settings_file::SETTINGS_VERSION,
                repaired: Vec::new(),
            },
        };
        if let Some(note) = settings_file::repair_note(&loaded.repaired) {
            notes.push(note);
        }
        let from_the_future = loaded.from_the_future();
        if from_the_future {
            notes.push(format!(
                "Your settings were written by a newer version of this app (format {}, this one \
                 understands {}). They have been read as far as possible and will not be \
                 overwritten — changes made here cannot be saved until you update.",
                loaded.found,
                settings_file::SETTINGS_VERSION
            ));
        }
        // **Migration runs before `normalise`.** `normalise` is value clamping on load and save
        // and stays that; a migration is a change of shape, and running it afterwards would clamp
        // a value the migration had not yet produced.
        let mut settings_debt = None;
        if loaded.needs_migration() && !from_the_future {
            if let Some(json) = &raw {
                if let Err(error) = migrate_settings(dir, loaded.found, json, &loaded.settings) {
                    // The original is untouched — the backup is written first and the replacement
                    // is atomic — so the app carries on with what it read and tries again next
                    // time. Saying nothing would be worse: an unmigrated file is a file the next
                    // migration still has to handle.
                    notes.push(format!(
                        "Your settings could not be updated to the current format ({error}). They \
                         were read successfully and nothing was changed on disk."
                    ));
                    settings_debt = Some(SettingsDebt {
                        version: loaded.found,
                        original: json.clone(),
                    });
                }
            }
        }
        let mut settings = loaded.settings;
        settings.normalise();

        // **Set aside on failure is deliberately *not* what happens here.**
        //
        // The other two files hold things a learner made — marked words, chosen settings — and
        // losing either silently would be the worst outcome, so a broken one is preserved and
        // reported. This file holds only what was learned by asking a server, and it refills
        // itself the next time those words are displayed. Reporting "some of your data was
        // recovered" for a cache would spend a real warning on nothing, and a warning is worth
        // only what it is spent on. So an unreadable one is simply empty.
        let mut audio: HashMap<WordKey, bool> = read_optional(&dir.join(AUDIO_FILE))
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<Vec<AudioRecord>>(&json).ok())
            .map(|records| {
                records
                    .into_iter()
                    .take(Store::AUDIO_MAX)
                    .map(|record| (record.key, record.available))
                    .collect()
            })
            .unwrap_or_default();

        // The first version of this cache listed only the words with *no* recording, because
        // that was all it needed: the button was drawn until a press proved otherwise. Now that
        // availability is checked before the button appears, a hit is worth keeping too — so the
        // old file is folded in as the misses it always was.
        //
        // `or_insert`, never overwrite: anything already in the new file was learned later and
        // by a better method, and a stale "no" must not win over a fresh "yes".
        if let Ok(Some(json)) = read_optional(&dir.join(LEGACY_SILENT_FILE)) {
            if let Ok(keys) = serde_json::from_str::<Vec<WordKey>>(&json) {
                for key in keys.into_iter().take(Store::AUDIO_MAX) {
                    audio.entry(key).or_insert(false);
                }
            }
        }

        Ok(Store {
            root: dir.to_path_buf(),
            words: Mutex::new(words),
            settings: Mutex::new(settings),
            settings_source: raw
                .filter(|text| serde_json::from_str::<serde_json::Value>(text).is_ok()),
            audio: Mutex::new(audio),
            note: (!notes.is_empty()).then(|| notes.join(" ")),
            settings_debt: Mutex::new(settings_debt),
            settings_from_the_future: from_the_future,
        })
    }

    /// Whether this store refuses to write settings because the file on disk is newer.
    pub fn settings_are_read_only(&self) -> bool {
        self.settings_from_the_future
    }

    /// What is known about which words have a recording.
    ///
    /// The interface reads this at startup, so a word checked in an earlier session is never
    /// asked about again. That is the whole point of the cache: the endpoint is asked once per
    /// word ever, rather than once per run.
    pub fn audio_known(&self) -> Vec<AudioRecord> {
        let mut records: Vec<AudioRecord> = self
            .audio
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(key, available)| AudioRecord {
                key: key.clone(),
                available: *available,
            })
            .collect();
        records.sort_by(|left, right| {
            (&left.key.term, &left.key.reading).cmp(&(&right.key.term, &right.key.reading))
        });
        records
    }

    /// Record whether a word has a recording.
    ///
    /// Failing to write is not worth failing the caller over: the value stays in memory either
    /// way, so the session still benefits and the only cost is asking again next run.
    pub fn remember_audio(&self, key: WordKey, available: bool) -> Result<(), StoreError> {
        // The lock covers the file replacement as well as the mutation, for the reason
        // [`Store::set_status`] gives. Releasing it before serialising let two threads take
        // different snapshots and write them in either order, so the later write dropped
        // whatever the earlier one had added — and a word lost from this file is re-probed
        // against someone else's server on every later launch. `word_audio` is an async command
        // and `audio_availability` runs beside it, so this is two real threads, not a hypothesis.
        let mut known = self
            .audio
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if known.len() >= Store::AUDIO_MAX && !known.contains_key(&key) {
            return Ok(());
        }
        // Already knew, and knew the same thing. Nothing to write.
        if known.insert(key, available) == Some(available) {
            return Ok(());
        }
        let records = known
            .iter()
            .map(|(key, available)| AudioRecord {
                key: key.clone(),
                available: *available,
            })
            .collect::<Vec<_>>();
        // Unlike `set_status`, a failed write is deliberately not rolled back: this is a cache of
        // answers, the answer is still true, and keeping it means the session stops asking.
        save_audio(&self.root, &records)
    }

    /// What had to be recovered when this store was opened, if anything.
    ///
    /// A sentence to show the reader, not an error: the store works. It exists so that
    /// setting a file aside is never silent — a learner whose marks have gone should be told
    /// where they went, not left to notice.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The status of one word. Never fails: an unmarked word is [`WordStatus::New`].
    pub fn status(&self, key: &WordKey) -> WordStatus {
        self.words
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .copied()
            .unwrap_or_default()
    }

    /// The statuses for a whole passage, in order.
    ///
    /// Batched because the reader needs one per token and taking the lock per word
    /// would be pointless work.
    pub fn statuses(&self, keys: &[WordKey]) -> Vec<WordStatus> {
        let words = self
            .words
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        keys.iter()
            .map(|key| words.get(key).copied().unwrap_or_default())
            .collect()
    }

    /// Mark a word, and persist immediately.
    ///
    /// Marking back to [`WordStatus::New`] removes the record rather than storing a
    /// default, so the file stays the size of what the learner has actually done.
    pub fn set_status(&self, key: WordKey, status: WordStatus) -> Result<(), StoreError> {
        // The lock covers the file replacement as well as the mutation. Releasing it before
        // serialising allowed two command threads to take different snapshots and then race on
        // the same temporary file. It also let readers observe a value a failed disk write later
        // had to retract.
        let mut words = self
            .words
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = words.get(&key).copied();
        if status == WordStatus::New {
            words.remove(&key);
        } else {
            words.insert(key.clone(), status);
        }

        if let Err(error) = save_words(&self.root, &words) {
            match previous {
                Some(previous) => {
                    words.insert(key, previous);
                }
                None => {
                    words.remove(&key);
                }
            }
            return Err(error);
        }

        Ok(())
    }

    pub fn counts(&self) -> Counts {
        let words = self
            .words
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut counts = Counts::default();
        for status in words.values() {
            match status {
                WordStatus::Seen => counts.seen += 1,
                WordStatus::Known => counts.known += 1,
                WordStatus::Ignored => counts.ignored += 1,
                WordStatus::New => {}
            }
        }
        counts
    }

    /// The directory this store lives in.
    ///
    /// For things kept *beside* the settings rather than in them — the looked-up words, so
    /// far. Asking the store rather than recomputing the path means anything holding a store
    /// in a temp directory, as the tests do, is not quietly writing into the real one.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Save settings, unless the file on disk was written by a newer build.
    ///
    /// **The refusal is a real error and not a silent no-op**, because the interface has to be
    /// able to tell the learner why the switch they just moved did not stay moved.
    pub fn set_settings(&self, mut settings: Settings) -> Result<(), StoreError> {
        if self.settings_from_the_future {
            return Err(StoreError::Refused(format!(
                "the settings file was written by a newer version of this app (format above {}),                  so saving would discard settings this build cannot see",
                settings_file::SETTINGS_VERSION
            )));
        }
        // **Pay any outstanding backup before overwriting the file it protects.** If the
        // backup still cannot be written the save is refused and nothing is touched, which is
        // the whole point: an original nothing has preserved must not be replaced by a save.
        {
            let mut debt = self
                .settings_debt
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(owed) = debt.clone() {
                // Classified as a refusal rather than passed through as the I/O error it is: what
                // happened to the caller is that the save did not happen and the file is intact,
                // which is a different thing from a write that failed halfway.
                back_up_original(&self.root, owed.version, &owed.original).map_err(|error| {
                    StoreError::Refused(format!(
                        "the settings file from before this version could not be backed up first,                          so saving would overwrite an original nothing has preserved: {error}"
                    ))
                })?;
                *debt = None;
            }
        }
        settings.normalise();
        let contents = settings_file::write_preserving(&settings, self.settings_source.as_deref())
            .map_err(io_error)?;
        let mut kept = self
            .settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Publish the in-memory value only after the durable replacement succeeds. A caller
        // receiving an error can therefore trust that the Store still reports its old value.
        write_atomically(&self.root.join("settings.json"), &contents)?;
        *kept = settings;
        Ok(())
    }
}

/// Write the audio-availability cache.
///
/// Sorted for the same reason `save_words` sorts: a `HashMap`'s iteration order would reshuffle
/// the whole file on every write, and a cache nobody can read a diff of is one nobody can check.
fn save_audio(root: &Path, records: &[AudioRecord]) -> Result<(), StoreError> {
    let mut sorted = records.to_vec();
    sorted.sort_by(|left, right| {
        (&left.key.term, &left.key.reading).cmp(&(&right.key.term, &right.key.reading))
    });
    write_atomically(
        &root.join(AUDIO_FILE),
        serde_json::to_string_pretty(&sorted)
            .map_err(|error| StoreError::BadFormat(error.to_string()))?
            .as_bytes(),
    )
}

fn save_words(root: &Path, words: &HashMap<WordKey, WordStatus>) -> Result<(), StoreError> {
    let mut records: Vec<WordRecord> = words
        .iter()
        .map(|(key, status)| WordRecord {
            key: key.clone(),
            status: *status,
        })
        .collect();
    // Sorted so the file has a stable order and diffs are readable — a
    // HashMap's iteration order would reshuffle it on every save.
    records.sort_by(|left, right| {
        (&left.key.term, &left.key.reading).cmp(&(&right.key.term, &right.key.reading))
    });
    write_atomically(
        &root.join("words.json"),
        &serde_json::to_vec_pretty(&records).map_err(io_error)?,
    )
}

fn io_error(error: serde_json::Error) -> StoreError {
    StoreError::BadFormat(error.to_string())
}

fn read_optional(path: &Path) -> Result<Option<String>, StoreError> {
    match fs::read_to_string(path) {
        Ok(json) => Ok(Some(json)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Move an unreadable file out of the way and say so.
///
/// Renamed rather than removed, always. The contents may be a learner's only copy of
/// something — a partial write, a file from a newer version, or a bug in this app writing the
/// wrong shape — and none of those are ours to throw away. Whatever is wrong with it, it is
/// still theirs.
///
/// A fixed suffix rather than a timestamped one: the interesting file is the original, and a
/// directory that accumulates `words.json.unreadable.3` is a directory nobody reads. If one
/// is already set aside, the newer failure keeps the older file — the first failure is the one
/// with the real data in it.
fn set_aside(dir: &Path, name: &str, why: &str, what: &str) -> String {
    let from = dir.join(name);
    let to = dir.join(format!("{name}.unreadable"));
    if to.exists() {
        return format!("Your saved {what} could not be read ({why}), so they have been left as they are and a new list started.");
    }
    match fs::rename(&from, &to) {
        Ok(()) => format!(
            "Your saved {what} could not be read ({why}). The file has been set aside as {name}.unreadable and a new one started — nothing was deleted."
        ),
        Err(error) => format!(
            "Your saved {what} could not be read ({why}), and the file could not be moved out of the way either ({error})."
        ),
    }
}

/// Bring `settings.json` up to the current format, backing up the original first.
///
/// # The order is the guarantee
///
/// 1. Copy the original aside as `settings.json.v{found}`. Nothing is written over.
/// 2. Serialise the migrated settings and write them through [`write_atomically`].
///
/// An interruption anywhere leaves either the original file or the complete migrated one, never
/// half of either, because step 2 is a rename. If step 1 fails, step 2 never runs — a migration
/// with no backup is not one this store performs.
///
/// **Re-running is safe and does nothing.** After a successful migration the file declares the
/// current version, so `Loaded::needs_migration` is false and this is not called again. That is
/// why an interrupted run can simply be retried on the next open rather than needing recovery
/// state of its own.
///
/// The backup is kept for the same reason `set_aside` keeps its file: it is the learner's, and
/// a fixed name rather than a timestamped one means the directory stays readable. An existing
/// backup is never overwritten — the first one has the original in it.
fn migrate_settings(
    dir: &Path,
    found: u32,
    original: &str,
    settings: &Settings,
) -> Result<(), StoreError> {
    back_up_original(dir, found, original)?;
    let contents = settings_file::write_preserving(settings, Some(original)).map_err(io_error)?;
    write_atomically(&dir.join("settings.json"), &contents)
}

/// Preserve the settings file as it was, before anything writes over it.
///
/// **`is_file`, not `exists`.** A directory sitting at that path is not a backup, and treating it
/// as one would let the caller proceed having preserved nothing — which is the single outcome this
/// exists to prevent. Writing then fails, the original stays as it is, and the next attempt tries
/// again.
///
/// An existing backup is never overwritten, for the same reason `set_aside` keeps its file: the
/// first one has the original in it.
fn back_up_original(dir: &Path, found: u32, original: &str) -> Result<(), StoreError> {
    let backup = dir.join(format!("settings.json.v{found}"));
    if backup.is_file() {
        return Ok(());
    }
    write_atomically(&backup, original.as_bytes())
}

/// Write via a temporary file and rename over the target, so an interrupted save
/// cannot truncate what was already there. `fs::rename` replaces the destination on
/// Windows as well as Unix.
pub(crate) fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), StoreError> {
    static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

    // The counter matters even though each Store serialises its own writes: tests, migration
    // tools, and a future helper process may open the same directory independently. A fixed
    // `words.tmp` lets those otherwise unrelated writers truncate one another's staging file.
    let id = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp.{}.{id}", std::process::id()));
    let outcome: Result<(), StoreError> = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();

    if outcome.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    outcome
}

#[cfg(test)]
mod tests {

    /// `"learning"` still reads, and never writes.
    ///
    /// **This is the whole of the exception the generated bindings document.** `WordStatus::Seen`
    /// shipped as `Learning`, so stores written before the rename hold `"learning"`; the alias is
    /// what stops `Store::open` failing the whole `Vec<WordRecord>` and setting every word status
    /// aside. ts-rs cannot express an alias and reports it as an attribute it could not parse —
    /// and **ignoring it is correct**, because the generated union describes what the app *emits*
    /// and the app has not emitted `"learning"` since the rename.
    ///
    /// That argument is only sound while both halves hold, so both are asserted here rather than
    /// left to the comment: the legacy name is still accepted, and the canonical name is what
    /// comes back. If the second ever stopped being true, the generated
    /// `"new" | "seen" | "known" | "ignored"` would be describing a wire format the app no longer
    /// writes, and the exception would have to go with it.
    #[test]
    fn the_legacy_status_name_is_read_and_never_written() {
        let read: WordStatus = serde_json::from_str("\"learning\"").expect("the alias is accepted");
        assert_eq!(read, WordStatus::Seen);

        let written = serde_json::to_string(&read).expect("serialises");
        assert_eq!(
            written, "\"seen\"",
            "the canonical name is what goes back on the wire"
        );
        assert_ne!(written, "\"learning\"");

        // And the canonical name reads too, which is the ordinary case the alias must not have
        // displaced.
        let canonical: WordStatus = serde_json::from_str("\"seen\"").expect("the current name");
        assert_eq!(canonical, WordStatus::Seen);
    }

    /// **A settings file that still names ichi.moe opens, and keeps everything else in it.**
    ///
    /// This is the whole reason `AnalyzerChoice::IchiMoe` survives the analyzer being withdrawn.
    /// Delete the variant and `"analyzer": "ichimoe"` stops parsing; a settings file that does not
    /// parse is *set aside whole* by `Store::open`, so retiring one dropdown entry would have
    /// taken somebody's palettes, bindings, deck and every other preference with it.
    ///
    /// Two halves, and the second is the one worth having: it loads, *and* the rest survives.
    #[test]
    fn a_file_naming_the_withdrawn_analyzer_opens_on_auto() {
        let mut settings: Settings = serde_json::from_str(
            r#"{"analyzer": "ichimoe", "anki_deck": "Japanese::Reading", "font_size": 17.0,
                "read_clipboard": true, "settings_section": "anki"}"#,
        )
        .expect("a file naming the withdrawn analyzer still parses");
        settings.normalise();

        assert_eq!(
            settings.analyzer,
            AnalyzerChoice::Auto,
            "ichi.moe ran the same Ichiran engine, so Auto is where it lands"
        );
        assert_eq!(
            settings.anki_deck, "Japanese::Reading",
            "the rest of the file survived"
        );
        assert_eq!(settings.font_size, 17.0);
        assert!(settings.read_clipboard);
        assert_eq!(settings.settings_section, "anki");

        // And the stale value is written out: normalise runs on save too, so the next write
        // leaves a file that no longer mentions it.
        let written = serde_json::to_string(&settings).expect("serialises");
        assert!(
            !written.contains("ichimoe"),
            "saving should have replaced the withdrawn value, not kept it"
        );
    }

    /// The choices actually offered are the three that run on this machine.
    #[test]
    fn every_analyzer_offered_is_a_local_one() {
        for choice in [
            AnalyzerChoice::Auto,
            AnalyzerChoice::Ichiran,
            AnalyzerChoice::BuiltIn,
        ] {
            let mut settings = Settings {
                analyzer: choice,
                ..Settings::default()
            };
            settings.normalise();
            assert_eq!(
                settings.analyzer, choice,
                "{choice:?} is offered and must survive"
            );
        }
    }

    /// **A palette kept before decoration was part of one still applies.**
    ///
    /// `NamedTheme::decor` is an `Option` precisely so this works: an older entry has no
    /// decoration, and applying it leaves the watermarks where they are rather than resetting
    /// them to a default the person never chose. Deserialising has to survive the field being
    /// absent, or every palette anybody had saved would be dropped on load.
    #[test]
    fn a_palette_saved_before_decoration_still_loads() {
        let older: NamedTheme = serde_json::from_str(
            r#"{"name": "Dusk", "theme": {"mode": "dark", "surface_hue": 300, "accent_hue": 297}}"#,
        )
        .expect("a palette without decoration is still a palette");
        assert_eq!(older.name, "Dusk");
        assert!(older.decor.is_none(), "and it carries none");
    }

    /// **A palette saved with decoration but before blur existed still loads — with everything
    /// else in the file.**
    ///
    /// This is the regression for the whole `#[serde(default)]` argument on [`ThemeDecor`], and
    /// what makes it worth a test of its own is how far the damage reaches. A missing key fails
    /// `ThemeDecor`, which fails the `NamedTheme` around it, which fails the `Settings` around
    /// that — and [`Store::open`] answers a `Settings` that will not parse with
    /// `Settings::default()`. So one field added without thinking would not lose the palettes.
    /// It would lose the hotkeys, the dictionaries, the deck, and every word status setting in
    /// the file, silently, on next launch.
    ///
    /// Spelled as the JSON a real file holds rather than built from the struct: a struct literal
    /// would be updated along with the field and would stop describing an older file the moment
    /// it mattered.
    #[test]
    fn a_palette_saved_before_blur_still_loads() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "font_size": 15.0,
                "anki_deck": "Reading",
                "theme_presets": [{
                    "name": "Dusk",
                    "theme": {"mode": "dark", "surface_hue": 300, "accent_hue": 297},
                    "decor": {
                        "cloud": 0.1, "cloud_tint": "accent",
                        "sakura": 0.2, "sakura_tint": "blossom",
                        "scale": 1.5, "cloud_pad": 2.0,
                        "cloud_spread": 1.25, "sakura_spread": 0.75,
                        "animations": false, "animation_ms": 120
                    }
                }]
            }"#,
        )
        .expect("a settings file written before blur is still a settings file");

        // The rest of the file, which is what a failure here would really have cost.
        assert_eq!(settings.font_size, 15.0);
        assert_eq!(settings.anki_deck, "Reading");

        let decor = settings.theme_presets[0]
            .decor
            .as_ref()
            .expect("the decoration came back");
        assert_eq!(decor.cloud, 0.1, "and it kept what it did say");
        assert_eq!(decor.animation_ms, 120);
        assert!(!decor.blur, "the softness it never had is off");
        assert_eq!(
            decor.cloud_blur, 3.0,
            "at the same figure a fresh install has"
        );
    }

    /// **A settings file written before the decoration switches shows both decorations.**
    ///
    /// Before 2026-10-19 the opacity was the only control, so every file on disk is one whose
    /// clouds and sakura were drawn whenever their opacity said so. A missing switch has to read
    /// as on, or the update would quietly take somebody's watermarks away. Spelled as JSON for
    /// the reason `a_palette_saved_before_blur_still_loads` gives.
    #[test]
    fn a_file_from_before_the_decoration_switches_shows_both() {
        let settings: Settings = serde_json::from_str(
            r#"{ "decor_cloud": 0.11, "decor_sakura": 0.3, "anki_deck": "Reading" }"#,
        )
        .expect("a file without the switches is still a settings file");
        assert!(settings.decor_cloud_on, "the clouds stay on");
        assert!(settings.decor_sakura_on, "and so does the sakura");
        assert_eq!(settings.decor_cloud, 0.11, "at the strength it already had");
        assert_eq!(settings.decor_sakura, 0.3);
        assert_eq!(settings.anki_deck, "Reading");
    }

    /// **The fresh-install look is the one the maintainer tuned**, set as the default on
    /// 2026-10-19. Pinned so it changes only by a decision, not by a stray edit to the list.
    #[test]
    fn a_fresh_install_has_the_tuned_decoration() {
        let fresh = Settings::default();
        assert!(fresh.decor_cloud_on && fresh.decor_sakura_on, "both shown");
        assert_eq!(fresh.decor_cloud, 0.02);
        assert_eq!(fresh.decor_cloud_tint, DecorTint::Ink);
        assert_eq!(fresh.decor_sakura, 0.2);
        assert_eq!(fresh.decor_sakura_tint, DecorTint::Blossom);
        assert_eq!(fresh.decor_scale, 1.5);
        assert_eq!(fresh.decor_cloud_pad, 1.5);
        assert_eq!(fresh.decor_cloud_spread, 0.5);
        assert_eq!(fresh.decor_sakura_spread, 0.7);
        assert!(fresh.decor_blur);
        assert_eq!(fresh.decor_cloud_blur, 3.0);
        assert_eq!(fresh.decor_sakura_blur, 0.0);
    }

    /// **The two lists of decoration defaults agree.**
    ///
    /// [`ThemeDecor::default`] exists to fill fields missing from a stored palette, and
    /// [`Settings::default`] is what a fresh install gets. They are the same set of values
    /// written out twice, which is the shape of drift this file keeps finding: let them part and
    /// an older palette applies a watermark subtly unlike the one a new user sees, with nothing
    /// on screen to say why.
    #[test]
    fn a_palette_defaults_to_what_a_fresh_install_has() {
        let fresh = Settings::default();
        let decor = ThemeDecor::default();
        assert_eq!(decor.cloud, fresh.decor_cloud);
        assert_eq!(decor.cloud_tint, fresh.decor_cloud_tint);
        assert_eq!(decor.sakura, fresh.decor_sakura);
        assert_eq!(decor.sakura_tint, fresh.decor_sakura_tint);
        assert_eq!(decor.scale, fresh.decor_scale);
        assert_eq!(decor.cloud_pad, fresh.decor_cloud_pad);
        assert_eq!(decor.cloud_spread, fresh.decor_cloud_spread);
        assert_eq!(decor.sakura_spread, fresh.decor_sakura_spread);
        // The one value that differs, on purpose: see `ThemeDecor::default`.
        assert!(
            !decor.blur,
            "a palette missing `blur` predates softness, so it has none"
        );
        assert_eq!(decor.cloud_blur, fresh.decor_cloud_blur);
        assert_eq!(decor.sakura_blur, fresh.decor_sakura_blur);
        assert_eq!(decor.animations, fresh.animations);
        assert_eq!(decor.animation_ms, fresh.animation_ms);
    }

    /// Decoration inside a palette is held to the same ranges as the live settings.
    ///
    /// Applying a palette writes straight into those settings, so a hand-edited file could
    /// otherwise put a watermark somewhere none of the sliders can reach — and the way back would
    /// be to edit the file again.
    #[test]
    fn decoration_inside_a_palette_is_clamped_like_the_live_one() {
        let wild = ThemeDecor {
            cloud: 9.0,
            cloud_tint: DecorTint::Ink,
            sakura: -4.0,
            sakura_tint: DecorTint::Ink,
            scale: 40.0,
            cloud_pad: -1.0,
            cloud_spread: 99.0,
            sakura_spread: f64::NAN,
            blur: true,
            cloud_blur: 400.0,
            sakura_blur: f64::NEG_INFINITY,
            animations: true,
            animation_ms: 99_999,
        };
        let mut settings = Settings {
            theme_presets: vec![NamedTheme {
                name: "Wild".into(),
                theme: Theme::default(),
                decor: Some(wild),
            }],
            ..Settings::default()
        };
        settings.normalise();

        let kept = settings.theme_presets[0]
            .decor
            .as_ref()
            .expect("the decoration survived");
        assert_eq!(kept.cloud, 0.16, "held to the top of the slider");
        assert_eq!(kept.sakura, 0.0, "and the bottom");
        assert_eq!(kept.scale, 1.8);
        assert_eq!(kept.cloud_pad, 0.0);
        assert_eq!(kept.cloud_spread, 2.5);
        assert_eq!(
            kept.sakura_spread, 1.0,
            "not a number falls back rather than poisoning"
        );
        assert_eq!(kept.cloud_blur, 8.0, "softness has a ceiling like the rest");
        assert_eq!(
            kept.sakura_blur, 1.0,
            "and an infinity falls back rather than reaching the filter"
        );
        assert_eq!(kept.animation_ms, 400);
    }

    /// A palette round-trips through the store with its decoration intact.
    #[test]
    fn a_palette_keeps_its_decoration_across_a_reopen() {
        let scratch = Scratch::new("preset-decor");
        let decor = ThemeDecor {
            cloud: 0.1,
            cloud_tint: DecorTint::Accent,
            sakura: 0.2,
            sakura_tint: DecorTint::Blossom,
            scale: 1.5,
            cloud_pad: 2.0,
            cloud_spread: 1.25,
            sakura_spread: 0.75,
            blur: true,
            cloud_blur: 4.5,
            sakura_blur: 2.5,
            animations: false,
            animation_ms: 240,
        };
        {
            let store = Store::open(scratch.path()).expect("open");
            let mut settings = store.settings();
            settings.theme_presets = vec![NamedTheme {
                name: "Dusk".into(),
                theme: Theme::default(),
                decor: Some(decor.clone()),
            }];
            store.set_settings(settings).expect("save");
        }
        let reopened = Store::open(scratch.path()).expect("reopen");
        let saved = &reopened.settings().theme_presets[0];
        assert_eq!(saved.name, "Dusk");
        assert_eq!(
            saved.decor.as_ref().expect("decoration came back"),
            &decor,
            "every value survived, including the two tints and the animation switch"
        );
    }

    /// **The sidecar's console stays hidden unless somebody asks for it.**
    ///
    /// Ichiran runs through WSL, which is a console program, so without this a black window
    /// flashes up on every single analysis — of a console that is empty, because its output is
    /// captured and shown in the app. There is no reading of that behaviour where it is wanted,
    /// which is why this is the one default that changes behaviour rather than preserving it.
    ///
    /// Asserted in three places at once, because "the default is on" can be true of the struct
    /// and false of what a real install ends up with:
    ///
    ///   - `Settings::default()`, which is what a fresh install gets,
    ///   - a settings file written before the field existed, which `#[serde(default)]` fills,
    ///   - and a file that names every other field but not this one.
    #[test]
    fn the_analyzer_console_is_hidden_by_default() {
        assert!(Settings::default().quiet, "a fresh install hides it");

        let empty: Settings = serde_json::from_str("{}").expect("an empty object is settings");
        assert!(
            empty.quiet,
            "a settings file from before the field existed hides it"
        );

        let older: Settings =
            serde_json::from_str(r#"{"read_clipboard": true, "log": true, "font_size": 16.0}"#)
                .expect("a partial file is settings");
        assert!(
            older.quiet,
            "a file naming other fields but not this one still hides it"
        );

        // And the opposite is reachable, or the switch in Diagnostics does nothing.
        let asked: Settings = serde_json::from_str(r#"{"quiet": false}"#).expect("settings");
        assert!(!asked.quiet, "asking for the console gets the console");
    }

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            // **The process id is load-bearing, not decoration.** Two `cargo test` runs
            // overlapping — a workspace run started while a package run is still going, which is
            // ordinary during development — shared these paths exactly, and each one's
            // `remove_dir_all` deleted the other's fixture mid-import. It surfaced as
            // `PermissionDenied` from a different test every time, which reads like a real fault
            // and is not one. `log.rs` and `corpus.rs` already keyed on the pid; these did not.
            let path = std::env::temp_dir()
                .join(format!("sakatsu-store-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Scratch(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn an_unmarked_word_is_new_and_a_marked_one_survives_reopening() {
        let scratch = Scratch::new("roundtrip");
        let taberu = WordKey::new("食べる", "たべる");

        let store = Store::open(scratch.path()).expect("open");
        assert_eq!(store.status(&taberu), WordStatus::New);

        store
            .set_status(taberu.clone(), WordStatus::Seen)
            .expect("set");
        assert_eq!(store.status(&taberu), WordStatus::Seen);

        // Reopened from disk, not from memory.
        let reopened = Store::open(scratch.path()).expect("reopen");
        assert_eq!(reopened.status(&taberu), WordStatus::Seen);
    }

    #[test]
    fn concurrent_marks_are_one_transaction_each_and_all_survive_reopening() {
        use std::sync::{Arc, Barrier};

        const WRITERS: usize = 24;
        let scratch = Scratch::new("concurrent-marks");
        let store = Arc::new(Store::open(scratch.path()).expect("open"));
        let start = Arc::new(Barrier::new(WRITERS + 1));
        let mut threads = Vec::new();

        for index in 0..WRITERS {
            let store = Arc::clone(&store);
            let start = Arc::clone(&start);
            threads.push(std::thread::spawn(move || {
                start.wait();
                store
                    .set_status(
                        WordKey::new(format!("word-{index}"), format!("reading-{index}")),
                        WordStatus::Seen,
                    )
                    .expect("concurrent write");
            }));
        }

        start.wait();
        for thread in threads {
            thread.join().expect("writer panicked");
        }

        let reopened = Store::open(scratch.path()).expect("reopen");
        assert_eq!(reopened.counts().seen, WRITERS);
        for index in 0..WRITERS {
            assert_eq!(
                reopened.status(&WordKey::new(
                    format!("word-{index}"),
                    format!("reading-{index}")
                )),
                WordStatus::Seen
            );
        }
    }

    #[test]
    fn a_failed_persist_does_not_publish_the_unwritten_value() {
        let scratch = Scratch::new("failed-transaction");
        let root = scratch.path().to_path_buf();
        let store = Store::open(&root).expect("open");
        let key = WordKey::new("水", "みず");

        fs::remove_dir_all(&root).expect("remove backing directory");
        fs::write(&root, b"this path is no longer a directory").expect("block the path");

        assert!(
            store.set_status(key.clone(), WordStatus::Known).is_err(),
            "the replacement cannot succeed when the store root is a file"
        );
        assert_eq!(
            store.status(&key),
            WordStatus::New,
            "memory must not claim a value the file did not accept"
        );

        fs::remove_file(&root).expect("remove blocker");
    }

    #[test]
    fn homographs_are_tracked_separately() {
        // The reason the key carries a reading: marking 上手 as known should not claim
        // the learner knows a word they have never met.
        let scratch = Scratch::new("homograph");
        let store = Store::open(scratch.path()).expect("open");

        let jouzu = WordKey::new("上手", "じょうず");
        let uwate = WordKey::new("上手", "うわて");
        store
            .set_status(jouzu.clone(), WordStatus::Known)
            .expect("set");

        assert_eq!(store.status(&jouzu), WordStatus::Known);
        assert_eq!(store.status(&uwate), WordStatus::New);
    }

    #[test]
    fn marking_a_word_back_to_new_forgets_it_rather_than_storing_a_default() {
        let scratch = Scratch::new("forget");
        let store = Store::open(scratch.path()).expect("open");
        let key = WordKey::new("水", "みず");

        store
            .set_status(key.clone(), WordStatus::Known)
            .expect("set");
        assert_eq!(store.counts().known, 1);

        store
            .set_status(key.clone(), WordStatus::New)
            .expect("unset");
        assert_eq!(store.counts(), Counts::default());
        let json = fs::read_to_string(scratch.path().join("words.json")).expect("read");
        assert!(!json.contains('水'), "the record should be gone: {json}");
    }

    #[test]
    fn statuses_come_back_in_the_order_asked_for() {
        let scratch = Scratch::new("batch");
        let store = Store::open(scratch.path()).expect("open");
        store
            .set_status(WordKey::new("食べる", "たべる"), WordStatus::Known)
            .expect("set");

        let keys = [
            WordKey::new("気持ち", "きもち"),
            WordKey::new("食べる", "たべる"),
            WordKey::new("水", "みず"),
        ];
        assert_eq!(
            store.statuses(&keys),
            [WordStatus::New, WordStatus::Known, WordStatus::New]
        );
    }

    #[test]
    fn a_token_keys_on_its_dictionary_form_and_falls_back_to_its_surface() {
        let conjugated = Token {
            surface: "食べさせられて".into(),
            dict_form: "食べる".into(),
            lemma_reading: "たべる".into(),
            ..Default::default()
        };
        assert_eq!(
            WordKey::for_token(&conjugated),
            WordKey::new("食べる", "たべる")
        );

        // Punctuation and anything else the analyzer could not resolve.
        let bare = Token {
            surface: "ヷ".into(),
            ..Default::default()
        };
        assert_eq!(WordKey::for_token(&bare), WordKey::new("ヷ", ""));
    }

    /// The tint cannot be had without the frame leaving the shot, and the store is where
    /// that is guaranteed — not the checkbox that happens to set it.
    #[test]
    fn a_region_tint_forces_the_frame_out_of_the_shot() {
        let mut settings = Settings {
            frame_tint: 0.2,
            hide_during_capture: false,
            ..Settings::default()
        };
        settings.normalise();
        assert!(
            settings.hide_during_capture,
            "a tint the frame is still wearing during a capture is a tint in the OCR"
        );

        // A hand-edited file is the case this exists for: nothing in the interface ran.
        let mut edited = Settings {
            frame_tint: 0.5,
            hide_during_capture: false,
            ..Settings::default()
        };
        edited.normalise();
        assert!(edited.hide_during_capture);
    }

    /// A settings file written before the sidecar setting existed gets it **on**, like a fresh one.
    ///
    /// **The regression is a field-level `#[serde(default)]`, which this field used to carry.**
    /// A field-level attribute overrides the struct-level one and fills a missing `bool` from
    /// `bool::default()` — `false` — so flipping [`Settings::default`] alone would have reached
    /// new installs and silently skipped everyone who already had the app. Two readers following
    /// the same instructions, one of them roughly three times slower per passage, with nothing on
    /// screen to explain the difference.
    #[test]
    fn a_file_written_before_the_sidecar_setting_still_gets_the_default() {
        let older = serde_json::json!({ "anki_deck": "Japanese" });
        let settings: Settings = serde_json::from_value(older).expect("an older file must load");
        assert!(
            settings.ichiran_server,
            "a missing bool takes Settings::default(), not bool::default()"
        );
        assert_eq!(
            settings.ichiran_server,
            Settings::default().ichiran_server,
            "an older file and a fresh install must agree"
        );
    }

    /// A settings file written before the deck filter existed gets it **on**, like a fresh one.
    ///
    /// The trap this guards is that a bare `#[serde(default)]` fills a missing `bool` with
    /// `false` rather than with what `Settings::default` says. Everyone who had already used the
    /// app would have loaded with the filter off while a new install got it on — two people
    /// following the same instructions, looking at different screens, with nothing to explain it.
    #[test]
    fn a_half_written_theme_costs_the_theme_and_not_the_settings_file() {
        // The failure this guards is silent and total: a `Theme` that will not parse fails
        // `Settings`, and `Store::open` answers an unparseable `Settings` with
        // `Settings::default()` — so one absent hue takes the hotkeys, the deck and every
        // preference with it. Hand-edited files and field renames both produce exactly this.
        let truncated = serde_json::json!({
            "anki_deck": "Japanese",
            "theme": { "mode": "light" },
            "theme_presets": [{ "name": "kept" }],
        });
        let settings: Settings =
            serde_json::from_value(truncated).expect("a partial theme must still load");

        assert_eq!(
            settings.anki_deck, "Japanese",
            "the rest of the file has to survive a theme that does not parse"
        );
        assert_eq!(
            settings.theme.mode,
            ThemeMode::Light,
            "and keep what it did say"
        );
        assert_eq!(
            settings.theme.surface_hue,
            Theme::default().surface_hue,
            "filling only what was missing"
        );
        assert_eq!(
            settings.theme_presets.len(),
            1,
            "a saved palette survives too"
        );
    }

    #[test]
    fn a_settings_file_from_before_the_deck_filter_still_arrives_with_it_on() {
        // Deliberately not a `Settings { .. }` literal: the field being *absent from the JSON* is
        // the whole case, and a literal cannot express that.
        let older = serde_json::json!({ "anki_deck": "Japanese" });
        let settings: Settings = serde_json::from_value(older).expect("an older file still loads");

        assert!(
            settings.anki_our_decks_only,
            "an older settings file must inherit the current default, not `bool::default()`"
        );
        assert_eq!(settings.anki_deck, "Japanese", "and keep what it did carry");
        assert_eq!(
            settings.anki_our_decks_only,
            Settings::default().anki_our_decks_only,
            "and inherit it from `Settings::default` specifically, which is the mechanism              `#[serde(default)]` at struct level provides"
        );
    }

    #[test]
    fn concurrent_audio_answers_all_survive() {
        use std::sync::{Arc, Barrier};

        // The mirror of `concurrent_marks`, for the cache rather than the marks. Before the lock
        // covered the write, each thread snapshotted the map and serialised it after releasing
        // the lock, so the last writer's older snapshot overwrote everything added meanwhile —
        // and every word lost here is re-probed against someone else's server on each later run.
        //
        // One-sided by nature: holding the lock makes this deterministic, so a pass is real. A
        // regression may not lose a word on every run, but it cannot make this fail spuriously.
        const WRITERS: usize = 24;
        let scratch = Scratch::new("concurrent-audio");
        let store = Arc::new(Store::open(scratch.path()).expect("open"));
        let start = Arc::new(Barrier::new(WRITERS + 1));
        let mut threads = Vec::new();

        for index in 0..WRITERS {
            let store = Arc::clone(&store);
            let start = Arc::clone(&start);
            threads.push(std::thread::spawn(move || {
                start.wait();
                store
                    .remember_audio(
                        WordKey::new(format!("word-{index}"), format!("reading-{index}")),
                        index % 2 == 0,
                    )
                    .expect("concurrent write");
            }));
        }

        start.wait();
        for thread in threads {
            thread.join().expect("writer panicked");
        }

        let known = Store::open(scratch.path()).expect("reopen").audio_known();
        assert_eq!(
            known.len(),
            WRITERS,
            "every answer has to reach the file, not just the last thread's snapshot"
        );
        for index in 0..WRITERS {
            let key = WordKey::new(format!("word-{index}"), format!("reading-{index}"));
            let record = known
                .iter()
                .find(|record| record.key == key)
                .unwrap_or_else(|| panic!("word-{index} was lost from the cache"));
            assert_eq!(record.available, index % 2 == 0, "and keep its answer");
        }
    }

    #[test]
    fn what_is_known_about_a_word_s_audio_survives_a_restart() {
        let scratch = Scratch::new("audio-cache");
        let store = Store::open(scratch.path()).expect("open");
        assert!(store.audio_known().is_empty(), "a new store knows nothing");

        // **Both answers are kept.** Remembering only the misses would mean re-asking about
        // every word that *does* have audio on every run, which is the cost this exists to avoid.
        store
            .remember_audio(WordKey::new("友達", "ともだち"), true)
            .expect("hit");
        store
            .remember_audio(
                WordKey::new("いかがお過ごしでしょうか", "いかがおすごしでしょうか"),
                false,
            )
            .expect("miss");
        store
            .remember_audio(WordKey::new("友達", "ともだち"), true)
            .expect("the same answer twice is not a second entry");

        let known = Store::open(scratch.path()).expect("reopen").audio_known();
        assert_eq!(known.len(), 2, "one entry per distinct word: {known:?}");
        assert!(known
            .iter()
            .any(|record| record.key == WordKey::new("友達", "ともだち") && record.available));
        assert!(known.iter().any(|record| !record.available));
        // Term *and* reading, like every other word key here: 上手 read じょうず is a different
        // word from 上手 read うわて and must not inherit its answer.
        assert!(!known
            .iter()
            .any(|record| record.key == WordKey::new("友達", "とも")));
    }

    #[test]
    fn the_first_cache_of_silent_words_is_folded_into_the_current_one() {
        let scratch = Scratch::new("audio-cache-legacy");
        fs::create_dir_all(scratch.path()).expect("dir");
        fs::write(
            scratch.path().join("silent-audio.json"),
            r#"[{"term":"物は使いよう","reading":"ものはつかいよう"},{"term":"猿","reading":"さる"}]"#,
        )
        .expect("write legacy");
        // Learned later, and by the better method. A stale "no" must not overwrite it.
        fs::write(
            scratch.path().join("audio-availability.json"),
            r#"[{"key":{"term":"猿","reading":"さる"},"available":true}]"#,
        )
        .expect("write current");

        let known = Store::open(scratch.path()).expect("open").audio_known();
        assert_eq!(known.len(), 2, "both files contribute: {known:?}");
        assert!(
            known
                .iter()
                .any(|record| record.key == WordKey::new("猿", "さる") && record.available),
            "the newer answer wins: {known:?}"
        );
        assert!(known.iter().any(|record| record.key
            == WordKey::new("物は使いよう", "ものはつかいよう")
            && !record.available));
    }

    #[test]
    fn an_unreadable_audio_cache_is_simply_empty_and_still_writes() {
        let scratch = Scratch::new("audio-cache-broken");
        // `Scratch` names a directory; `Store::open` is what creates it.
        fs::create_dir_all(scratch.path()).expect("dir");
        fs::write(scratch.path().join("audio-availability.json"), "{ not json").expect("write");

        let store = Store::open(scratch.path()).expect("a broken cache is not a broken store");
        assert!(store.audio_known().is_empty());
        // **No note.** The other two files are set aside and reported because they hold what a
        // learner made; this one refills itself, and spending a real warning on it would train
        // people to ignore the warnings that matter.
        assert!(
            store.note().is_none(),
            "a cache must not report itself as lost data: {:?}",
            store.note()
        );
        store
            .remember_audio(WordKey::new("猿", "さる"), true)
            .expect("mark");
        assert_eq!(
            Store::open(scratch.path())
                .expect("reopen")
                .audio_known()
                .len(),
            1,
            "a broken cache must not stay broken"
        );
    }

    /// A settings file written before the theme existed still opens, on the palette it had.
    #[test]
    fn a_settings_file_from_before_the_theme_arrives_dark() {
        let older = serde_json::json!({ "anki_deck": "Japanese" });
        let settings: Settings = serde_json::from_value(older).expect("an older file still loads");

        // The whole promise of this slice: nobody's app changes colour because the colours became
        // derived. Dark is the default and the default is what an existing file gets.
        assert_eq!(settings.theme.mode, ThemeMode::Dark);
        assert_eq!(settings.theme.surface_hue, 300.0);
        assert_eq!(settings.theme.accent_hue, 297.0);
    }

    /// The dials are held to their ranges, and a hand-set colour that is not one is dropped.
    #[test]
    fn the_palette_dials_and_overrides_are_held_to_what_they_can_mean() {
        let mut settings = Settings {
            theme: Theme {
                tint: 99.0,
                contrast: -4.0,
                depth: f64::NAN,
                overrides: BTreeMap::from([
                    ("accent".into(), "#FF8800".into()),
                    // No sensible repair exists for any of these, so they go rather than being
                    // corrected into a choice nobody made.
                    ("bg".into(), "blue-ish".into()),
                    ("surface".into(), "#abc".into()),
                    ("border".into(), "#12345g".into()),
                ]),
                ..Theme::default()
            },
            ..Settings::default()
        };
        settings.normalise();

        assert_eq!(settings.theme.tint, 2.5, "held to the top of its range");
        assert_eq!(
            settings.theme.contrast, 0.7,
            "held to the bottom of its range"
        );
        assert_eq!(settings.theme.depth, 0.0, "not a number at all falls back");

        assert_eq!(settings.theme.overrides.len(), 1);
        assert_eq!(
            settings.theme.overrides.get("accent").map(String::as_str),
            Some("#FF8800")
        );
    }

    /// A settings file written before the dials existed still opens on the palette it had.
    #[test]
    fn a_theme_from_before_the_dials_reads_as_the_identity() {
        // The defaults for these two are `1.0`, not zero — a bare `#[serde(default)]` would have
        // filled them with `0.0` and flattened every older palette to neutral grey with no
        // contrast, which is the same trap the deck filter's default hit.
        let older = serde_json::json!({
            "theme": { "mode": "dark", "surface_hue": 300.0, "accent_hue": 297.0 }
        });
        let settings: Settings = serde_json::from_value(older).expect("an older file still loads");

        assert_eq!(settings.theme.tint, 1.0);
        assert_eq!(settings.theme.contrast, 1.0);
        assert_eq!(settings.theme.depth, 0.0);
        // Zero is the identity here, unlike the two above: a palette from before the kind wheel
        // could be turned should keep the kind colours it was showing.
        assert_eq!(settings.theme.kind_spin, 0.0);
        assert!(settings.theme.overrides.is_empty());
    }

    /// The kind wheel is a hue, so it wraps rather than refusing what a hand-edited file says.
    #[test]
    fn a_kind_spin_off_the_circle_is_brought_back_onto_it() {
        let mut settings = Settings {
            theme: Theme {
                kind_spin: 400.0,
                ..Theme::default()
            },
            ..Settings::default()
        };
        settings.normalise();
        assert_eq!(settings.theme.kind_spin, 40.0);

        let mut broken = Settings {
            theme: Theme {
                kind_spin: f64::NAN,
                ..Theme::default()
            },
            ..Settings::default()
        };
        broken.normalise();
        assert_eq!(
            broken.theme.kind_spin, 0.0,
            "a value that is not a number falls back rather than poisoning every kind colour"
        );
    }

    /// Saved palettes are checked on the way in, not only when one is applied.
    #[test]
    fn every_saved_palette_is_normalised_and_the_list_is_bounded() {
        let mut settings = Settings {
            theme_presets: vec![
                NamedTheme {
                    name: "  Padded  ".into(),
                    theme: Theme {
                        mode: ThemeMode::Light,
                        surface_hue: 400.0,
                        accent_hue: -30.0,
                        ..Theme::default()
                    },
                    decor: None,
                },
                // A palette nobody can pick is a row in a list that does nothing.
                NamedTheme {
                    name: "   ".into(),
                    theme: Theme::default(),
                    decor: None,
                },
            ],
            ..Settings::default()
        };
        settings.normalise();

        assert_eq!(
            settings.theme_presets.len(),
            1,
            "the nameless one should go"
        );
        let kept = &settings.theme_presets[0];
        assert_eq!(kept.name, "Padded");
        // **Checked here rather than when applied.** A palette is only read on the way out, so one
        // that arrived broken would wait in the file instead of being caught.
        assert_eq!(kept.theme.surface_hue, 40.0);
        assert_eq!(kept.theme.accent_hue, 330.0);
        assert_eq!(
            kept.theme.mode,
            ThemeMode::Light,
            "the mode is the learner's"
        );

        let mut many = Settings {
            theme_presets: (0..Settings::THEME_PRESETS_MAX + 20)
                .map(|n| NamedTheme {
                    name: format!("Palette {n}"),
                    theme: Theme::default(),
                    decor: None,
                })
                .collect(),
            ..Settings::default()
        };
        many.normalise();
        assert_eq!(many.theme_presets.len(), Settings::THEME_PRESETS_MAX);
    }

    /// A saved palette survives a restart, and forgetting one does not disturb the live theme.
    #[test]
    fn saved_palettes_round_trip_and_are_independent_of_the_live_one() {
        let scratch = Scratch::new("theme-presets");
        let store = Store::open(scratch.path()).expect("open");

        let mine = NamedTheme {
            name: "Dusk".into(),
            theme: Theme {
                mode: ThemeMode::Light,
                surface_hue: 20.0,
                accent_hue: 200.0,
                ..Theme::default()
            },
            decor: None,
        };
        let mut settings = store.settings();
        settings.theme = mine.theme.clone();
        settings.theme_presets = vec![mine.clone()];
        store.set_settings(settings).expect("save");

        let reopened = Store::open(scratch.path()).expect("reopen").settings();
        assert_eq!(reopened.theme_presets, vec![mine.clone()]);
        assert_eq!(
            reopened.theme, mine.theme,
            "the live palette is kept separately"
        );

        // **Forgetting is not a change of appearance.** Applying a palette copies its values, so
        // the live theme never pointed at this entry — dropping it leaves the colours alone.
        let mut without = reopened.clone();
        without.theme_presets.clear();
        store.set_settings(without).expect("save again");

        let after = Store::open(scratch.path()).expect("reopen").settings();
        assert!(after.theme_presets.is_empty());
        assert_eq!(
            after.theme, mine.theme,
            "forgetting a palette changed the live one"
        );
    }

    /// A hue off the circle is wrapped onto it rather than refused.
    #[test]
    fn a_hand_edited_hue_is_brought_back_onto_the_circle() {
        let mut settings = Settings {
            theme: Theme {
                mode: ThemeMode::System,
                surface_hue: 420.0,
                accent_hue: -63.0,
                ..Theme::default()
            },
            ..Settings::default()
        };
        settings.normalise();

        // 420° is 60°, and -63° is 297° — both have an obvious meaning, so refusing them would be
        // refusing something the learner plainly meant.
        assert_eq!(settings.theme.surface_hue, 60.0);
        assert_eq!(settings.theme.accent_hue, 297.0);
        assert_eq!(
            settings.theme.mode,
            ThemeMode::System,
            "the mode is left alone"
        );

        // And nothing infinite survives to reach the colour maths.
        let mut broken = Settings {
            theme: Theme {
                surface_hue: f64::NAN,
                accent_hue: f64::INFINITY,
                ..Theme::default()
            },
            ..Settings::default()
        };
        broken.normalise();
        assert_eq!(broken.theme.surface_hue, 0.0);
        assert_eq!(broken.theme.accent_hue, 0.0);
    }

    /// The address of a runtime this app no longer installs is moved on.
    ///
    /// Ollama was the managed runtime until 2026-08-27, and its endpoint was written into every
    /// settings file that set the vision reader up. Left alone it would point the reader at a
    /// service this app no longer starts — running, on a machine that still has Ollama, and
    /// serving a model it may no longer hold.
    #[test]
    fn the_retired_ollama_endpoint_is_moved_to_the_app_s_own_server() {
        let mut settings = Settings {
            vlm_endpoint: "http://127.0.0.1:11434".into(),
            vlm_model: "hf.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF".into(),
            ..Settings::default()
        };
        settings.normalise();

        let fresh = Settings::default();
        assert_eq!(settings.vlm_endpoint, fresh.vlm_endpoint);
        assert_eq!(settings.vlm_model, fresh.vlm_model);
        assert_ne!(settings.vlm_endpoint, "http://127.0.0.1:11434");
    }

    /// **A deliberate Ollama setup is not the retired default**, and must survive.
    ///
    /// Someone running Ollama with a model of their own has chosen that; migrating them would
    /// be this app overruling a choice it did not make. The pair is what identifies the value
    /// this app itself wrote, and either half differing means a person put it there.
    #[test]
    fn an_endpoint_somebody_chose_is_left_where_they_put_it() {
        let kept = [
            ("http://127.0.0.1:11434", "qwen2.5vl:7b"),
            ("http://127.0.0.1:8000", "paddle-vl-manga"),
            ("http://192.168.1.10:8140", "paddleocr-vl-manga"),
        ];
        for (endpoint, model) in kept {
            let mut settings = Settings {
                vlm_endpoint: endpoint.into(),
                vlm_model: model.into(),
                ..Settings::default()
            };
            settings.normalise();
            assert_eq!(settings.vlm_endpoint, endpoint, "{endpoint} was rewritten");
            assert_eq!(settings.vlm_model, model, "{model} was rewritten");
        }
    }

    /// A settings file that binds left or right click is retired, not honoured.
    ///
    /// Both were bindable until 2026-08-27, so a file naming one is not corrupt — it is older.
    /// The required two go back to their defaults and the optional one clears, because the
    /// settings page shows all three and a rewrite it displays is not a silent one.
    #[test]
    fn a_binding_on_a_reading_click_is_retired_on_load() {
        for button in [MouseButton::Left, MouseButton::Right] {
            let mut settings = Settings {
                capture_bind: Binding::Mouse { button },
                overlay_bind: Binding::Mouse { button },
                switch_bind: Some(Binding::Mouse { button }),
                ..Settings::default()
            };
            settings.normalise();

            let fresh = Settings::default();
            assert_eq!(settings.capture_bind, fresh.capture_bind, "{button:?}");
            assert_eq!(settings.overlay_bind, fresh.overlay_bind, "{button:?}");
            assert_eq!(settings.switch_bind, None, "{button:?}");
        }
    }

    /// The three that remain are untouched, which is the half that would break quietly.
    ///
    /// A rule that retired every mouse binding would look identical in the app to one that
    /// worked — the settings page would show a keyboard default either way — so the buttons
    /// that *are* allowed have to be asserted, not assumed.
    #[test]
    fn the_buttons_the_reader_does_not_use_stay_bound() {
        for button in [MouseButton::Middle, MouseButton::X1, MouseButton::X2] {
            assert!(button.bindable(), "{button:?} should be allowed");

            let mut settings = Settings {
                capture_bind: Binding::Mouse { button },
                switch_bind: Some(Binding::Mouse { button }),
                ..Settings::default()
            };
            settings.normalise();

            assert_eq!(settings.capture_bind, Binding::Mouse { button });
            assert_eq!(settings.switch_bind, Some(Binding::Mouse { button }));
        }

        assert!(!MouseButton::Left.bindable());
        assert!(!MouseButton::Right.bindable());
        // A keyboard chord is never in question, whatever the mouse rule is.
        assert!(Binding::default().bindable());
    }

    /// The mirror of the test this replaced, which asserted the opposite while the engine was
    /// paused. Kept as a test rather than deleted: a stored choice surviving `normalise` is the
    /// whole difference between offered and not, and it would be easy to reinstate the migration
    /// by accident while tidying the withdrawn-analyzer case beside it.
    #[test]
    fn a_saved_vlm_choice_survives_now_that_the_engine_is_offered() {
        let mut settings = Settings {
            ocr: OcrChoice::Vlm,
            ..Settings::default()
        };
        settings.normalise();
        assert_eq!(settings.ocr, OcrChoice::Vlm);

        // The withdrawn analyzer beside it is still migrated, which is a different case.
        let mut withdrawn = Settings {
            analyzer: AnalyzerChoice::IchiMoe,
            ..Settings::default()
        };
        withdrawn.normalise();
        assert_eq!(withdrawn.analyzer, AnalyzerChoice::Auto);
    }

    #[test]
    fn no_tint_leaves_hiding_alone() {
        // Hiding is useful on its own — someone may have turned it on for the compositor
        // reason it was written for — so switching the tint off must not switch it off too.
        let mut settings = Settings {
            frame_tint: 0.0,
            hide_during_capture: true,
            ..Settings::default()
        };
        settings.normalise();
        assert!(settings.hide_during_capture, "not this rule's to undo");

        let mut off = Settings {
            frame_tint: 0.0,
            hide_during_capture: false,
            ..Settings::default()
        };
        off.normalise();
        assert!(!off.hide_during_capture);
    }

    #[test]
    fn the_frame_opacities_are_clamped() {
        let mut wild = Settings {
            frame_tint: 9.0,
            ..Settings::default()
        };
        wild.normalise();
        assert_eq!(wild.frame_tint, 0.5, "past a light wash it is a curtain");

        let mut negative = Settings {
            frame_tint: -1.0,
            ..Settings::default()
        };
        negative.normalise();
        assert_eq!(negative.frame_tint, 0.0);
        assert!(
            !negative.hide_during_capture,
            "a clamped-away tint is no tint"
        );
    }

    /// **Zero has to survive, and everything else has to be usable.**
    ///
    /// The double-tap window is the one threshold in here where zero is an answer rather than
    /// a broken value: it switches the gesture off. Clamping it up the way `click_delay` does
    /// would turn "I do not want this" into "I want it, in under a fifth of a second" — a
    /// lock that flips on taps the reader did not think of as a pair, which is exactly the
    /// complaint the off switch exists for.
    #[test]
    fn the_double_tap_window_is_clamped_but_zero_means_off() {
        let off = Settings {
            lock_tap_ms: 0,
            ..Settings::default()
        };
        assert_eq!(off.lock_tap(), 0, "zero is off, not 'very fast'");

        // Too small to press a chord twice inside, and too large to be a double anything.
        let hasty = Settings {
            lock_tap_ms: 20,
            ..Settings::default()
        };
        assert_eq!(hasty.lock_tap(), 150);
        let patient = Settings {
            lock_tap_ms: 30_000,
            ..Settings::default()
        };
        assert_eq!(patient.lock_tap(), 1_000);

        // And an ordinary value is its own.
        let ordinary = Settings {
            lock_tap_ms: 400,
            ..Settings::default()
        };
        assert_eq!(ordinary.lock_tap(), 400);
        assert_eq!(
            Settings::default().lock_tap(),
            400,
            "the default has to be a value the gesture actually works at"
        );
    }

    #[test]
    fn settings_default_and_persist() {
        let scratch = Scratch::new("settings");
        let store = Store::open(scratch.path()).expect("open");
        assert_eq!(store.settings(), Settings::default());
        assert!(store.settings().furigana);

        store
            .set_settings(Settings {
                furigana: false,
                // Away from the default too: a display choice that silently reverted on reopen
                // would be indistinguishable from the reader ignoring the setting.
                keep_line_breaks: true,
                // Away from the default too, and for the same reason: an alignment that reverted
                // on reopen would look like the reader ignoring the choice.
                passage_align: PassageAlign::Center,
                // Both away from their defaults, for the same reason: these are the two the quick
                // panel offers, and a toggle that reverted on reopen would look like the app
                // ignoring it rather than like a persistence bug.
                mark_reading_guesses: true,
                // The same reason once more: off by default, so only a round trip can tell
                // "the reader never turned it on" from "the app forgot that they did".
                show_word_parts: true,
                copy_capture_to_clipboard: true,
                mark_seen_on_focus: false,
                // Away from the default so the A/B choice is proven to survive a restart: a
                // reader comparing the two card models would otherwise be moved back to the
                // new one every time the app reopened.
                focus_before_open: false,
                // Away from the default so this test proves the accessibility choice is kept.
                tab_navigation: true,
                split_ratio: 0.7,
                capture_bind: Binding::Mouse {
                    button: MouseButton::X2,
                },
                overlay_bind: Binding::Mouse {
                    button: MouseButton::X1,
                },
                frame_rect: Some([10.0, 20.0, 640.0, 200.0]),
                hold_ms: 400,
                lock_tap_ms: 250,
                hide_during_capture: true,
                font_size: 18.0,
                reader_scale: 1.4,
                panel_scale: 0.8,
                read_clipboard: true,
                quiet: false,
                // Away from the default, like the rest of this literal: the test is that every
                // field survives a write and a reopen, and a field left at its default would
                // round-trip even if `set_settings` dropped it entirely.
                ichiran_server: false,
                preload_ichiran_on_startup: false,
                preload_ocr_on_startup: false,
                check_updates_on_startup: false,
                log: true,
                collect_sentences: true,
                analyzer: AnalyzerChoice::BuiltIn,
                ocr: OcrChoice::Paddle,
                vlm_endpoint: "http://127.0.0.1:8080".into(),
                vlm_model: "paddleocr-vl-for-manga".into(),
                preprocess: PreprocessSettings {
                    tighten: true,
                    contrast: false,
                    upscale: true,
                },
                anki_deck: "Japanese".into(),
                anki_our_decks_only: false,
                theme: Theme {
                    mode: ThemeMode::Light,
                    surface_hue: 210.0,
                    accent_hue: 40.0,
                    // Away from the default, so the wheel's position is proven to survive a
                    // restart rather than snapping back and repainting every sense list.
                    kind_spin: 84.0,
                    tint: 1.4,
                    contrast: 1.2,
                    depth: 0.03,
                    overrides: BTreeMap::from([("accent".into(), "#ff8800".into())]),
                },
                theme_presets: vec![NamedTheme {
                    name: "Midnight".into(),
                    theme: Theme {
                        mode: ThemeMode::Dark,
                        surface_hue: 250.0,
                        accent_hue: 200.0,
                        ..Theme::default()
                    },
                    decor: None,
                }],
                animations: false,
                animation_ms: 90,
                panels_ratio: 0.62,
                decor_cloud: 0.11,
                decor_cloud_tint: DecorTint::Accent,
                decor_cloud_on: false,
                decor_sakura: 0.0,
                // Ink, not Blossom: Blossom became the default, and a field left at its default
                // round-trips even if it is dropped.
                decor_sakura_tint: DecorTint::Ink,
                decor_sakura_on: false,
                decor_scale: 1.4,
                decor_cloud_pad: 2.5,
                decor_cloud_spread: 1.6,
                decor_sakura_spread: 0.8,
                // Off: softness became the default, so on would prove nothing here.
                decor_blur: false,
                decor_cloud_blur: 5.5,
                decor_sakura_blur: 2.0,
                frequency_source: "jpdb".into(),
                frequency_direction: crate::dict::frequency::Direction::Count,
                show_frequency_source: true,
                settings_section: "diagnostics".into(),
                switch_bind: Some(Binding::Key {
                    chord: "CmdOrCtrl+Shift+O".into(),
                }),
                click_delay_ms: 400,
                overlay_enabled: false,
                frame_locked: true,
                reference_layout: BrowseLayout::Rows,
                glossary_sort: GlossarySort::Alpha,
                reference_tabs: vec!["竜".into(), "smoker".into()],
                reference_tab: 1,
                source_on_analysis: true,
                source_on_reference: true,
                frame_tint: 0.25,
                frame_clickthrough: true,
                always_on_top: true,
                hide_setup: true,
            })
            .expect("save");

        let reopened = Store::open(scratch.path()).expect("reopen");
        assert!(!reopened.settings().preload_ichiran_on_startup);
        assert!(!reopened.settings().furigana);
        assert!(reopened.settings().keep_line_breaks);
        assert_eq!(reopened.settings().passage_align, PassageAlign::Center);
        assert!(reopened.settings().mark_reading_guesses);
        assert!(reopened.settings().show_word_parts);
        assert!(reopened.settings().copy_capture_to_clipboard);
        assert!(!reopened.settings().mark_seen_on_focus);
        assert!(!reopened.settings().focus_before_open);
        assert!(reopened.settings().tab_navigation);
        assert_eq!(reopened.settings().split_ratio, 0.7);
        assert_eq!(reopened.settings().theme.kind_spin, 84.0);
        // The binding and the frame's position have to survive a restart, or neither is
        // persistent in the way the feature promises.
        assert_eq!(
            reopened.settings().capture_bind,
            Binding::Mouse {
                button: MouseButton::X2
            }
        );
        assert_eq!(
            reopened.settings().frame_rect,
            Some([10.0, 20.0, 640.0, 200.0])
        );
        assert_eq!(reopened.settings().hold_ms, 400);
        assert!(reopened.settings().hide_during_capture);
        assert_eq!(reopened.settings().font_size, 18.0);
        assert_eq!(reopened.settings().reader_scale, 1.4);
        assert_eq!(reopened.settings().panel_scale, 0.8);
        assert!(reopened.settings().read_clipboard);
        // Written as `false` against a `true` default, which is the direction that
        // catches a field silently falling back to its default on load.
        assert!(!reopened.settings().quiet);
        // Both written as `true` against `false` defaults. A log that quietly reverted to
        // off would look exactly like a log that was never turned on.
        assert!(reopened.settings().log);
        assert!(reopened.settings().collect_sentences);
        assert_eq!(reopened.settings().analyzer, AnalyzerChoice::BuiltIn);
        assert_eq!(reopened.settings().ocr, OcrChoice::Paddle);
        assert_eq!(reopened.settings().vlm_endpoint, "http://127.0.0.1:8080");
        assert_eq!(reopened.settings().vlm_model, "paddleocr-vl-for-manga");
        // Each flag on its own, not the struct as a whole: three booleans compared in one
        // assertion pass just as well when two of them are swapped.
        assert!(reopened.settings().preprocess.tighten);
        assert!(!reopened.settings().preprocess.contrast);
        assert!(reopened.settings().preprocess.upscale);
        assert_eq!(reopened.settings().anki_deck, "Japanese");
        assert!(!reopened.settings().animations);
        // The layout the study area was left in. It is deliberately not 0.5 here so a
        // fall-back cannot pass by landing on its own default.
        assert_eq!(reopened.settings().panels_ratio, 0.62);
        // Also not its default, and deliberately below 0.5 — the reference divider defaults
        // wide, so a fall-back here cannot pass by coincidence either.
        // The decoration, all four not their defaults — including a zero, which is how the
        // sakura is switched off and so is the one value a careless `unwrap_or_default`
        // would silently agree with.
        assert_eq!(reopened.settings().decor_cloud, 0.11);
        assert_eq!(reopened.settings().decor_cloud_tint, DecorTint::Accent);
        assert_eq!(reopened.settings().decor_sakura, 0.0);
        assert_eq!(reopened.settings().decor_sakura_tint, DecorTint::Ink);
        assert!(
            !reopened.settings().decor_cloud_on,
            "a switched-off decoration stays off"
        );
        assert!(!reopened.settings().decor_sakura_on);
        assert!(!reopened.settings().decor_blur);
        assert_eq!(reopened.settings().decor_scale, 1.4);
        assert_eq!(reopened.settings().decor_cloud_pad, 2.5);
        assert_eq!(reopened.settings().decor_cloud_spread, 1.6);
        assert_eq!(reopened.settings().decor_sakura_spread, 0.8);
        assert_eq!(reopened.settings().frequency_source, "jpdb");
        assert_eq!(
            reopened.settings().frequency_direction,
            crate::dict::frequency::Direction::Count,
            "reading this back as a rank would order every search backwards"
        );
        assert!(
            reopened.settings().show_frequency_source,
            "the opt-in frequency source line must survive a restart"
        );
        assert_eq!(reopened.settings().settings_section, "diagnostics");
        assert!(!reopened.settings().check_updates_on_startup);
        // `Some` against a `None` default, and a delay that is not the default either.
        assert_eq!(
            reopened.settings().switch_bind,
            Some(Binding::Key {
                chord: "CmdOrCtrl+Shift+O".into()
            })
        );
        assert_eq!(reopened.settings().click_delay_ms, 400);
        // `false` against a `true` default, and `true` against a `false` one: both
        // directions, so neither can pass by falling back onto the value it replaced.
        assert!(!reopened.settings().overlay_enabled);
        assert!(reopened.settings().frame_locked);
        // Both away from their defaults, for the same reason `panels_ratio` is not 0.5.
        assert_eq!(reopened.settings().glossary_sort, GlossarySort::Alpha);
        assert_eq!(reopened.settings().reference_tabs, ["竜", "smoker"]);
        assert_eq!(reopened.settings().reference_tab, 1);
        // `true` against a `false` default, the direction that catches a fall-back. A
        // dismissal that quietly reverted would put the panel back on screen every launch,
        // which is the one thing hiding it is meant to stop.
        assert!(reopened.settings().hide_setup);
        assert!(reopened.settings().always_on_top);
        assert!(reopened.settings().source_on_analysis);
        assert!(reopened.settings().source_on_reference);
        assert_eq!(reopened.settings().reference_layout, BrowseLayout::Rows);
        assert_eq!(reopened.settings().frame_tint, 0.25);
        assert!(reopened.settings().frame_clickthrough);
    }
    /// A spread outside the range the artwork survives is pulled back rather than refused.
    ///
    /// Reachable by hand-editing the file or by a version with a wider range, and the cost
    /// of not checking is a decoration that stops reading as one — the slots between the
    /// clouds are the motif, and squeezing them shut leaves a bar.
    #[test]
    fn a_spread_outside_what_the_artwork_survives_is_pulled_back() {
        let mut settings = Settings {
            decor_cloud_spread: 12.0,
            decor_sakura_spread: 0.01,
            ..Settings::default()
        };
        settings.normalise();
        assert_eq!(settings.decor_cloud_spread, 2.5);
        assert_eq!(settings.decor_sakura_spread, 0.5);
    }

    /// A settings file can name a tab that is not there — hand-edited, written by a version
    /// with a larger cap, or half-written by a crash. The interface indexes the tab list
    /// with this, so an index past the end has to be impossible by the time it is read
    /// rather than guarded at every use.
    #[test]
    fn a_reference_tab_past_the_end_is_pulled_back_onto_a_real_tab() {
        let mut settings = Settings {
            reference_tabs: vec!["竜".into(), "smoker".into()],
            reference_tab: 9,
            ..Settings::default()
        };
        settings.normalise();
        assert_eq!(settings.reference_tab, 1, "the last tab, not the ninth");
    }

    /// The underflow this is really guarding: `len() - 1` on an empty list is `usize::MAX`,
    /// which would be indexed straight into the tab strip.
    #[test]
    fn no_tabs_at_all_leaves_the_index_at_zero() {
        let mut settings = Settings {
            reference_tabs: Vec::new(),
            reference_tab: 4,
            ..Settings::default()
        };
        settings.normalise();
        assert_eq!(settings.reference_tab, 0);
    }

    /// The cap is what keeps a long session from growing the settings file without bound.
    #[test]
    fn more_tabs_than_the_cap_are_trimmed_and_the_index_follows() {
        let mut settings = Settings {
            reference_tabs: (0..40).map(|n| n.to_string()).collect(),
            reference_tab: 39,
            ..Settings::default()
        };
        settings.normalise();
        assert_eq!(settings.reference_tabs.len(), Settings::REFERENCE_TABS_MAX);
        assert_eq!(
            settings.reference_tab,
            Settings::REFERENCE_TABS_MAX - 1,
            "the index has to be trimmed with the list, not left pointing past it"
        );
    }

    /// A settings file written before these fields existed must still load — that is what
    /// `#[serde(default)]` on the struct is for, and it is easy to break by accident.
    /// Nothing the analyzer keeps may land on a file the store owns.
    ///
    /// The word cache shipped pointing at `words.json`, which is where saved word statuses
    /// live. It wrote a map over a learner's known and seen marks, and the store then
    /// refused to load or save at all — the data gone and the feature dead, from one filename
    /// chosen without looking at what was already in the directory.
    ///
    /// Asserted as a property of the *store*: these are the names it will not share.
    #[test]
    fn the_store_owns_these_filenames_and_nothing_else_may_use_them() {
        let scratch = Scratch::new("owned-names");
        let store = Store::open(scratch.path()).expect("open");
        store
            .set_status(WordKey::new("水", "みず"), WordStatus::Known)
            .expect("mark");
        store
            .set_settings(Settings::default())
            .expect("save settings");

        for owned in ["words.json", "settings.json"] {
            assert!(
                scratch.path().join(owned).exists(),
                "{owned} is the store's; anything else writing there destroys it"
            );
        }
        // The cache's name, which must not be one of them.
        assert!(
            !["words.json", "settings.json"].contains(&"analysis-cache.json"),
            "the analysis cache must not share a name with a store file"
        );

        // And a store opened beside the cache is a store that still works.
        std::fs::write(
            scratch.path().join("analysis-cache.json"),
            r#"{"engine":"ichiran","words":[]}"#,
        )
        .expect("write the cache");
        let reopened = Store::open(scratch.path()).expect("the cache must not break the store");
        assert_eq!(
            reopened.status(&WordKey::new("水", "みず")),
            WordStatus::Known,
            "the saved status survived the cache being written beside it"
        );
    }

    #[test]
    fn an_older_settings_file_still_loads() {
        let older = r#"{ "furigana": false, "split_ratio": 0.6 }"#;
        let settings: Settings = serde_json::from_str(older).expect("older file parses");
        assert!(!settings.furigana);
        assert!(
            settings.mark_seen_on_focus,
            "an older settings file should adopt automatic Seen marking"
        );
        assert!(
            settings.focus_before_open,
            "an older settings file should adopt the focus-then-open card model"
        );
        assert!(
            !settings.tab_navigation,
            "an older settings file should preserve reader-first Tab handling"
        );
        assert_eq!(settings.split_ratio, 0.6);
        assert_eq!(settings.capture_bind, Binding::default());
        assert_eq!(settings.overlay_bind, Settings::default().overlay_bind);
        assert_eq!(settings.frame_rect, None);
        assert_eq!(settings.hold_ms, 500);
        assert!(
            !settings.hide_during_capture,
            "the transparent interior is the default"
        );
        assert_eq!(settings.font_size, 15.0);
        assert_eq!(settings.analyzer, AnalyzerChoice::Auto);
        assert_eq!(
            settings.ocr,
            OcrChoice::Auto,
            "older settings adopt Auto OCR"
        );
        assert_eq!(
            settings.anki_deck, "",
            "a file from before this field existed has chosen no deck — it used to fall back to              Anki's own `Default`, which is the last place 屠つ vocabulary should land"
        );
        assert!(settings.animations);
        assert!(
            settings.frequency_source.is_empty(),
            "no corpus is installed until someone installs one"
        );
        assert_eq!(
            settings.settings_section, "reading",
            "settings opens on its first section until someone opens another"
        );
        assert!(
            !settings.show_frequency_source,
            "older settings adopt the uncluttered Reference result list"
        );
        assert_eq!(
            settings.switch_bind, None,
            "no hotkey is set until someone sets one"
        );
        assert_eq!(settings.click_delay_ms, 250);
        assert!(
            settings.overlay_enabled,
            "a file written before this setting existed has not switched the overlay off,              and reading it as off would kill both bindings on upgrade"
        );
        assert!(
            !settings.frame_locked,
            "the frame has always come to the pointer on a tap"
        );
        assert_eq!(
            settings.glossary_sort,
            GlossarySort::Grouped,
            "the glossary has always opened under its headings"
        );
        assert_eq!(
            settings.passage_align,
            PassageAlign::Left,
            "the reader has always set a passage from the left edge"
        );
        assert_eq!(settings.reference_layout, BrowseLayout::Cards);
        assert!(
            settings.reference_tabs.is_empty(),
            "a fresh install opens the reference page on one empty tab"
        );
        // A file from before spacing existed takes today's default, as every missing field does.
        // That was 1.0, the spacing the patterns were drawn at; since 2026-10-19 it is the tuned
        // 0.5 / 0.7. Nothing released had files this old, so no installed look moved.
        assert_eq!(
            (settings.decor_cloud_spread, settings.decor_sakura_spread),
            (
                Settings::default().decor_cloud_spread,
                Settings::default().decor_sakura_spread
            ),
            "a missing spacing is the fresh-install spacing"
        );
        assert!(
            settings.decor_cloud_on && settings.decor_sakura_on,
            "a file from before the switches shows both decorations"
        );
        assert!(
            !settings.hide_setup,
            "a file written before this setting existed has not opted out of anything, \
             and defaulting the other way would hide the installation panel from everyone \
             who upgrades"
        );
        assert!(
            !settings.always_on_top,
            "a window that pinned itself above everything without being asked would be a \
             surprising thing for an upgrade to do"
        );
    }

    #[test]
    fn the_retired_reader_ratio_is_accepted_but_never_saved_again() {
        // Serde ignores unknown object fields by design. Pin that compatibility here because
        // every settings file written while Translation split the reader contains this one.
        let mut legacy = serde_json::to_value(Settings::default()).expect("serialize legacy");
        legacy
            .as_object_mut()
            .expect("settings are an object")
            .insert("reader_ratio".into(), serde_json::json!(0.38));

        let loaded: Settings = serde_json::from_value(legacy).expect("old settings still load");
        let saved = serde_json::to_value(loaded).expect("new settings serialize");
        assert!(
            saved.get("reader_ratio").is_none(),
            "future saves must omit the divider that no longer exists"
        );
    }

    #[test]
    fn a_legacy_reference_layout_migrates_and_the_glossary_one_is_dropped() {
        // Two different jobs in one file. `dictionary_show: "compact"` is the oldest spelling of
        // the Reference choice and still has to arrive as `reference_layout: rows` — that
        // preference is live. The Glossary's copy is retired: the page draws one column at every
        // width now, so both its spellings load without complaint and neither is written again.
        let mut legacy = serde_json::to_value(Settings::default()).expect("serialize legacy");
        let object = legacy.as_object_mut().expect("settings are an object");
        object.remove("reference_layout");
        object.insert("dictionary_show".into(), serde_json::json!("compact"));
        object.insert("glossary_show".into(), serde_json::json!("cards"));
        object.insert("glossary_layout".into(), serde_json::json!("rows"));

        let loaded: Settings = serde_json::from_value(legacy).expect("old settings still load");
        assert_eq!(loaded.reference_layout, BrowseLayout::Rows);
        let saved = serde_json::to_value(loaded).expect("new settings serialize");
        assert!(saved.get("dictionary_show").is_none());
        assert_eq!(
            saved.get("reference_layout"),
            Some(&serde_json::json!("rows"))
        );
        for retired in ["glossary_layout", "glossary_show"] {
            assert!(
                saved.get(retired).is_none(),
                "{retired} names a choice the glossary no longer offers"
            );
        }
    }

    #[test]
    fn a_settings_file_missing_a_newer_field_still_loads() {
        // An older file must not stop the app from starting.
        let scratch = Scratch::new("partial");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        fs::write(
            scratch.path().join("settings.json"),
            r#"{ "furigana": false }"#,
        )
        .expect("write");

        let store = Store::open(scratch.path()).expect("open");
        assert!(!store.settings().furigana);
        assert_eq!(
            store.settings().split_ratio,
            Settings::default().split_ratio
        );
    }

    /// `Learning` was the on-disk spelling before the status was renamed to `Seen`.
    /// One unknown enum value rejects the whole `Vec<WordRecord>`, so this protects every
    /// mark in the file rather than only the records carrying the old value.
    #[test]
    fn legacy_learning_statuses_load_as_seen_without_setting_the_file_aside() {
        let scratch = Scratch::new("learning-to-seen");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        fs::write(
            scratch.path().join("words.json"),
            r#"[
                {"term":"会う","reading":"あう","status":"learning"},
                {"term":"水","reading":"みず","status":"known"},
                {"term":"東京","reading":"とうきょう","status":"ignored"},
                {"term":"初めて","reading":"はじめて","status":"new"}
            ]"#,
        )
        .expect("write legacy statuses");

        let store = Store::open(scratch.path()).expect("legacy store should open");

        assert_eq!(
            store.status(&WordKey::new("会う", "あう")),
            WordStatus::Seen
        );
        assert_eq!(store.status(&WordKey::new("水", "みず")), WordStatus::Known);
        assert_eq!(
            store.status(&WordKey::new("東京", "とうきょう")),
            WordStatus::Ignored
        );
        assert_eq!(
            store.status(&WordKey::new("初めて", "はじめて")),
            WordStatus::New
        );
        assert_eq!(
            store.note(),
            None,
            "a successful migration must not report recovery"
        );
        assert!(
            !scratch.path().join("words.json.unreadable").exists(),
            "the learner's complete status file was set aside"
        );
    }

    #[test]
    fn corrupt_stored_words_are_reported_rather_than_silently_discarded() {
        // Quietly resetting would throw away everything the learner had marked. Refusing to
        // open would throw away everything *else* — with no store, settings fall back to
        // their defaults and nothing can be written, so one unreadable file read as the whole
        // app resetting itself. Neither. It is set aside, said out loud, and the rest works.
        let scratch = Scratch::new("corrupt");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        fs::write(scratch.path().join("words.json"), "{ not json").expect("write");

        let store = Store::open(scratch.path()).expect("an unreadable file must not stop this");

        let note = store.note().expect("it must say what it did");
        assert!(
            note.contains("words.json.unreadable"),
            "no route back: {note}"
        );

        // Nothing deleted, and the original still holds what it held.
        let aside = scratch.path().join("words.json.unreadable");
        assert!(aside.exists(), "the file was not preserved");
        assert_eq!(fs::read_to_string(&aside).expect("read"), "{ not json");

        // And the store is a working store.
        store
            .set_status(WordKey::new("水", "みず"), WordStatus::Known)
            .expect("saving must work again");
        assert_eq!(
            Store::open(scratch.path())
                .expect("reopen")
                .status(&WordKey::new("水", "みず")),
            WordStatus::Known,
        );
    }

    /// The failure that was reported: one unreadable file took every setting with it.
    #[test]
    fn unreadable_words_do_not_take_the_settings_with_them() {
        let scratch = Scratch::new("words-not-settings");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        fs::write(
            scratch.path().join("settings.json"),
            r#"{ "font_size": 19.0, "furigana": false }"#,
        )
        .expect("write settings");
        // Exactly what the analysis cache wrote over it.
        fs::write(
            scratch.path().join("words.json"),
            r#"{"engine":"ichiran","words":[]}"#,
        )
        .expect("write words");

        let store = Store::open(scratch.path()).expect("open");
        assert_eq!(
            store.settings().font_size,
            19.0,
            "the settings were readable and must have been read"
        );
        assert!(!store.settings().furigana);
    }

    /// **A setting this version no longer has is passed over, not fatal.** Every file saved
    /// before 2026-10-23 carries `content_width`, the column width the reading page now takes
    /// from the window. Rejecting it would reset every other setting on upgrade.
    #[test]
    fn a_retired_column_width_does_not_cost_the_other_settings() {
        let scratch = Scratch::new("retired-column-width");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        fs::write(
            scratch.path().join("settings.json"),
            r#"{ "content_width": 54.0, "font_size": 19.0, "furigana": false }"#,
        )
        .expect("write settings");

        let store = Store::open(scratch.path()).expect("open");
        assert_eq!(store.settings().font_size, 19.0);
        assert!(!store.settings().furigana);
    }

    /// And the other way round, so neither file can take the other down.
    #[test]
    fn unreadable_settings_do_not_take_the_word_statuses_with_them() {
        let scratch = Scratch::new("settings-not-words");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        // Written by the store, so this tests the real format rather than a guess at it.
        Store::open(scratch.path())
            .expect("open")
            .set_status(WordKey::new("水", "みず"), WordStatus::Known)
            .expect("mark");
        fs::write(scratch.path().join("settings.json"), "{ not json").expect("write");

        let store = Store::open(scratch.path()).expect("open");
        assert_eq!(store.status(&WordKey::new("水", "みず")), WordStatus::Known);
        assert!(store
            .note()
            .expect("said")
            .contains("settings.json.unreadable"));
    }

    /// A second failure must not overwrite the first file set aside — that one is the one
    /// with the real data in it.
    #[test]
    fn setting_aside_twice_keeps_the_first_one() {
        let scratch = Scratch::new("aside-twice");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        fs::write(scratch.path().join("words.json.unreadable"), "the original").expect("write");
        fs::write(scratch.path().join("words.json"), "{ not json").expect("write");

        Store::open(scratch.path()).expect("open");
        assert_eq!(
            fs::read_to_string(scratch.path().join("words.json.unreadable")).expect("read"),
            "the original",
            "the first file set aside was overwritten by the second"
        );
    }

    #[test]
    fn the_saved_file_is_stable_and_readable() {
        let scratch = Scratch::new("stable");
        let store = Store::open(scratch.path()).expect("open");
        for term in ["水", "食べる", "気持ち"] {
            store
                .set_status(WordKey::new(term, ""), WordStatus::Seen)
                .expect("set");
        }

        let first = fs::read_to_string(scratch.path().join("words.json")).expect("read");
        // Re-saving must not reshuffle the file: a HashMap's order would.
        store
            .set_status(WordKey::new("水", ""), WordStatus::Seen)
            .expect("set");
        let second = fs::read_to_string(scratch.path().join("words.json")).expect("read");
        assert_eq!(first, second);
        assert!(first.contains("\"status\": \"seen\""));
    }

    /// **A word is one word however it was conjugated when you marked it.**
    ///
    /// `WordKey::for_token` keys on the dictionary form and the lemma reading, never the surface.
    /// Marking 伝わって Known in one sentence and meeting 伝わる in the next has to be the same
    /// word, and the failure would be silent: `statuses` would answer honestly about a key
    /// nothing else writes, so the reader would simply see a word it had already been told about
    /// come back as New.
    #[test]
    fn a_word_is_the_same_word_however_it_was_conjugated() {
        let scratch = Scratch::new("status-one-key");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        let store = Store::open(scratch.path()).expect("open");

        // As Analysis has it: a conjugated surface out of a sentence.
        let analysed = Token {
            surface: "伝わって".into(),
            dict_form: "伝わる".into(),
            lemma_reading: "つたわる".into(),
            ..Default::default()
        };
        // The same word met in its dictionary form.
        let looked_up = Token {
            surface: "伝わる".into(),
            dict_form: "伝わる".into(),
            lemma_reading: "つたわる".into(),
            ..Default::default()
        };
        assert_eq!(
            WordKey::for_token(&analysed),
            WordKey::for_token(&looked_up),
            "the surface differs and the word does not",
        );

        store
            .set_status(WordKey::for_token(&looked_up), WordStatus::Known)
            .expect("mark the plain form");
        assert_eq!(
            store.status(&WordKey::for_token(&analysed)),
            WordStatus::Known,
            "and the reader sees it",
        );

        // And the batched read answers the same, in the order it was asked.
        assert_eq!(
            store.statuses(&[WordKey::for_token(&analysed), WordKey::new("未知", "みち"),]),
            vec![WordStatus::Known, WordStatus::default()],
        );
    }
}

#[cfg(test)]
mod startup_preload_tests {
    use super::Settings;

    #[test]
    fn missing_startup_preload_inherits_the_enabled_default() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(old.preload_ichiran_on_startup);
        assert!(Settings::default().preload_ichiran_on_startup);
        assert!(old.preload_ocr_on_startup);
        assert!(Settings::default().preload_ocr_on_startup);
    }

    #[test]
    fn a_disabled_ocr_preload_survives_and_leaves_ichiran_alone() {
        let settings: Settings =
            serde_json::from_str(r#"{"preload_ocr_on_startup":false}"#).unwrap();
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(!restored.preload_ocr_on_startup);
        assert!(
            restored.preload_ichiran_on_startup,
            "the two preloads are separate choices"
        );
    }

    #[test]
    fn an_explicitly_disabled_preload_survives_serialization() {
        let settings: Settings =
            serde_json::from_str(r#"{"preload_ichiran_on_startup":false}"#).unwrap();
        assert!(!settings.preload_ichiran_on_startup);
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(!restored.preload_ichiran_on_startup);
        assert!(
            restored.ichiran_server,
            "preload and persistence are separate choices"
        );
    }
}
