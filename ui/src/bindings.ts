/**
 * GENERATED FROM THE RUST TYPES. Do not edit.
 *
 *   cargo run -p sakatsu-core --features typescript --bin generate-bindings
 *
 * `core/tests/typescript_contract.rs` fails when this file stops matching the Rust types, and
 * `npm run check` fails when the code that consumes it stops matching this file.
 *
 * This is one half of the IPC contract. The other is `model.golden.json`, which is generated from
 * real serialized bytes and decides any disagreement: these declarations describe the types as
 * Rust declares them, the golden describes what serde actually puts on the wire. What this half
 * adds is the enums — every variant of `LineBreak`, `WordStatus` and `Form` is a literal here, so
 * renaming one in Rust breaks the frontend that still uses the old name. A JSON import cannot do
 * that, which is why `model.contract.ts` has to widen those same unions back to `string`.
 */

export type ResourceKind = "words" | "kanjidic";

export type ImportPreview = { id: string, kind: ResourceKind, title: string, revision: string, sha256: string, source: string | null, };

export type DictionaryPreferences = { check_updates: boolean, automatic: boolean, };

export type ManagedDictionary = { id: string, kind: ResourceKind, title: string, revision: string, generation: string, source: string | null, home: string | null, entries: number | null, sha256: string | null, consented: boolean, phase: string, detail: string, available_revision: string | null, };

export type DictionaryOperation = { id: string, target: string, phase: string, detail: string, bytes: number, };

export type DictionaryManagerSnapshot = { preferences: DictionaryPreferences, dictionaries: Array<ManagedDictionary>, operation: DictionaryOperation | null, };

export type UpdatePhase = "not_configured" | "idle" | "checking" | "current" | "available" | "downloading" | "ready" | "installing" | "failed" | "offline";

export type UpdateStatus = { generation: number, phase: UpdatePhase, version: string | null, notes: string, downloaded: number, total: number | null, error: string | null, };

export type Sense = { 
/**
 * Part-of-speech / field / misc tags as the source gives them, e.g. "n,vs,vt".
 */
pos: string, 
/**
 * The gloss text, e.g. "memory; recollection; remembrance".
 */
gloss: string, 
/**
 * The JMdict sequence this sense came from, when the source states one.
 *
 * **Provenance the merge used to discard.** `dict::merge` collapses several entries' senses
 * into one list and `dict::context::RankedSense` indexes into that, so there was no way to
 * ask whether the ranker favoured the *right entry* — only which position it chose. That is
 * why sense ordering had never been measured. Carried here so it can be.
 */
sequence: number | null, };

export type Prop = { 
/**
 * The verb class, e.g. "v5u", "v1", "vk" — drives the formation generator.
 */
pos: string, 
/**
 * The form label as prose, e.g. "Past (~ta)", "Causative-Passive".
 */
form: string, neg: boolean, fml: boolean, 
/**
 * Depth in the via chain; 0 is the outermost applied form.
 */
via_depth: number, 
/**
 * The dictionary-form reading (kana) of the verb this form applies to.
 * Present on the base transformation.
 */
base_reading: string, 
/**
 * True for the deepest prop — the transformation on the dictionary verb.
 */
is_base: boolean, };

export type Conjugation = { lemma: string, 
/**
 * The lemma's reading (kana), normalized to hiragana.
 */
reading: string, props: Array<Prop>, };

export type Alternative = { lemma: string, reading: string, form: string, senses: Array<Sense>, };

export type LineBreak = "none" | "wrap" | "break";

export type ConstructionIdentity = "explanatory_nan" | "contracted_motion" | "copula_recall";

export type ConstructionLookup = "headword" | "grammar_pattern";

export type ConstructionGrammar = "sentence" | "auxiliary";

export type SourceSpan = { start: number, end: number, };

export type Construction = { identity: ConstructionIdentity, source: SourceSpan, 
/**
 * Indices into [`Token::components`], in source order.
 */
components: Array<number>, lookup: ConstructionLookup, grammar: ConstructionGrammar, 
/**
 * Resolved through the complete pack set; imported entries are valid destinations too.
 */
grammar_pattern: string, };

export type LexicalRole = "pronoun" | "suffix" | "noun";

export type Token = { 
/**
 * The text as written.
 */
surface: string, 
/**
 * The dictionary form.
 */
dict_form: string, 
/**
 * The surface reading (kana) — this drives furigana.
 */
reading: string, 
/**
 * The dictionary-form reading (kana). May differ from `reading`
 * (つたわって vs つたわる).
 */
lemma_reading: string, 
/**
 * Morphology the analyzer established independently of dictionary meanings.
 *
 * Optional for backward compatibility and because analyzers need not expose it. A composed
 * token does not inherit a constituent's role: the role describes the token itself.
 */
lexical_role: LexicalRole | null, senses: Array<Sense>, conjugation: Conjugation | null, 
/**
 * The parts of a compound word, in order (伝わって, きます). Empty otherwise.
 *
 * **This is the word's grammatical decomposition, which the source does not always spell.**
 * Ichiran decomposes a contracted 食べてる as 食べて + いる, and いる is not in the text — it
 * is the form the contraction stands for. Grammar detection, the lookup fallback and the
 * explanations read these parts, so they are kept whole. Whether a part may be *offered to a
 * reader as a piece of the passage* is a separate question, answered by [`Token::selectable`].
 */
components: Array<Token>, 
/**
 * **Whether a reader may select this token**: its spelling and position are verified against
 * the source, a part lies inside the word it belongs to after its previous selectable sibling,
 * and a word-part has a reading available — required to exist, not verified.
 *
 * Selection reads this rather than the parts list, because the parts list is grammar. A part
 * that names an expanded form — いる in 食べてる — stays in `components` for everything that
 * needs the morphology, and is simply not offered as a piece of the text. Nothing is trimmed
 * to make it fit and no offset is invented for it.
 *
 * **Computed, never trusted.** [`crate::analyzer::mark_selectable`] sets it on every token
 * and part of a finished analysis, from the source alone. False on anything that did not come
 * through it, which is the safe reading.
 */
selectable: boolean, 
/**
 * Learner-facing constructions established from the same evidence that formed this token.
 * Empty on older serialized tokens; consumers use one shared compatibility normalizer.
 */
constructions: Array<Construction>, 
/**
 * Readings the analyzer passed over.
 */
alternatives: Array<Alternative>, 
/**
 * The line break that follows this token, if any.
 *
 * Set by [`crate::lines::mark_line_breaks`] after analysis, because no analyzer reports one.
 */
line_break: LineBreak, 
/**
 * Byte offset of this token in the source text; `None` if it could not be
 * grounded (e.g. punctuation the analyzer normalized away).
 */
start: number | null, end: number | null, 
/**
 * How sure the analyzer is of this reading, on its own scale.
 *
 * Ichiran's per-word score. **`None` under Lindera**, which reports nothing
 * comparable — so a rule keyed on this does not fire there rather than acting on a
 * number that was invented to fill the gap.
 *
 * Not a probability and not comparable across analyzers. The one thing it is good
 * for is spotting a reading Ichiran itself barely believes: a katakana name folded
 * onto a rare homophone scores single digits where a real word scores tens.
 *
 * An integer because Ichiran's scores are integers, and because `Token` is `Eq`.
 */
confidence: number | null, 
/**
 * The romanised reading, when the analyzer supplies one (Ichiran does).
 *
 * Worth carrying because it is the only useful thing left to say about a word no
 * dictionary knows: シロコ means nothing to any source here, but *Shiroko* at least
 * tells a reader how to say it.
 */
romaji: string | null, 
/**
 * This reading was assembled character by character and nothing confirmed it.
 *
 * Set by `dict::names` on a run of single-kanji tokens the segmenter left adjacent and no name
 * dictionary could rejoin — the signature of a compound it did not know. 善福寺川 is the case:
 * four characters, four readings correct in isolation, and ぜん・ふく・てら・かわ over a river
 * called ぜんぷくじがわ.
 *
 * **A caveat, not a claim.** It does not say the reading is wrong; it says the app has no
 * source for it as a whole. It is the same honesty `merge::WordView::likely_name` offers for a
 * katakana word nothing is spelled as — "both facts are true and neither is certain".
 *
 * It will sometimes fire on two ordinary one-character words that happen to be neighbours. That
 * is the accepted cost: an unnecessary caveat is cheap, and a confident wrong reading is what
 * this exists to prevent.
 */
reading_unverified: boolean, 
/**
 * The dictionary lists more than one reading for this spelling, and one of them was chosen.
 *
 * **The neighbouring field is the opposite problem.** `reading_unverified` means *nothing*
 * vouched for this reading; this means *several sources vouched for different ones* and the
 * app picked. Both are caveats, and they are not the same caveat.
 *
 * Set by `dict::readings`, which is the only layer that can: an analyzer has no dictionary
 * (nothing under `analyzer/` imports `crate::dict`), so a flag set inside `analyze` would be
 * Ichiran-only — the mistake `dict::merge` records at `NAME_WINS_BELOW`, where a caveat gated
 * on `confidence` never appeared under Lindera at all.
 *
 * Measured at 69.2% correct over the 273 sentences in `tests/heteronym_readings.rs`, which is
 * why the reader draws it rather than keeping it to itself. A learner cannot tell a guessed
 * reading from a certain one, and this is the app saying which it is.
 *
 * **What it does not cover.** It finds a *spelling* with several readings, not a *surface*
 * with several lemmas. 行った is flagged because 行く is いく or ゆく — not because the word
 * might be 行う. That needs deinflection knowledge this pass does not have.
 */
reading_ambiguous: boolean, };

export type AnalyzerPolicy = "automatic" | "ichiran" | "built_in";

export type FallbackReason = "starting" | "unavailable" | "timed_out" | "malformed_output" | "invalid_spans";

export type FallbackScope = "passage" | "session";

export type Fallback = { reason: FallbackReason, scope: FallbackScope, 
/**
 * The technical account, for the expandable detail. Never the passage.
 */
detail: string, };

export type ContractionFamily = "te_oru" | "negative_conditional";

export type StemEvidence = "headword" | "reading";

export type RepairNote = { 
/**
 * Byte span of the repaired word in the source.
 */
start: number, end: number, family: ContractionFamily, 
/**
 * `ら → ん` before `な`: 怒んなきゃ rather than 怒らなきゃ.
 */
nasal: boolean, 
/**
 * The entry's headword and its reading.
 */
lemma: string, lemma_reading: string, 
/**
 * The derived reading of the whole span.
 */
reading: string, 
/**
 * The entry's sequence, when the dictionary gives one. `number` for the reason
 * [`Sense::sequence`] gives.
 */
sequence: number | null, stem: StemEvidence, 
/**
 * How many of the analyzer's tokens the word replaced.
 */
replaced: number, };

export type Execution = { 
/**
 * The policy in force when the passage was analysed.
 */
requested: AnalyzerPolicy, 
/**
 * The engine that actually produced the tokens: `"ichiran"`, `"lindera"`, or another name.
 */
engine: string, 
/**
 * Set whenever the policy's preferred engine did not produce this passage — including when
 * that is shown only as a quiet label. Provenance is always recorded; presentation decides
 * how loudly to say it.
 */
fallback: Fallback | null, 
/**
 * Contracted forms rejoined after the engine answered — empty when there were none, and
 * always empty for the built-in analyzer, which composes them itself.
 */
repairs: Array<RepairNote>, };

export type Analysis = { text: string, tokens: Array<Token>, 
/**
 * How it was produced. `None` means unknown: an analysis from before this was recorded, or
 * one made outside the reader's path. Set only by `study::analyse_resilient`.
 */
execution: Execution | null, };

export type SentenceRange = { first_token: number, last_token_end: number, };

export type Form = "causative-passive" | "causative" | "passive" | "potential" | "desiderative" | "volitional" | "imperative" | "provisional" | "conditional" | "alternative" | "conjunctive" | "continuative" | "adverbial" | "past" | "non-past" | "adjective-stem" | "contraction" | "unclassified";

export type Formation = { 
/**
 * The kana the form is built from, e.g. `"いう"`.
 */
from: string, 
/**
 * The change applied, e.g. `"う → わ, then add せる"`.
 */
change: string, 
/**
 * The resulting kana, e.g. `"いわせる"`.
 */
result: string, };

export type Step = { form: Form, 
/**
 * The analyzer's own wording, kept so an unclassified form can still be shown.
 */
label: string, 
/**
 * The conjugation class this step applies to, e.g. `"v1"`.
 */
pos: string, neg: boolean, fml: boolean, 
/**
 * The dictionary form this step's chain resolves to, e.g. 食べる.
 */
lemma: string, 
/**
 * The dictionary form's reading in kana, on the base step.
 */
base_reading: string, 
/**
 * Depth in the analyzer's chain; larger is deeper (closer to the lemma).
 */
via_depth: number, 
/**
 * True for the transformation applied directly to the dictionary word.
 */
is_base: boolean, 
/**
 * How this word builds this form, when it can be derived without guessing.
 */
formation: Formation | null, 
/**
 * For a compound, which component this step came from.
 */
component: number | null, };

export type Auxiliary = { 
/**
 * Index of the auxiliary component within the token.
 */
component: number, 
/**
 * The pattern it forms, as a [grammar pack](super::pack) id.
 */
pattern_id: string, };

export type Breakdown = { 
/**
 * The transformations, in the order the word is built up: the dictionary form
 * first, then each layer applied to it.
 */
steps: Array<Step>, 
/**
 * Auxiliary patterns, when the token is a compound.
 */
auxiliaries: Array<Auxiliary>, };

export type Match = { 
/**
 * The [`GrammarPattern`](super::pack::GrammarPattern) id; look the text up in
 * the pack it came from.
 */
pattern_id: string, 
/**
 * Index of the first token in the match.
 */
first_token: number, 
/**
 * Index one past the last token in the match.
 */
last_token_end: number, 
/**
 * Byte span in the source text, when every token in the match was grounded.
 */
start: number | null, end: number | null, 
/**
 * What each element of the rule consumed, **as backtracking finally resolved it**.
 *
 * One entry per rule position, in rule order, so a caller can name `elements[0]` and
 * `elements[2]` — the two endpoints of `AとかBとか` — instead of scanning the match for
 * characters and hoping it finds the same thing the recogniser did.
 *
 * **Empty is possible and means "no element spans", never "the rule had none".** A match
 * placed from a validated construction annotation
 * ([`grammar::structural`](super::structural)) has no rule behind it, and a `Match`
 * deserialized from an interface that predates this field arrives without one. Both are
 * absences: a caller that needs an element span finds none and offers nothing, rather than
 * falling back to a guess.
 */
elements: Array<ElementSpan>, };

export type ElementSpan = { 
/**
 * The element's position in the rule.
 */
element: number, 
/**
 * Index of the first token this element consumed.
 */
first_token: number, 
/**
 * Index one past the last token this element consumed. **Equal to `first_token` where a
 * `{min: 0}` part consumed nothing**, which is recorded rather than omitted so that every
 * later position keeps the index the rule gives it.
 */
last_token_end: number, 
/**
 * Byte span of the consumed tokens, on the same terms as [`Match::start`] — the first
 * token's start and the last one's end, when both were grounded.
 */
start: number | null, end: number | null, 
/**
 * The exact source span of the **ending** a `surface_ends` or `surface_ends_any` element
 * matched on. `None` for every other kind of element, and `None` where the ending cannot be
 * located in the source — an ungrounded token, a run that is not byte-contiguous, or a token
 * whose surface is not what the source says.
 */
suffix_start: number | null, suffix_end: number | null, };

export type ExpressionOffer = { 
/**
 * The expression as written, e.g. `のに`.
 */
expression: string, 
/**
 * The grammar pattern that recognised it, so the interface can link the two.
 */
pattern_id: string, 
/**
 * The pack entry's own title, already resolved — an unresolvable id never gets this far.
 */
title: string, 
/**
 * Byte range in the analysed text.
 */
start: number, end: number, 
/**
 * How the expression's span stands to the selected token's.
 *
 * Decided by comparing validated spans, never by comparing spelling: two tokens can spell
 * the same characters and cover different source, and the label the reader sees is a claim
 * about *this* selection's extent.
 *
 * **One field rather than a pair of booleans**, because the relationship has four states and
 * two booleans can encode a fifth that does not exist. See [`SelectionExtent`].
 */
extent: SelectionExtent, 
/**
 * Set when this offer is one **occurrence inside** a larger recognised construction, naming
 * the construction's own extent. `None` for a whole-span offer.
 */
occurrence: Occurrence | null, 
/**
 * Every sense the dictionary records, in its order. **No favourite**: see the module docs.
 */
senses: Array<MergedSense>, 
/**
 * Which dictionary answered.
 */
source: string, };

export type Occurrence = { 
/**
 * The rule position that consumed this occurrence.
 */
element: number, 
/**
 * The construction's own byte span, which the offer deliberately does not claim.
 */
start: number, end: number, };

export type SelectionExtent = "same" | "contains_selection" | "within_selection" | "overlapping" | "unknown";

export type Ruby = { text: string, reading: string | null, };

export type DictionaryUpkeep = { 
/**
 * The name this dictionary is installed under — see [`index::Dictionary::installed_as`].
 *
 * **The identity, where `title` is only a label.** Two installed dictionaries can share a
 * title, so a list keyed on one collapses them into a single row and offers a rebuild for
 * whichever it happened to keep.
 */
id: string, 
/**
 * The dictionary's own title, as its `index.json` gives it. For showing, not for keying.
 */
title: string, 
/**
 * English glosses are searchable.
 */
has_glosses: boolean, 
/**
 * A kanji's compounds can be listed.
 */
has_kanji: boolean, 
/**
 * A word's other spellings can be found — and, because a cross-reference is filed under the
 * word it points at, this is the difference between a definition and an arrow.
 */
has_sequence: boolean, 
/**
 * Whether any of the above is missing, which is exactly what the backfill would build.
 */
needs_indexes: boolean, };

export type WordStatus = "new" | "seen" | "known" | "ignored";

export type StudyView = { analysis: Analysis, 
/**
 * The exact partition the core's sentence-sensitive features use.
 *
 * Sent with the analysis so the reader can keep a comparison inside one sentence without
 * copying the boundary rule into the UI.
 */
sentences: Array<SentenceRange>, 
/**
 * Parallel to `analysis.tokens` — one breakdown per token, empty where the word
 * has no grammar to explain.
 */
breakdowns: Array<Breakdown>, 
/**
 * Grammar patterns spanning the token sequence.
 */
patterns: Array<Match>, 
/**
 * Parallel to `analysis.tokens` — how well the learner knows each word.
 */
statuses: Array<WordStatus>, 
/**
 * Parallel to `analysis.tokens` — how each word should be set, with the kana over the
 * characters they belong to where that can be established.
 *
 * Computed here rather than asked for per word: the reader draws every token at once,
 * and a round trip each would be hundreds for a page.
 */
ruby: Array<Array<Ruby>>, };

export type MergedSense = { tags: Array<string>, glosses: Array<string>, 
/**
 * Which dictionary entry this sense came from, when the source states one.
 *
 * **The merge used to lose this, and losing it made the ordering unmeasurable.** Several
 * entries' senses collapse into one `inline` list here, and `dict::context::RankedSense`
 * indexes into that list — so "the ranker favoured sense 2" could never be turned back into
 * "the ranker favoured 前【ぜん】", and the only ground truth available for sense choice is
 * filed *by entry*: a Jitendex example sentence sits under a JMdict sequence.
 */
sequence: number | null, };

export type Retrieved = { "kind": "unknown" } | { "kind": "whole", 
/**
 * What was asked — the token's dictionary form, or its surface when it has none.
 */
query: string, } | { "kind": "component", 
/**
 * The component's dictionary form, as queried.
 */
query: string, };

export type DictExample = { japanese: string, english: string, retrieved: Retrieved, };

export type AlsoIn = { title: string, 
/**
 * What this source answered. Missing on historical cached responses.
 */
retrieved: Retrieved, 
/**
 * **What this source adds, not everything it says.**
 *
 * Senses whose every gloss is already inline are dropped, because the disclosure is
 * titled *also in* — it is framed as "what else", and reprinting the five senses the
 * reader has just read does not represent the source more faithfully, it buries the one
 * sense that would have.
 *
 * Whole senses only. See [`adds_anything_new`] for why a partly-new sense is kept
 * intact rather than trimmed to its new glosses.
 */
senses: Array<MergedSense>, };

export type NameHit = { 
/**
 * The name itself: "Seia".
 */
glosses: Array<string>, 
/**
 * The dictionary's own codes — `fem`, `surname`, `place`.
 *
 * Raw codes rather than prose, so the page can draw them with the same chip a sense's
 * `n` or `adv` wears and explain them from the same tag glossary. A name is a kind of
 * answer, not a different kind of thing, and it should not need its own vocabulary of
 * labels to say so.
 */
tags: Array<string>, 
/**
 * Always named, for the same reason a promoted definition is.
 */
source: string, 
/**
 * Whether the name answer belongs to the selected token or one of its components.
 */
retrieved: Retrieved, 
/**
 * Whether this should be shown *before* the word senses.
 *
 * Two ways to earn it: the analyzer barely believed its own reading (see
 * [`NAME_WINS_BELOW`]), or there are no senses for it to come before — a name that is
 * the only answer found is the answer, and folding it away would hide the page's one
 * piece of information.
 */
leads: boolean, };

export type WordView = { headword: string, 
/**
 * Adopted only from a source that agrees with itself; `None` when nothing does.
 */
reading: string | null, 
/**
 * What shows without the learner having to open anything.
 */
inline: Array<MergedSense>, 
/**
 * The source of `inline` when it is **not** the primary — always named, because
 * a promoted definition the learner cannot attribute is worse than none.
 */
inline_source: string | null, 
/**
 * What the inline definitions answer. Kept separate from every other answer group because
 * one dictionary may answer a component while another supplies a whole-token name.
 */
inline_retrieved: Retrieved, 
/**
 * Quietly disclosed: the other sources that also have this word.
 */
also_in: Array<AlsoIn>, 
/**
 * Attached to the word, gathered from any source, in source order.
 */
examples: Array<DictExample>, 
/**
 * A name this word may be. `None` when no name dictionary has it.
 */
name: NameHit | null, 
/**
 * Nothing at all has this word and it is written wholly in katakana.
 *
 * In manga that is usually a name — the character the panel is about — and
 * occasionally an effect word. Worth saying, but only as a guess: the page must not
 * assert a meaning here, because there is none to assert.
 */
likely_name: boolean, 
/**
 * How to say it, when the analyzer romanised it. The only useful thing left when no
 * dictionary knows the word at all.
 */
romaji: string | null, };

export type VocabularySenseRank = { 
/**
 * Where this sense appeared before contextual ordering.
 */
original_index: number, 
/**
 * Additive evidence. Zero means the context supplied no usable distinction.
 */
score: number, 
/**
 * Short, learner-readable statements describing only evidence that actually fired.
 */
reasons: Array<string>, };

export type VocabularyRanking = { 
/**
 * Parallel to the returned word's `inline` senses, after ordering.
 */
senses: Array<VocabularySenseRank>, 
/**
 * Index in that reordered list, only for a unique positive winner.
 */
favoured: number | null, };

export type ContextualWordView = { word: WordView, ranking: VocabularyRanking, };

export type CharReading = { text: string, 
/**
 * True for the reading this character supplies in the word searched for.
 *
 * False on every reading is not "none of these" — it also covers a word whose
 * reading is not built from its characters at all. [`KanjiView::irregular`] is what
 * separates the two.
 */
here: boolean, };

export type KanjiView = { character: string, on: Array<CharReading>, kun: Array<CharReading>, 
/**
 * Name-only readings. Never marked as the reading used by an ordinary word.
 */
nanori: Array<string>, meanings: Array<string>, strokes: number, grade: number | null, 
/**
 * Visible parts from basic KRADFILE, not necessarily traditional radicals.
 */
components: Array<string>, 
/**
 * The searched word's reading is not built from its characters' own readings —
 * 明日 あした. Nothing is marked, and the page says why rather than leaving the
 * learner to wonder which reading applies.
 */
irregular: boolean, };

export type SearchForm = { text: string, 
/**
 * The dictionary scores this form below zero.
 *
 * Which is its way of saying rarely-written or an outdated character form — EDRDG's
 * `rK` and `oK`. Jitendex leaves `term_tags` empty, so the score is the only signal
 * there is, and it agrees with the documented tags everywhere it was checked: 付近 200
 * against 附近 -101 (`rK`), 竜 200 against 龍 -101 (`oK`). A *positive* alternative like
 * 鮨 at 99 is an ordinary second spelling and is not marked.
 */
rare: boolean, };

export type WordHit = { 
/**
 * Other spellings of this same dictionary entry, commonest first.
 */
forms: Array<SearchForm>, 
/**
 * This word's characters, `here` marking the reading this word lends each one.
 */
chars: Array<KanjiView>, 
/**
 * Sentence-local sense evidence, only when a Japanese Reference query contains more than
 * one analyzable word. A single word and every English query deliberately leave this empty:
 * without neighbouring language, Reference has no context from which to make a preference.
 */
ranking: VocabularyRanking | null, 
/**
 * The form this hit was found from — what the reader actually typed or clicked.
 *
 * 走った arrives and 走る is answered, and the page said nothing about the difference; on a
 * page for learners that difference is the lesson rather than plumbing to hide.
 *
 * **A fact, not a decision.** Whether it is worth showing is `surfaceWorthShowing` in
 * `ui/src/lib/kana.js`, which the selected-word header already uses — one rule, in the one
 * place both surfaces that draw the chip can reach. This reports; the interface decides.
 * `None` for every English query, for the same reason `ranking` is: an English query makes no
 * claim about how a Japanese word was written.
 */
surface: string | null, headword: string, 
/**
 * Adopted only from a source that agrees with itself; `None` when nothing does.
 */
reading: string | null, 
/**
 * What shows without the learner having to open anything.
 */
inline: Array<MergedSense>, 
/**
 * The source of `inline` when it is **not** the primary — always named, because
 * a promoted definition the learner cannot attribute is worse than none.
 */
inline_source: string | null, 
/**
 * What the inline definitions answer. Kept separate from every other answer group because
 * one dictionary may answer a component while another supplies a whole-token name.
 */
inline_retrieved: Retrieved, 
/**
 * Quietly disclosed: the other sources that also have this word.
 */
also_in: Array<AlsoIn>, 
/**
 * Attached to the word, gathered from any source, in source order.
 */
examples: Array<DictExample>, 
/**
 * A name this word may be. `None` when no name dictionary has it.
 */
name: NameHit | null, 
/**
 * Nothing at all has this word and it is written wholly in katakana.
 *
 * In manga that is usually a name — the character the panel is about — and
 * occasionally an effect word. Worth saying, but only as a guess: the page must not
 * assert a meaning here, because there is none to assert.
 */
likely_name: boolean, 
/**
 * How to say it, when the analyzer romanised it. The only useful thing left when no
 * dictionary knows the word at all.
 */
romaji: string | null, };

export type Found = { words: Array<WordHit>, 
/**
 * Which way the query was read. The page used to work this out again from its own
 * regex and disagree with this one about halfwidth katakana.
 */
japanese: boolean, 
/**
 * The corpus the word results were ordered by, when one is installed.
 *
 * Named rather than implied. BCCWJ, JPDB and Innocent Corpus counted different
 * Japanese and disagree about what is common, so "most common first" is only a
 * meaningful claim with the corpus attached to it.
 */
ranked_by: string | null, 
/**
 * The query was English and no dictionary has a gloss index yet.
 *
 * Distinct from "no results", and the interface must not conflate them: one means
 * the word is not in the dictionary, the other means nothing was looked at.
 */
needs_gloss_index: boolean, 
/**
 * More entries matched than are being returned.
 *
 * **A boolean and not a number, deliberately.** The count available cheaply is of dictionary
 * *entries*, and several spellings of one word collapse into one result — so a figure taken
 * from it would overstate what a reader would actually see. What is measured here is exactly
 * what is claimed: one more distinct entry existed than fitted. A true total needs the
 * sequence number the index does not store; when that bucket is built, this can become one.
 *
 * Always `false` for a Japanese query, which returns one hit per token and never truncates.
 */
capped: boolean, };

export type Tier = "essential" | "optional" | "advanced";

export type SetupPhase = "checking" | "ready_to_install" | "downloading" | "paused_retrying" | "installing" | "verifying" | "complete" | "needs_attention";

export type SetupArtifactState = { file: string, version: string, immutable_source: string, required: boolean, expected_download_bytes: number, expected_installed_bytes: number, sha256: string, downloaded_bytes: number, partial_file: string | null, transfer_job: string | null, verification: string, };

export type SetupComponentState = { id: string, label: string, required: boolean, installed: boolean, installed_version: string | null, transfer_state: string, downloaded_bytes: number, total_bytes: number, verification: string, repair_eligible: boolean, artifacts: Array<SetupArtifactState>, };

export type StorageSummary = { target: string, download_bytes: number, installed_bytes: number, staging_bytes: number, peak_bytes: number, available_bytes: number | null, reserve_bytes: number, };

export type SetupSnapshot = { phase: SetupPhase, generation: number, required_ready: boolean, 
/**
 * New first-run journeys keep the analyzer decision across WSL/Windows restarts.
 */
analyzer_setup_pending: boolean, resumable: boolean, text_only: boolean, welcome_seen: boolean, existing_install: boolean, current_component: string | null, current_issue: string | null, storage: StorageSummary, components: Array<SetupComponentState>, };

export type SetupPreflight = { target_location: string, writable: boolean, required_bytes: number, available_bytes: number | null, peak_bytes: number, enough_space: boolean, network_available: boolean, supported_windows: boolean, supported_architecture: boolean, webview2: boolean, blocking_issue: string | null, };

export type CatalogSource = { name: string, licence: string, home: string, };

export type CatalogEntry = { id: string, label: string, about: string, licence: string, 
/**
 * Where the data comes from. A link, because for some of it a link is a licence condition.
 */
home: string, 
/**
 * Separately linkable sources when one setup action builds several licensed artifacts.
 */
sources: Array<CatalogSource>, 
/**
 * Rendered, because the number is only there to be read: "50 MB".
 */
size: string, bytes: number, tier: Tier, 
/**
 * Whether the app has to take this up after fetching it. See `Component::needs_reload`.
 *
 * The interface no longer *asks* for anything on the strength of this — `commands::adopt`
 * has already done the reload by the time `fetch-done` arrives. It is still reported
 * because the screens use it to say what is happening while it happens, and because
 * `tests/ipc.rs` pins the shape of this struct.
 */
needs_reload: boolean, 
/**
 * False for Ichiran, which is installed by a guided terminal rather than the
 * ordinary artifact downloader.
 */
fetchable: boolean, 
/**
 * The absolute path of a bundled companion script, resolved under the installed
 * resource directory. Guided system setup launches this through its own command;
 * ordinary artifact downloads use it only as provenance and a developer fallback.
 */
script: string | null, ready: boolean, 
/**
 * A legacy install remains usable, but lacks the supplemental artifacts now offered.
 */
update_available: boolean, };

export type FetchProgress = { 
/**
 * The component id the setup screens key their rows by.
 */
id: string, 
/**
 * The file being fetched, or what is happening when there is no file — "importing…".
 */
file: string, step: number, steps: number, done: number, 
/**
 * Null when the server sent no length, so a bar has to cope with not knowing.
 */
total: number | null, speed: number | null, eta: number | null, attempt: number, };

export type SetupProgress = { 
/**
 * The snapshot this belongs to. The store drops progress from an older generation.
 */
generation: number, component: string, artifact: string, step: number, steps: number, done: number, total: number | null, speed: number | null, eta: number | null, attempt: number, };

export type ErrorCode = "store_unavailable" | "settings_read_only" | "write_failed" | "stale_recovery";

export type PersistenceError = { code: ErrorCode, 
/**
 * The whole sentence, from the side that knows what happened. **Never prefixed** by anything
 * that passes it on — that is how a degraded session came to say the same thing three times,
 * nested.
 */
message: string, };

export type DictionaryRecoveryState = "unresolved" | "conflicting" | "cleanup_only" | "never_switched" | "unreadable";

export type DictionaryRecovery = { 
/**
 * What to call it — the container's own directory name, which is what the reader installed.
 */
name: string, 
/**
 * The full path, for a diagnostic somebody can copy.
 */
path: string, state: DictionaryRecoveryState, 
/**
 * The transaction an action must name to be accepted. Absent only when the record is
 * unreadable, which is the one case where nothing can be acted on.
 */
transaction: string | null, 
/**
 * The generation that transaction published, which an action must also name.
 */
published: string | null, 
/**
 * What the container pointed at before — a generation, or `null` for a legacy layout.
 */
previous: string | null, 
/**
 * What it points at now, when that is neither of the above.
 */
live: string | null, 
/**
 * Why it cannot be read, for [`DictionaryRecoveryState::Unreadable`].
 */
detail: string | null, 
/**
 * Whether a repair has already been attempted and failed.
 */
already_tried: boolean, };

export type InstallRecoveryState = "restored" | "retained" | "failed" | "competing" | "unidentified";

export type LoadingOutcome = "loaded" | "failed" | "not_needed" | "unknown";

export type InstallRecovery = { 
/**
 * The component id, as the catalog names it.
 */
id: string, state: InstallRecoveryState, 
/**
 * Where it was going, or is now.
 */
destination: string | null, 
/**
 * Where the bytes are, when they are not at the destination.
 */
saved: string | null, 
/**
 * Every abandoned install offering the same destination, for
 * [`InstallRecoveryState::Competing`].
 */
claimants: Array<string>, 
/**
 * Why, for the states that have a reason.
 */
detail: string | null, 
/**
 * Whether the restored files were also **loaded** into this session, and how that is known.
 *
 * **Restoring a file and being able to use it are different things**, and the interface has to
 * say which happened: a component put back on disk but not adopted is one the reader will not
 * see until they restart, and reporting that as success would be the fix reading as a
 * different bug.
 */
loading: LoadingOutcome, };

export type RecoveryReport = { 
/**
 * Dictionaries with an unfinished publication.
 */
dictionaries: Array<DictionaryRecovery>, 
/**
 * Components whose files were moved aside and not replaced.
 */
installs: Array<InstallRecovery>, };

export type WorkPriority = "foreground" | "speculative";

export type RequestIdentity = { 
/**
 * One webview lifetime. A second window may use the same operation names safely.
 */
owner: string, 
/**
 * A stable operation name such as `study` or `capture`.
 */
operation: string, 
/**
 * Monotonic within one owner and operation.
 */
generation: number, priority: WorkPriority, };

export type EngineRole = "recognition" | "analysis" | "translation";

export type Hosting = { "kind": "in_process" } | { "kind": "managed_local" } | { "kind": "endpoint", external: boolean, };

export type EngineState = "not_installed" | "not_loaded" | "loading" | "ready";

export type Acceleration = { "kind": "cpu" } | { "kind": "gpu_layers", offloaded: number, total: number, 
/**
 * The device llama.cpp named, already reduced to a safe label.
 */
device: string | null, } | { "kind": "unknown" } | { "kind": "not_running" };

export type EngineStatus = { role: EngineRole, 
/**
 * What the settings ask for, e.g. `"auto"` or `"automatic"`.
 */
configured: string, 
/**
 * What is loaded, when something is.
 */
loaded: string | null, hosting: Hosting, state: EngineState, acceleration: Acceleration, };

export type CacheUse = "fresh" | "cached" | "mixed";

export type FailureCode = "empty_input" | "malformed_input" | "engine_unavailable" | "engine_failed" | "cancelled";

export type Outcome = { "kind": "succeeded" } | { "kind": "failed", code: FailureCode, };

export type OperationResult = { role: EngineRole, 
/**
 * The request this answered. The interface shows a result only for the request it owns.
 */
request: RequestIdentity | null, 
/**
 * The engine that produced it — for Auto recognition, the model Auto picked.
 */
engine: string, 
/**
 * For analysis: why the preferred analyzer did not read this passage.
 */
fallback: FallbackReason | null, cache: CacheUse, outcome: Outcome, 
/**
 * What the engine reported about where it computed, at the time of this request.
 */
acceleration: Acceleration, };

export type RuntimeStatus = { engines: Array<EngineStatus>, recognition: OperationResult | null, analysis: OperationResult | null, translation: OperationResult | null, };
