//! The normalized analysis model — **the contract** between the analyzer and the
//! rest of the app.
//!
//! It is shaped by the richest analyzer (Ichiran, see `project notes` §4) so that a
//! leaner embeddable analyzer can later fill the same shape without the grammar
//! engine noticing. Everything downstream (grammar, dictionaries, UI) depends on
//! these types and *not* on any analyzer's native output.

use serde::{Deserialize, Serialize};

/// One dictionary sense of a word.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Sense {
    /// Part-of-speech / field / misc tags as the source gives them, e.g. "n,vs,vt".
    #[serde(default)]
    pub pos: String,
    /// The gloss text, e.g. "memory; recollection; remembrance".
    pub gloss: String,
    /// The JMdict sequence this sense came from, when the source states one.
    ///
    /// **Provenance the merge used to discard.** `dict::merge` collapses several entries' senses
    /// into one list and `dict::context::RankedSense` indexes into that, so there was no way to
    /// ask whether the ranker favoured the *right entry* — only which position it chose. That is
    /// why sense ordering had never been measured. Carried here so it can be.
    #[serde(default)]
    // **`number`, not the `bigint` ts-rs infers from `i64`.** This is the rule in the `bindings`
    // module working: where the generated declaration and the golden disagree, the golden wins,
    // because it is bytes rather than a second opinion about them. `serde_json` emits a plain
    // JSON number here and `model.golden.json` shows one; `bigint` would make the frontend
    // program against a type nothing sends. ts-rs is being cautious about the full `i64` range,
    // and a JMdict sequence is seven digits, so `i32` describes the range honestly.
    // `as`, not `type`: `#[ts(type = "number")]` replaces the whole declaration and silently
    // drops the `| null` the `Option` earns, which the golden then contradicts.
    #[cfg_attr(feature = "typescript", ts(as = "Option<i32>"))]
    pub sequence: Option<i64>,
}

/// One morphological property of a conjugated form.
///
/// A compound conjugation (食べさせられていた) is a *chain* of these, each tagged with
/// its depth in Ichiran's `via` chain. The deepest one (`is_base`) is the
/// transformation applied directly to the dictionary verb, and it carries the
/// verb's reading — which is what lets the grammar layer show *how* a form is
/// built, not merely name it.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Prop {
    /// The verb class, e.g. "v5u", "v1", "vk" — drives the formation generator.
    #[serde(default)]
    pub pos: String,
    /// The form label as prose, e.g. "Past (~ta)", "Causative-Passive".
    pub form: String,
    #[serde(default)]
    pub neg: bool,
    #[serde(default)]
    pub fml: bool,
    /// Depth in the via chain; 0 is the outermost applied form.
    #[serde(default)]
    pub via_depth: u32,
    /// The dictionary-form reading (kana) of the verb this form applies to.
    /// Present on the base transformation.
    #[serde(default)]
    pub base_reading: String,
    /// True for the deepest prop — the transformation on the dictionary verb.
    #[serde(default)]
    pub is_base: bool,
}

/// The conjugation of a token: its dictionary lemma plus the ordered chain of
/// forms that produced the surface.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Conjugation {
    pub lemma: String,
    /// The lemma's reading (kana), normalized to hiragana.
    #[serde(default)]
    pub reading: String,
    pub props: Vec<Prop>,
}

/// A reading the analyzer considered but did not choose (an ambiguous surface).
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Alternative {
    pub lemma: String,
    #[serde(default)]
    pub reading: String,
    #[serde(default)]
    pub form: String,
    #[serde(default)]
    pub senses: Vec<Sense>,
}

/// What the line break after a token means, when there is one.
///
/// **The analyzers throw the break away**, so without this it is invisible to every rule that asks
/// where a sentence ends. Lindera cuts its lattice at a newline and then drops the whitespace
/// token; Ichiran returns the break as its own segment and `convert` skips it. `ends_sentence`
/// listed `'\n'` among its terminators for a long time and never once fired.
///
/// Three states rather than a bool, because a break is genuinely two different things and they
/// want opposite treatment:
///
/// - a **`Break`** separates two utterances — subtitles, lyrics, most game script;
/// - a **`Wrap`** is one sentence too wide for the panel. PP-OCR emits one line per detected
///   box, and for vertical Japanese a *column* is a box, so one sentence down three columns of a
///   bubble arrives as three lines. Splitting there would shatter it.
///
/// [`crate::lines`] decides which, by asking the parser. The third state earns itself twice over:
/// `dict::names` may join a name across a `Wrap` — 善福寺川 broken over two columns is one name —
/// and must never join one across a `Break`.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineBreak {
    /// Nothing follows this token but the next one.
    #[default]
    None,
    /// A line ended here, mid-sentence.
    Wrap,
    /// A line ended here, and so did the sentence.
    Break,
}

/// Stable identity for a learner-facing construction.
///
/// This is also the study identity: it survives inflection and spelling changes and never uses the
/// selectable surface as a vocabulary key.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstructionIdentity {
    ExplanatoryNan,
    ContractedMotion,
    CopulaRecall,
}

/// Where a construction gets its lookup answer.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstructionLookup {
    /// Look the token's lexical head up normally.
    Headword,
    /// Suppress lexical hits and let the named grammar entry answer.
    GrammarPattern,
}

/// Where the grammar entry belongs in the study response.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstructionGrammar {
    Sentence,
    Auxiliary,
}

/// A validated half-open source span.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

/// Evidence that a token is one of the constructions the reader explains specially.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Construction {
    pub identity: ConstructionIdentity,
    pub source: SourceSpan,
    /// Indices into [`Token::components`], in source order.
    pub components: Vec<u32>,
    pub lookup: ConstructionLookup,
    pub grammar: ConstructionGrammar,
    /// Resolved through the complete pack set; imported entries are valid destinations too.
    pub grammar_pattern: String,
}

/// A narrow morphological role established by the analyzer before dictionary lookup.
///
/// This is deliberately smaller than a general part-of-speech mirror. Most analyzer morphology
/// either already reaches the normalized contract through senses and conjugation, or has no safe
/// use downstream. Pronouns and suffixes are the two measured exceptions under Lindera: UniDic
/// distinguishes them, Lindera supplies no senses, and discarding that distinction let a
/// homographic noun lead the dictionary answer. Older serialized tokens have no role and remain
/// unknown rather than acquiring one from their spelling.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LexicalRole {
    Pronoun,
    Suffix,
    /// UniDic's 名詞. **Carried because grouping needs the left constituent established, not
    /// merely un-refuted.** A rule that accepted "does not inflect, is not a suffix, is not in
    /// the particle table" was reading silence as a noun, and silence is what the analyzer says
    /// about a word it could not classify at all.
    Noun,
}

/// One analysed word. Compounds carry their parts in `components`.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Token {
    /// The text as written.
    pub surface: String,
    /// The dictionary form.
    #[serde(default)]
    pub dict_form: String,
    /// The surface reading (kana) — this drives furigana.
    #[serde(default)]
    pub reading: String,
    /// The dictionary-form reading (kana). May differ from `reading`
    /// (つたわって vs つたわる).
    #[serde(default)]
    pub lemma_reading: String,
    /// Morphology the analyzer established independently of dictionary meanings.
    ///
    /// Optional for backward compatibility and because analyzers need not expose it. A composed
    /// token does not inherit a constituent's role: the role describes the token itself.
    #[serde(default)]
    pub lexical_role: Option<LexicalRole>,
    #[serde(default)]
    pub senses: Vec<Sense>,
    #[serde(default)]
    pub conjugation: Option<Conjugation>,
    /// The parts of a compound word, in order (伝わって, きます). Empty otherwise.
    ///
    /// **This is the word's grammatical decomposition, which the source does not always spell.**
    /// Ichiran decomposes a contracted 食べてる as 食べて + いる, and いる is not in the text — it
    /// is the form the contraction stands for. Grammar detection, the lookup fallback and the
    /// explanations read these parts, so they are kept whole. Whether a part may be *offered to a
    /// reader as a piece of the passage* is a separate question, answered by [`Token::selectable`].
    #[serde(default)]
    pub components: Vec<Token>,
    /// **Whether a reader may select this token**: its spelling and position are verified against
    /// the source, a part lies inside the word it belongs to after its previous selectable sibling,
    /// and a word-part has a reading available — required to exist, not verified.
    ///
    /// Selection reads this rather than the parts list, because the parts list is grammar. A part
    /// that names an expanded form — いる in 食べてる — stays in `components` for everything that
    /// needs the morphology, and is simply not offered as a piece of the text. Nothing is trimmed
    /// to make it fit and no offset is invented for it.
    ///
    /// **Computed, never trusted.** [`crate::analyzer::mark_selectable`] sets it on every token
    /// and part of a finished analysis, from the source alone. False on anything that did not come
    /// through it, which is the safe reading.
    #[serde(default)]
    pub selectable: bool,
    /// Learner-facing constructions established from the same evidence that formed this token.
    /// Empty on older serialized tokens; consumers use one shared compatibility normalizer.
    #[serde(default)]
    pub constructions: Vec<Construction>,
    /// Readings the analyzer passed over.
    #[serde(default)]
    pub alternatives: Vec<Alternative>,
    /// The line break that follows this token, if any.
    ///
    /// Set by [`crate::lines::mark_line_breaks`] after analysis, because no analyzer reports one.
    #[serde(default)]
    pub line_break: LineBreak,
    /// Byte offset of this token in the source text; `None` if it could not be
    /// grounded (e.g. punctuation the analyzer normalized away).
    #[serde(default)]
    pub start: Option<usize>,
    #[serde(default)]
    pub end: Option<usize>,
    /// How sure the analyzer is of this reading, on its own scale.
    ///
    /// Ichiran's per-word score. **`None` under Lindera**, which reports nothing
    /// comparable — so a rule keyed on this does not fire there rather than acting on a
    /// number that was invented to fill the gap.
    ///
    /// Not a probability and not comparable across analyzers. The one thing it is good
    /// for is spotting a reading Ichiran itself barely believes: a katakana name folded
    /// onto a rare homophone scores single digits where a real word scores tens.
    ///
    /// An integer because Ichiran's scores are integers, and because `Token` is `Eq`.
    #[serde(default)]
    pub confidence: Option<i32>,
    /// The romanised reading, when the analyzer supplies one (Ichiran does).
    ///
    /// Worth carrying because it is the only useful thing left to say about a word no
    /// dictionary knows: シロコ means nothing to any source here, but *Shiroko* at least
    /// tells a reader how to say it.
    #[serde(default)]
    pub romaji: Option<String>,
    /// This reading was assembled character by character and nothing confirmed it.
    ///
    /// Set by `dict::names` on a run of single-kanji tokens the segmenter left adjacent and no name
    /// dictionary could rejoin — the signature of a compound it did not know. 善福寺川 is the case:
    /// four characters, four readings correct in isolation, and ぜん・ふく・てら・かわ over a river
    /// called ぜんぷくじがわ.
    ///
    /// **A caveat, not a claim.** It does not say the reading is wrong; it says the app has no
    /// source for it as a whole. It is the same honesty `merge::WordView::likely_name` offers for a
    /// katakana word nothing is spelled as — "both facts are true and neither is certain".
    ///
    /// It will sometimes fire on two ordinary one-character words that happen to be neighbours. That
    /// is the accepted cost: an unnecessary caveat is cheap, and a confident wrong reading is what
    /// this exists to prevent.
    #[serde(default)]
    pub reading_unverified: bool,
    /// The dictionary lists more than one reading for this spelling, and one of them was chosen.
    ///
    /// **The neighbouring field is the opposite problem.** `reading_unverified` means *nothing*
    /// vouched for this reading; this means *several sources vouched for different ones* and the
    /// app picked. Both are caveats, and they are not the same caveat.
    ///
    /// Set by `dict::readings`, which is the only layer that can: an analyzer has no dictionary
    /// (nothing under `analyzer/` imports `crate::dict`), so a flag set inside `analyze` would be
    /// Ichiran-only — the mistake `dict::merge` records at `NAME_WINS_BELOW`, where a caveat gated
    /// on `confidence` never appeared under Lindera at all.
    ///
    /// Measured at 69.2% correct over the 273 sentences in `tests/heteronym_readings.rs`, which is
    /// why the reader draws it rather than keeping it to itself. A learner cannot tell a guessed
    /// reading from a certain one, and this is the app saying which it is.
    ///
    /// **What it does not cover.** It finds a *spelling* with several readings, not a *surface*
    /// with several lemmas. 行った is flagged because 行く is いく or ゆく — not because the word
    /// might be 行う. That needs deinflection knowledge this pass does not have.
    #[serde(default)]
    pub reading_ambiguous: bool,
}

/// The half-open token range of the sentence containing `index`.
///
/// **Shared, and it must stay that way.** `grammar::particles` reads cues within a sentence
/// and the translation strip builds its variants from one; two copies of this rule would let
/// the particle panel and the strip disagree about where the sentence ends while both looked
/// right. `anki::sentence::bounds` is a *third* rule with a different terminator set — it
/// answers a different question (what goes on a card) and is deliberately left alone.
///
/// **A cue may not read across a full stop.** `Predicate` looks forward for a verb and `SecondWa`
/// counts topics; both used to range over the entire token list, and the interface passes the
/// whole analysed passage because a capture is a page, not a line. The result was that unrelated
/// text decided a particle — confidently, with the winner's accent treatment and a `because` line
/// naming a verb the reader could not see anywhere near it.
///
/// The Japanese terminators, ASCII `!` and `?`, and a line break. ASCII `.` is deliberately not
/// one: the corpus carries sentences with English fragments and decimals in them, and splitting
/// `3.14` into two sentences would trade one silent error for another. `!` and `?` carry no
/// equivalent cost — nothing writes `3!14` — and game text uses the halfwidth pair as freely as
/// the fullwidth one.
///
/// **The line break arrives as [`LineBreak::Break`], not as a character.** It was listed here as
/// `'\n'` for a long time and never matched: no analyzer emits a token carrying one, so a
/// punctuation-free capture was one sentence however many lines it had. `crate::analyzer::lines`
/// records it against the token it follows, and decides there whether the line merely wrapped.
///
/// **This is the only copy of the rule.** `anki::sentence` held a second one that split on
/// `。！？!?` and *not* on a newline, so the sentence on a card could differ from the one on
/// screen and the one that was translated — a two-line manga capture was one sentence to the
/// card and two to the reader, while `Nice!すごい。` split for the card and not for the screen. It now
/// calls this function.
///
/// A fragment that ends without a terminator has no boundary to find, and its range runs on into
/// whatever follows. That is the honest floor rather than a bug: 0.8% of the corpus is quoted
/// dialogue closing on 」, and it is the whole of the 0.5% residual left after this fix — against
/// the 96% it replaced. `」` is not added to the set because a sentence can continue after one.
pub fn sentence_bounds(tokens: &[Token], index: usize) -> (usize, usize) {
    let start = tokens
        .get(..index)
        .unwrap_or_default()
        .iter()
        .rposition(ends_sentence)
        .map(|at| at + 1)
        .unwrap_or(0);
    let end = tokens
        .get(index..)
        .unwrap_or_default()
        .iter()
        .position(ends_sentence)
        .map(|at| index + at + 1)
        .unwrap_or(tokens.len());
    (start, end)
}

/// One sentence in an [`Analysis`], as a half-open token range.
///
/// A named type at the UI boundary is intentional: a pair of `usize`s says nothing about whether
/// its end is inclusive, while a range is the unit a comparison selection is allowed to inhabit.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentenceRange {
    pub first_token: usize,
    pub last_token_end: usize,
}

impl SentenceRange {
    pub fn contains(self, index: usize) -> bool {
        (self.first_token..self.last_token_end).contains(&index)
    }
}

/// Partition `tokens` into the same sentences [`sentence_bounds`] answers for one token.
///
/// This is the collection-shaped half of that function, not another boundary rule. The desktop
/// sends it with an analysis so the reader can keep a comparison inside one sentence without
/// reimplementing Japanese sentence detection in TypeScript.
pub fn sentence_ranges(tokens: &[Token]) -> Vec<SentenceRange> {
    let mut ranges = Vec::new();
    let mut first = 0;
    for (index, token) in tokens.iter().enumerate() {
        if ends_sentence(token) {
            ranges.push(SentenceRange {
                first_token: first,
                last_token_end: index + 1,
            });
            first = index + 1;
        }
    }
    if first < tokens.len() {
        ranges.push(SentenceRange {
            first_token: first,
            last_token_end: tokens.len(),
        });
    }
    ranges
}

fn ends_sentence(token: &Token) -> bool {
    // `'\n'` is deliberately **not** in this list. It was, for years, and it never fired: no
    // analyzer puts a newline into a token, so the arm was dead and a punctuation-free capture
    // arrived as one sentence however many lines it had. The break reaches here as
    // [`LineBreak::Break`] instead, decided once by [`crate::lines`] where the text is still at
    // hand and the parser can say whether the line was wrapped.
    token.line_break == LineBreak::Break
        || token
            .surface
            .chars()
            .any(|c| matches!(c, '。' | '！' | '？' | '!' | '?'))
}

/// Whether `text` carries no hiragana at all — the runtime proxy for "this is nominal".
///
/// **A decision rule, living where [`contains_japanese`] lives, for the reason that one does.**
/// `Token` carries no analyzer-native part of speech by the time anything downstream sees it, so
/// nounhood has to be approximated from the characters, and hiragana is the signal that survives.
/// It excludes それは, には, のに and でも by construction while admitting 世界遺産, テレビ放送 and
/// コラーゲン鍋.
///
/// It decides real joins: `dict::merge_compounds` uses it to license joining a dictionary-known
/// run, measured at 99.8% not-wrong against gold where the unrestricted rule was wrong half the
/// time. **It was written three times before it was written once** — in that module, and again in
/// each of the two harnesses that measured it — so the number that justified shipping the rule was
/// computed by a different copy of the rule. That is precisely the failure the note below warns
/// about, committed one function above the warning.
pub fn is_kana_free(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(|c| ('\u{3041}'..='\u{309F}').contains(&c))
}

/// Whether `text` holds a real Japanese word character — kana or a kanji letter — as
/// opposed to being only punctuation or another script.
///
/// Excludes the ・ middle dot and full/half-width punctuation, which is the distinction
/// that makes this useful: 。、・ is Japanese *text* but contains no word.
///
/// One table, because two places need the answer and they must not disagree. [`Token`]
/// asks it to decide what is vocabulary; the clipboard watcher asks it to decide whether
/// what was just copied is worth reading. A second character list written for the second
/// caller is how the app ends up with two opinions about what Japanese is.
/// Whether this token is the explanatory な + ん group, by shape rather than by spelling.
///
/// **The spelling なん is not evidence and must never be treated as any.** なんか and なんですか both
/// begin with it, and the first is the pronoun 何 — UniDic says 代名詞, a different part of speech
/// entirely. What identifies the construction is the pair underneath: the attributive copula な
/// and the nominalising ん that contracts の, which is why the components are asked about and the
/// surface only confirms what they already say.
///
/// One recogniser, because three places need the same answer — the grammar match, the vocabulary
/// answer, and the tests — and three string comparisons that drift apart are how a construction
/// comes to be explained in one panel and not the other.
pub fn is_explanatory_nan(token: &Token) -> bool {
    crate::construction::has(token, ConstructionIdentity::ExplanatoryNan)
}

pub fn contains_japanese(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{3041}'..='\u{3096}'   // hiragana letters
            | '\u{309D}'..='\u{309E}' // hiragana iteration marks
            | '\u{30A1}'..='\u{30FA}' // katakana letters
            | '\u{30FC}'..='\u{30FE}' // ー and katakana iteration marks (not ・ U+30FB)
            | '\u{3400}'..='\u{9FFF}' // CJK ideographs
            | '\u{F900}'..='\u{FAFF}' // CJK compatibility ideographs
            | '\u{FF66}'..='\u{FF9D}' // halfwidth katakana
        )
    })
}

impl Token {
    /// Whether this token has a real word character (kana or kanji letter), as
    /// opposed to pure punctuation. Punctuation is analysed as tokens but is not
    /// vocabulary. Excludes the ・ middle dot and full/half-width punctuation.
    pub fn is_word(&self) -> bool {
        contains_japanese(&self.surface) || contains_japanese(&self.dict_form)
    }

    /// The parts of this token, if it is a compound (two or more components).
    pub fn is_compound(&self) -> bool {
        self.components.len() >= 2
    }
}

/// Which analyzer the reader asked for, as the effective policy the passage ran under.
///
/// The saved choice resolved with the `SAKATSU_ANALYZER` override, so what is recorded is what was
/// actually in force. Mirrors the store's `AnalyzerChoice` without its legacy variant, which
/// `Settings::normalise` removes before anything runs.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzerPolicy {
    /// Ichiran when it answers, the built-in analyzer otherwise.
    #[default]
    Automatic,
    /// Ichiran, asked for by name.
    Ichiran,
    /// The built-in analyzer (Lindera), asked for by name.
    BuiltIn,
}

/// Why a passage was not analysed by the engine its policy prefers.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    /// Ichiran is still being checked at startup; the built-in analyzer read this meanwhile.
    Starting,
    /// Ichiran is not installed, not reachable, or failed to run.
    Unavailable,
    /// Ichiran did not answer within its time budget.
    TimedOut,
    /// Ichiran answered with output that could not be read.
    MalformedOutput,
    /// Ichiran's answer did not satisfy the span contract.
    InvalidSpans,
}

/// Whether a fallback was about this passage or the whole session.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackScope {
    /// Ichiran was asked for this passage and failed; the built-in analyzer retried it once.
    Passage,
    /// Ichiran was not asked at all, because it is not available to this session (or not yet).
    Session,
}

/// A recorded fallback: the passage was read by the built-in analyzer instead.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fallback {
    pub reason: FallbackReason,
    pub scope: FallbackScope,
    /// The technical account, for the expandable detail. Never the passage.
    pub detail: String,
}

/// Which contracted verb form a repair recovered.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractionFamily {
    /// `～ておる` / `～でおる`, contracted to `～とる` / `～どる`: 間違っとる.
    TeOru,
    /// `～なければ`, contracted to `～なきゃ` / `～なけりゃ`, nasalised or not: 怒んなきゃ.
    NegativeConditional,
}

/// Which of the dictionary entry's recorded spellings spelled the stem the source shows.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemEvidence {
    /// The headword: 間違 from 間違う.
    Headword,
    /// The reading, for a stem the source writes in kana: かか from かかる.
    Reading,
}

/// **A contracted form the analyzer split, rejoined from the dictionary** (`lexical::repair`).
///
/// Linguistic evidence, never a fallback: the passage was still read by the engine
/// [`Execution::engine`] names. Recorded so the reader can say where the word came from and a
/// measurement can count exactly what changed.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairNote {
    /// Byte span of the repaired word in the source.
    pub start: usize,
    pub end: usize,
    pub family: ContractionFamily,
    /// `ら → ん` before `な`: 怒んなきゃ rather than 怒らなきゃ.
    pub nasal: bool,
    /// The entry's headword and its reading.
    pub lemma: String,
    pub lemma_reading: String,
    /// The derived reading of the whole span.
    pub reading: String,
    /// The entry's sequence, when the dictionary gives one. `number` for the reason
    /// [`Sense::sequence`] gives.
    #[cfg_attr(feature = "typescript", ts(as = "Option<i32>"))]
    pub sequence: Option<i64>,
    pub stem: StemEvidence,
    /// How many of the analyzer's tokens the word replaced.
    pub replaced: u32,
}

/// **How this analysis was produced**, recorded at execution — never read back afterwards from
/// whichever engine happens to be current.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Execution {
    /// The policy in force when the passage was analysed.
    pub requested: AnalyzerPolicy,
    /// The engine that actually produced the tokens: `"ichiran"`, `"lindera"`, or another name.
    pub engine: String,
    /// Set whenever the policy's preferred engine did not produce this passage — including when
    /// that is shown only as a quiet label. Provenance is always recorded; presentation decides
    /// how loudly to say it.
    pub fallback: Option<Fallback>,
    /// Contracted forms rejoined after the engine answered — empty when there were none, and
    /// always empty for the built-in analyzer, which composes them itself.
    #[serde(default)]
    pub repairs: Vec<RepairNote>,
}

/// The full analysis of a captured passage: the source text and its tokens.
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Analysis {
    pub text: String,
    pub tokens: Vec<Token>,
    /// How it was produced. `None` means unknown: an analysis from before this was recorded, or
    /// one made outside the reader's path. Set only by `study::analyse_resilient`.
    #[serde(default)]
    pub execution: Option<Execution>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_with_kana_or_kanji_is_vocabulary_but_punctuation_is_not() {
        let word = Token {
            surface: "先生".into(),
            dict_form: "先生".into(),
            ..Default::default()
        };
        assert!(word.is_word());

        // The full-width period ． (U+FF0E) that used to render as a blank
        // "Definition unavailable" row.
        let dot = Token {
            surface: "．".into(),
            dict_form: "．".into(),
            ..Default::default()
        };
        assert!(!dot.is_word());

        // The ・ middle dot (U+30FB) is punctuation, not a katakana letter.
        let middle_dot = Token {
            surface: "・".into(),
            ..Default::default()
        };
        assert!(!middle_dot.is_word());

        // Halfwidth katakana and the long-vowel mark are words.
        assert!(Token {
            surface: "ｶﾀｶﾅ".into(),
            ..Default::default()
        }
        .is_word());
        assert!(Token {
            surface: "コーヒー".into(),
            ..Default::default()
        }
        .is_word());
    }

    /// The clipboard watcher's question: is this worth reading at all?
    ///
    /// Same table as `is_word`, so what matters here is the cases a token never sees —
    /// other scripts, mixed text, and a line of Japanese punctuation with no word in it.
    #[test]
    fn japanese_is_recognized_and_nothing_else_is() {
        for yes in [
            "先生",
            "ひらがな",
            "コーヒー",
            "ｶﾀｶﾅ",
            "I ate ラーメン",
            "人々",
        ] {
            assert!(contains_japanese(yes), "{yes} should count as Japanese");
        }

        for no in [
            "",
            "hello",
            "12345",
            "Привет",
            "안녕하세요",   // Korean is not Japanese, and hangul is not in the table
            "。、！？（）", // Japanese punctuation, but no word in it
            "・",
            // 々 (U+3005) on its own. Not an oversight: it is a repetition mark, so it
            // only ever follows the kanji it repeats — 人々 above is caught by the 人.
            // A lone 々 is no more a word than a lone ・.
            "々",
        ] {
            assert!(
                !contains_japanese(no),
                "{no:?} should not count as Japanese"
            );
        }
    }

    #[test]
    fn a_compound_has_two_or_more_components() {
        let mut token = Token {
            surface: "伝わってきます".into(),
            ..Default::default()
        };
        assert!(!token.is_compound());
        token.components = vec![
            Token {
                surface: "伝わって".into(),
                ..Default::default()
            },
            Token {
                surface: "きます".into(),
                ..Default::default()
            },
        ];
        assert!(token.is_compound());
    }

    /// The collection form and the point-query form are one operation seen two ways: every token
    /// belongs to exactly one range, and asking for that token's bounds returns that same range.
    ///
    /// **Two independent axes, because a sentence can end two ways.** A token may carry a written
    /// terminator, or a line may have ended after it, or both, or neither — and exhausting the
    /// product is what makes this a law rather than a handful of agreeing examples. The second
    /// axis is new: `'\n'` used to be listed among the terminators here, on the belief that a
    /// break arrives as a token of its own. It does not, so that arm tested nothing.
    ///
    /// Six tokens rather than eight. The product is `4^n` masks, and the old single axis was
    /// `2^n`; six keeps the whole sweep at about five thousand cases, which is still every
    /// arrangement and still runs in a blink.
    #[test]
    fn sentence_ranges_are_an_exact_partition_and_agree_with_bounds() {
        const TERMINATORS: [char; 5] = ['。', '！', '？', '!', '?'];
        for length in 0..=6 {
            for written in 0..(1usize << length) {
                for broken in 0..(1usize << length) {
                    let tokens: Vec<Token> = (0..length)
                        .map(|index| Token {
                            surface: if written & (1 << index) == 0 {
                                "語".into()
                            } else {
                                // Cycled rather than fixed: this law held for `。` while a second
                                // copy of the rule in `anki::sentence` disagreed about `!` and `?`,
                                // and a test using one character could not see it.
                                TERMINATORS[index % TERMINATORS.len()].to_string()
                            },
                            line_break: if broken & (1 << index) == 0 {
                                LineBreak::None
                            } else {
                                LineBreak::Break
                            },
                            ..Default::default()
                        })
                        .collect();
                    let ranges = sentence_ranges(&tokens);

                    let mut cursor = 0;
                    for range in ranges {
                        assert_eq!(range.first_token, cursor, "ranges must be adjacent");
                        assert!(range.first_token < range.last_token_end);
                        assert!(range.last_token_end <= tokens.len());
                        for index in range.first_token..range.last_token_end {
                            assert!(range.contains(index));
                            assert_eq!(
                                sentence_bounds(&tokens, index),
                                (range.first_token, range.last_token_end),
                                "written {written:b}, broken {broken:b}, token {index}"
                            );
                        }
                        cursor = range.last_token_end;
                    }
                    assert_eq!(cursor, tokens.len(), "every token appears exactly once");
                }
            }
        }
    }

    /// A `Wrap` is not a boundary, which is the whole reason it is a separate state.
    #[test]
    fn a_wrapped_break_does_not_end_a_sentence() {
        let tokens = [
            Token {
                surface: "語".into(),
                line_break: LineBreak::Wrap,
                ..Default::default()
            },
            Token {
                surface: "語".into(),
                ..Default::default()
            },
        ];
        assert_eq!(sentence_ranges(&tokens).len(), 1);
        assert_eq!(sentence_bounds(&tokens, 0), (0, 2));
    }

    #[test]
    fn old_tokens_without_lexical_role_remain_unknown() {
        let mut old = serde_json::to_value(Token {
            surface: "たち".into(),
            ..Token::default()
        })
        .unwrap();
        old.as_object_mut().unwrap().remove("lexical_role");
        let restored: Token = serde_json::from_value(old).unwrap();
        assert_eq!(restored.lexical_role, None);
    }
    #[test]
    fn the_model_round_trips_through_json() {
        let analysis = Analysis {
            execution: None,
            text: "水を飲む".into(),
            tokens: vec![Token {
                surface: "飲む".into(),
                dict_form: "飲む".into(),
                reading: "のむ".into(),
                senses: vec![Sense {
                    pos: "[v5m,vt]".into(),
                    gloss: "to drink".into(),
                    sequence: None,
                }],
                ..Default::default()
            }],
        };
        let json = serde_json::to_string(&analysis).expect("serialize");
        let back: Analysis = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(analysis, back);
    }
}
