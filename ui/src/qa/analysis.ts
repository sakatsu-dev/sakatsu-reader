import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type {
  AnalyzerPolicy,
  Binding,
  ContextualWordView,
  CreditEntry,
  DictionaryMeta,
  FallbackReason,
  FallbackScope,
  Found,
  PassageAlign,
  SentenceTranslation,
  Settings,
  StudyView,
  Term,
  Token,
  VariantTranslations,
  WordView,
} from "../model";
// Written by core (`contraction_repair.rs`, `the_grammar_view_explains_each_contraction_from_its_base_form`)
// from the `StudyView` it serves, and compared there byte for byte — so what the layout test draws
// is what core produces, not a hand-built imitation of it.
import contractionBreakdowns from "./contraction-breakdowns.json";

const dictionary: DictionaryMeta = {
  title: "Jitendex.org",
  revision: "2026-07-09",
  format: 3,
  author: null,
  url: "https://jitendex.org/",
  description: null,
  attribution: "JMdict/EDICT",
  entry_count: 433885,
};

/** `?align=` as a `PassageAlign`, or the shipped default for anything else. */
function alignFrom(value: string | null): PassageAlign {
  return value === "center" || value === "right" ? value : "left";
}

function settings(params: URLSearchParams): Settings {
  const light = params.get("theme") === "light";
  return {
    furigana: true,
    mark_seen_on_focus: false,
    // Legacy settings compatibility probe; list activation no longer depends on this field.
    focus_before_open: params.get("cards") !== "hover",
    // `?keyboard=tab` is the setting a keyboard reader turns on: focus stays where it lands.
    tab_navigation: params.get("keyboard") === "tab",
    split_ratio: 0.36,
    capture_bind: { kind: "mouse", button: "middle" },
    overlay_bind: { kind: "key", chord: "CmdOrCtrl+Shift+E" },
    frame_rect: null,
    frame_locked: false,
    overlay_enabled: true,
    switch_bind: null,
    click_delay_ms: 250,
    frequency_source: "",
    frequency_direction: "rank",
    show_frequency_source: false,
    // `?section=` names the section Settings opens on — including one this build does not have,
    // which is what a settings file written by a newer version hands an older one.
    settings_section: params.get("section") ?? "reading",
    hold_ms: 500,
    lock_tap_ms: 400,
    hide_during_capture: false,
    font_size: Number(params.get("intro-size")) || 15,
    reader_scale: 1,
    panel_scale: 1,
    read_clipboard: false,
    quiet: true,
    ichiran_server: false,
    preload_ichiran_on_startup: params.get("preload") !== "off",
    preload_ocr_on_startup: params.get("ocrpreload") !== "off",
    check_updates_on_startup: true,
    // `long-paths` turns both records on, so their paths and the corpus warning are on screen.
    log: params.get("fixture") === "long-paths",
    collect_sentences: params.get("fixture") === "long-paths",
    analyzer: "builtin",
    ocr: "auto",
    vlm_endpoint: "http://127.0.0.1:11434",
    vlm_model: "",
    preprocess: { tighten: false, contrast: false, upscale: false },
    anki_deck: params.get("fixture") === "anki" ? "Sakatsu" : "",
    anki_our_decks_only: true,
    theme: {
      mode: light ? "light" : "dark",
      surface_hue: 250,
      accent_hue: 267,
      kind_spin: Number(params.get("spin") ?? 0),
      tint: 1,
      contrast: 1,
      depth: 0,
      overrides: {},
    },
    theme_presets: [],
    // Off by default so screenshots are not taken mid-crossfade; `?motion=on` is how the
    // specs that care about the hint's cycle ask for it.
    animations: params.get("motion") === "on",
    // Paired with `animations` above: on its own, `animations: true` with a zero duration is
    // still no animation, and a spec asking for motion would silently get none. 180 is the
    // store's own default.
    animation_ms: params.get("motion") === "on" ? Number(params.get("intro-speed") ?? 180) : 0,
    panels_ratio: 0.5,
    decor_cloud: 0,
    decor_cloud_on: true,
    decor_cloud_tint: "ink",
    decor_sakura: 0,
    decor_sakura_on: true,
    decor_sakura_tint: "ink",
    decor_scale: 1,
    decor_cloud_pad: 1,
    decor_cloud_spread: 1,
    decor_sakura_spread: 1,
    decor_blur: false,
    decor_cloud_blur: 3,
    decor_sakura_blur: 1,

    // The reader flows a capture as one paragraph by default; the layout specs that care
    // about the passage shape opt back in with ?breaks=keep.
    keep_line_breaks: params.get("breaks") === "keep",
    // The shipped default unless a spec asks: `?align=center` or `?align=right`.
    passage_align: alignFrom(params.get("align")),
    mark_reading_guesses: params.get("guesses") !== "off",
    // On here and off in the store: the parts row is what the word-parts specs are about, so
    // every one of them would otherwise open with `?parts=on`. `?parts=off` gets the shipped
    // default, which is what the spec for hiding it asks for.
    show_word_parts: params.get("parts") !== "off",
    copy_capture_to_clipboard: false,
    glossary_sort: "grouped",
    reference_layout: "cards",
    reference_tabs: [],
    reference_tab: 0,
    source_on_analysis: false,
    source_on_reference: false,
    frame_tint: 0,
    frame_clickthrough: false,
    always_on_top: false,
    hide_setup: true,
  };
}

function byteOffset(text: string, characterOffset: number) {
  return new TextEncoder().encode(text.slice(0, characterOffset)).length;
}

function token(text: string, surface: string, characterStart: number): Token {
  const start = byteOffset(text, characterStart);
  return {
    surface,
    dict_form: surface === "伝わってきます" ? "伝わる" : surface,
    reading: surface === "気持ち" ? "きもち" : surface === "伝わってきます" ? "つたわってきます" : surface,
    lemma_reading: surface === "気持ち" ? "きもち" : surface === "伝わってきます" ? "つたわる" : surface,
    senses: surface === "。" ? [] : [{ pos: surface === "が" ? "prt" : "n", gloss: "fixture sense", sequence: null }],
    conjugation: null,
    components: [],
    // Every fixture token is grounded in the text it came from, so every one is selectable — the
    // `grouped=expanded` fixture below is the one that is not.
    selectable: true,
    constructions: [],
    lexical_role: null,
    alternatives: [],
    // Fixtures are one line; one that needs a break sets this to "break".
    line_break: "none",
    start,
    end: start + new TextEncoder().encode(surface).length,
    confidence: null,
    romaji: null,
    reading_unverified: false,
    // **Set so the layout suite can see the marker at all.** Not a claim about 気持ち, which reads
    // きもち and nothing else — whether the flag is *true* of a word is core's question, measured
    // against the dictionary in `core/tests/heteronym_readings.rs`. What this fixture is for is
    // whether the reader draws it, and a fixture that never sets it cannot answer that. The
    // neighbouring `reading_unverified` is set nowhere, which is part of why its rule shipped dead.
    reading_ambiguous: surface === "気持ち",
  };
}

/**
 * **How the fixture passage was produced**, chosen by `?fallback=` so each notice can be shown:
 *
 * - absent: Ichiran answered under Automatic — no notice at all;
 * - `passage`: Ichiran failed on this passage and the built-in analyzer retried it;
 * - `starting`: Ichiran is still being checked at startup;
 * - `session-auto`: no Ichiran this session, under Automatic — the quiet label only;
 * - `session-ichiran`: no Ichiran this session, and it was asked for by name.
 */
function executionFor(params: URLSearchParams): StudyView["analysis"]["execution"] {
  const lindera = (reason: FallbackReason, scope: FallbackScope, detail: string, requested: AnalyzerPolicy = "automatic") => ({
    requested,
    engine: "lindera",
    fallback: { reason, scope, detail },
    repairs: [],
  });
  switch (params.get("fallback")) {
    case "passage":
      return lindera("timed_out", "passage", "ichiran: the sidecar did not answer within the budget");
    case "starting":
      return lindera("starting", "session", "Ichiran is still being checked; the built-in analyzer read this passage");
    case "session-auto":
      return lindera("unavailable", "session", "analyzer unavailable: could not run wsl");
    case "session-ichiran":
      return lindera("unavailable", "session", "analyzer unavailable: could not run wsl", "ichiran");
    default:
      return { requested: "automatic", engine: "ichiran", fallback: null, repairs: [] };
  }
}

/**
 * **A rejoined word.** `repaired=1` records the first word as one core rejoined after the engine
 * split it (`lexical::repair`), so the panel's attribution line has a record to match. Shape data:
 * the fixture's word is not a contraction, and the line's wording is `repair-note.test.mjs`'s.
 */
function withRepair(
  execution: StudyView["analysis"]["execution"],
  tokens: StudyView["analysis"]["tokens"],
  params: URLSearchParams,
): StudyView["analysis"]["execution"] {
  const whole = tokens[0];
  if (params.get("repaired") !== "1" || !execution || !whole) return execution;
  return {
    ...execution,
    repairs: [
      {
        start: whole.start ?? 0,
        end: whole.end ?? 0,
        family: "te_oru",
        nasal: false,
        lemma: whole.dict_form,
        lemma_reading: whole.lemma_reading,
        reading: whole.reading,
        sequence: null,
        stem: "headword",
        replaced: 2,
      },
    ],
  };
}

function study(text: string, params: URLSearchParams): StudyView {
  const canonical = text.trim();
  const seed = ["気持ち", "が", "伝わってきます", "。"];
  // `long-passage` repeats the seed so the reader has more than a screenful, which is the only
  // way to exercise a passage that does not fit. The walk below uses `indexOf(part, cursor)`, so
  // it finds each repetition in turn and the byte offsets stay honest — the caller just has to
  // submit text that really does repeat.
  const repeats = params.get("fixture") === "long-passage" ? 12 : 1;
  const body = Array.from({ length: repeats }, () => seed).flat();
  const chunks = seed.every((part) => canonical.includes(part))
    ? [...(canonical.startsWith("前の文です。") ? ["前の文です。"] : []), ...body]
    : [canonical];
  let cursor = 0;
  const tokens = chunks.map((part) => {
    const at = canonical.indexOf(part, cursor);
    cursor = at + part.length;
    return token(canonical, part, at);
  });
  // **A grouped word, with its parts.** What `lexical` assembles: one token whose dictionary form
  // is its own and whose constituents are kept so the reader can still reach them. Nested when
  // asked, because a component can have components and the panel walks a path to get there.
  //
  // A fixture because production reaches it only with the dictionaries installed, and the
  // question here is what the interface does with such a token — not whether core produces one,
  // which `core/tests/lexical_assembly.rs` answers.
  const grouping = params.get("grouped");
  if (grouping && tokens.length > 0) {
    const whole = tokens[0];
    const half = Math.max(1, Math.floor(whole.surface.length / 2));
    const left = { ...token(canonical, whole.surface.slice(0, half), 0), components: [] };
    const right = {
      ...token(canonical, whole.surface.slice(half), half),
      components: [],
    };
    // **An expanded grammatical part**: the second half is represented the way Ichiran names a
    // contracted auxiliary — a form the text does not spell, with no span, not selectable. The
    // reader must offer only the first half, and keep the second half's text as context.
    const expanded = {
      ...token(canonical, "いる", 0),
      start: null,
      end: null,
      selectable: false,
    };
    const inner =
      grouping === "expanded"
        ? [expanded]
        : grouping === "nested"
        ? [
            {
              ...right,
              components: [
                { ...token(canonical, right.surface.slice(0, 1), half), components: [] },
                { ...token(canonical, right.surface.slice(1), half + 1), components: [] },
              ],
            },
          ]
        : [right];
    // Its own identity, not a constituent's — the property that makes a unit's status and
    // lookup its own. Left as the token's own surface rather than a borrowed headword, so the
    // heading is predictable and the fixture claims nothing about a word it is not.
    tokens[0] = { ...whole, components: [left, ...inner] };
  }
  const sentences: StudyView["sentences"] = [];
  let firstToken = 0;
  tokens.forEach((part, index) => {
    // The `\n` this used to test for never matched: no analyzer emits a token carrying a
    // line break, which is why the real rule reads `line_break` instead. Both are checked here
    // so a fixture can represent either kind of ending.
    if (part.line_break === "break" || /[。！？]/u.test(part.surface)) {
      sentences.push({ first_token: firstToken, last_token_end: index + 1 });
      firstToken = index + 1;
    }
  });
  if (firstToken < tokens.length) {
    sentences.push({ first_token: firstToken, last_token_end: tokens.length });
  }
  return {
    analysis: { text: canonical, tokens, execution: withRepair(executionFor(params), tokens, params) },
    sentences,
    breakdowns: tokens.map((part) =>
      params.get("fixture") === "contraction" && part.surface in contractionBreakdowns
      ? (contractionBreakdowns as unknown as Record<string, StudyView["breakdowns"][number]>)[
          part.surface
        ]
      : part.surface === "伝わってきます"
      ? {
          steps: [
            {
              form: "conjunctive", label: "Te-form", pos: "verb", neg: false, fml: false,
              lemma: "伝わる", base_reading: "つたわる", via_depth: 1, is_base: true,
              formation: { from: "つたわる", change: "る → って", result: "つたわって" },
              component: 0,
            },
            {
              form: "non-past", label: "Non-past", pos: "verb", neg: false, fml: true,
              lemma: "くる", base_reading: "くる", via_depth: 1, is_base: false,
              formation: { from: "くる", change: "irregular", result: "きます" },
              component: 1,
            },
          ],
          auxiliaries: [{ component: 1, pattern_id: "te-kuru" }],
        }
      : { steps: [], auxiliaries: [] }),
    patterns: params.get("expression")
      ? // A `noni` match, so a selection can be covered by one and the rest of the sentence can be
        // selected to show it is not. `whole` narrows it to the single first token, which is the
        // shape where the selection *is* the expression; the byte spans follow the tokens
        // (気持ち is 9 bytes, が 3) so the fixture does not contradict itself.
        params.get("expression") === "whole"
          ? // `elements: []` is the honest value for a hand-built match: no rule ran, so there are
            // no rule positions. The whole-span offer this fixture exercises does not use them,
            // and an occurrence offer correctly finds none.
            [
              {
                pattern_id: "noni",
                first_token: 0,
                last_token_end: 1,
                start: 0,
                end: 9,
                elements: [],
              },
            ]
          : [
              {
                pattern_id: "noni",
                first_token: 0,
                last_token_end: 2,
                start: 0,
                end: 12,
                elements: [],
              },
            ]
      : params.get("fixture") === "long" && tokens.length > 2
        ? ["continuing-state", "speaker-direction", "formal-register"].map((pattern_id) => ({
            pattern_id,
            first_token: 2,
            last_token_end: 3,
            start: null,
            end: null,
            elements: [],
          }))
        : [],
    statuses: tokens.map(() => "new"),
    ruby: tokens.map((part) => [{ text: part.surface, reading: part.reading }]),
  };
}

function wordFor(part: Token, params: URLSearchParams): ContextualWordView {
  const long = params.get("fixture") === "long";
  const ambiguous = params.get("fixture") === "ambiguous";
  const component = params.get("component");
  const lexical = params.get("fixture") === "lexical-role";
  const glosses = long
    ? [
        "to communicate or convey a feeling, impression, intention, or piece of information clearly across a considerable distance",
      ]
    : [part.surface === "気持ち" ? "feeling; state of mind; sensation" : "to be conveyed; to come across"];
  const inline = component === "神" && part.surface === "神姫"
    ? [{ tags: ["n"], glosses: ["god; deity; divinity"], sequence: 1364440 }]
    : lexical && part.surface === "たち"
      ? [
          { tags: ["suf"], glosses: ["pluralizing suffix"], sequence: 1416220 },
          { tags: ["n"], glosses: ["long sword"], sequence: 1408340 },
        ]
      : lexical && part.surface === "なん"
        ? [
            { tags: ["pn"], glosses: ["what; why"], sequence: 2846738 },
            { tags: ["n"], glosses: ["difficulty; trouble"], sequence: 1460830 },
          ]
        : [
            { tags: [part.surface === "が" ? "prt" : "n"], glosses, sequence: null },
            ...(long
              ? Array.from({ length: 5 }, (_, index) => ({
                  tags: ["n"], sequence: null,
                  glosses: [`additional deterministic meaning ${index + 2} retained behind the disclosure`],
                }))
              : []),
            ...(ambiguous ? [{ tags: ["n"], glosses: ["another meaning supported equally by this context"], sequence: null }] : []),
          ];
  const word: WordView = {
    headword: part.dict_form,
    reading: part.lemma_reading,
    inline,
    inline_source: null,
    inline_retrieved: component
      ? { kind: "component", query: component }
      : { kind: "whole", query: part.dict_form },
    also_in: [],
    examples: Array.from({ length: long ? 7 : 2 }, (_, index) => ({
      japanese: `これは読みやすさを確認する例文です${index + 1}。`,
      english: `This is deterministic example ${index + 1} for checking the reading layout.`,
      retrieved: component
        ? { kind: "component", query: component }
        : { kind: "whole", query: part.dict_form },
    })),
    name: component === "神" && part.surface === "神姫"
      ? { glosses: ["shinki"], tags: ["fem"], source: "JMnedict", leads: false,
          retrieved: { kind: "whole", query: "神姫" } }
      : null,
    likely_name: false,
    romaji: null,
  };
  return {
    word,
    ranking: {
      senses: word.inline.map((_, index) => ({
        original_index: index,
        score: ambiguous || component ? 0 : index === 0 ? 2 : 0,
        reasons: index === 0 && !component ? ["The nearby structure supports this reading."] : [],
      })),
      favoured: ambiguous || component ? null : 0,
    },
  };
}

function comparison(text: string): VariantTranslations {
  const focused = "気持ちが伝わってきます。";
  return {
    japanese: { full: text.includes(focused) ? focused : text, runs: ["気持ち"], without: "が伝わってきます。" },
    full: "My feelings are coming through.",
    without: "They are coming through.",
    pieces: [
      { text: "My feelings", gone: true },
      { text: "are coming through.", gone: false },
    ],
    result_says_more: true,
  };
}

/// One result per part-of-speech heading, for the filter that hides them.
///
/// Four kinds and not five, because four is what the assertions need: a noun, a verb, an
/// adjective, and one entry with no part-of-speech tag at all. That last is not padding —
/// `kindOf` files 32% of the real dictionary under `Other` for exactly this reason, so a fixture
/// without one would let the filter be built and tested as though `Other` were an edge case.
///
/// The words are real and their tags are the ones the dictionary gives them. `相槌` is the
/// exception and says so: it is an ordinary noun, carried here with its tags emptied to stand for
/// the untagged third of the dictionary.
function partsOfSpeechResults(base: WordView): Found {
  const plain = { forms: [], chars: [], ranking: null, surface: null };
  return {
    ranked_by: null,
    japanese: false,
    needs_gloss_index: false,
    // The English path never truncates in this fixture, so a card list is not also a claim that
    // something was held back. `capped` has its own fixture below.
    capped: false,
    words: [
      { ...base, ...plain, headword: "図書館", reading: "としょかん",
        inline: [{ tags: ["n"], glosses: ["library"], sequence: null }] },
      { ...base, ...plain, headword: "食べる", reading: "たべる",
        inline: [{ tags: ["v1", "vt"], glosses: ["to eat"], sequence: null }] },
      { ...base, ...plain, headword: "早い", reading: "はやい",
        inline: [{ tags: ["adj-i"], glosses: ["fast; early"], sequence: null }] },
      { ...base, ...plain, headword: "相槌", reading: "あいづち",
        inline: [{ tags: [], glosses: ["aizuchi; a listener's interjection"], sequence: null }] },
      // **In two kinds at once, which nothing else here is.** Every other word above belongs
      // to exactly one category, so the suite could not tell a badge that names the matched
      // kind from one that names the first kind in `WORD_KINDS` — and shipped a list whose
      // cards read NOUNS while the learner was filtering by Adverbs. Real word, real tags.
      { ...base, ...plain, headword: "道中", reading: "どうちゅう",
        inline: [
          { tags: ["n"], glosses: ["journey; trip; travels"], sequence: null },
          { tags: ["adv"], glosses: ["during the journey; on the way; while travelling"], sequence: null },
        ] },
    ],
  };
}

/// A conjugated query, which is the case the "as written" chip exists for: 走った was typed and
/// 走る is the answer. The second word is deliberately *not* conjugated — without it the suite
/// could not tell a chip that appears when the analyzer did something from one on every card.
function inflectedResult(base: WordView): Found {
  const plain = { forms: [], chars: [], ranking: null };
  return {
    ranked_by: null,
    japanese: true,
    needs_gloss_index: false,
    capped: false,
    words: [
      { ...base, ...plain, headword: "走る", reading: "はしる", surface: "走った",
        inline: [{ tags: ["v5r", "vi"], glosses: ["to run"], sequence: null }] },
      { ...base, ...plain, headword: "道", reading: "みち", surface: "道",
        inline: [{ tags: ["n"], glosses: ["road; path; street"], sequence: null }] },
    ],
  };
}

/// An English answer that reached its ceiling. Two words is enough: the notice is driven by the
/// flag, not by counting what came back, which is the whole reason it can be honest.
function cappedResult(base: WordView): Found {
  const plain = { forms: [], chars: [], ranking: null, surface: null };
  return {
    ranked_by: null,
    japanese: false,
    needs_gloss_index: false,
    capped: true,
    words: [
      { ...base, ...plain, headword: "走る", reading: "はしる",
        inline: [{ tags: ["v5r", "vi"], glosses: ["to run"], sequence: null }] },
      { ...base, ...plain, headword: "営む", reading: "いとなむ",
        inline: [{ tags: ["v5m", "vt"], glosses: ["to run (a business)"], sequence: null }] },
    ],
  };
}

/// A word whose senses are function words, which is the case that had no colour at all.
///
/// が is the one the reader hit: a particle sense and a conjunction sense, neither of which any
/// kind recognised, so a first-class result type on this page was the one word drawn plain. The
/// third sense is a noun so the fixture also proves the two are told apart rather than all being
/// swept into one colour.
function functionWordResult(base: WordView): Found {
  const plain = { forms: [], chars: [], ranking: null, surface: null };
  return {
    ranked_by: null,
    japanese: true,
    needs_gloss_index: false,
    capped: false,
    words: [
      { ...base, ...plain, headword: "が", reading: "が",
        inline: [
          { tags: ["particle"], glosses: ["indicates the subject of a sentence"], sequence: null },
          { tags: ["conjunction"], glosses: ["but; however; (and) yet"], sequence: null },
          { tags: ["n"], glosses: ["a made-up noun sense, so the colours can be compared"], sequence: null },
        ] },
    ],
  };
}

function referenceResult(params: URLSearchParams): Found {
  const base = wordFor(token("図書館", "図書館", 0), params).word;
  if (params.get("fixture") === "personal-name") return {
    ranked_by: null, japanese: true, needs_gloss_index: false, capped: false,
    words: [{ ...base, headword: "山田太郎", reading: "やまだたろう", forms: [], chars: [], ranking: null, surface: null,
      inline: [{ tags: ["person"], glosses: ["Yamada Tarō"], sequence: null }] }],
  } as unknown as Found;
  // The other entry point to the rebuild. Reference offers it when English search has no index;
  // the panel in Settings offers the same operation, and both must leave the interface refreshed.
  if (params.get("fixture") === "needs-index") {
    return { ranked_by: null, japanese: false, needs_gloss_index: true, capped: false, words: [], chars: [], forms: [], ranking: null, surface: null } as unknown as Found;
  }
  if (params.get("fixture") === "parts") return partsOfSpeechResults(base);
  if (params.get("fixture") === "inflected") return inflectedResult(base);
  if (params.get("fixture") === "capped") return cappedResult(base);
  if (params.get("fixture") === "function") return functionWordResult(base);
  // A word with an older character form. Real data, read out of the installed Jitendex: 散歩
  // (さんぽ, seq 1303620, score 200) lists 散步 — 步 U+6B65, the old form of 歩 U+6B69 — at -101.
  if (params.get("fixture") === "forms") {
    return {
      ranked_by: null, japanese: true, needs_gloss_index: false, capped: false,
      words: [{ ...base, chars: [], ranking: null, surface: null, headword: "散歩", reading: "さんぽ",
        forms: [{ text: "散步", rare: true }],
        inline: [{ tags: ["n", "vs", "vi"], glosses: ["walk; stroll"], sequence: null }] }],
    } as unknown as Found;
  }
  return {
    ranked_by: null,
    japanese: true,
    needs_gloss_index: false,
    capped: false,
    words: [{
      ...base,
      headword: "図書館",
      reading: "としょかん",
      inline: [{ tags: ["n"], glosses: ["library"], sequence: null }],
      forms: [],
      ranking: null,
      // The typed form and the answer are the same word, so there is nothing to report.
      surface: "図書館",
      chars: [
        {
          character: "図",
          on: [{ text: "ズ", here: true }, { text: "ト", here: false }],
          kun: [{ text: "え", here: false }],
          nanori: ["のり"],
          meanings: ["map", "drawing", "plan"],
          strokes: 7,
          grade: 2,
          components: ["囗", "斗"],
          irregular: false,
        },
        {
          character: "書",
          on: [{ text: "ショ", here: true }],
          // `-がき` is 書's real KANJIDIC2 suffix reading — the 書き of 走り書き and 落書き, never a
          // word by itself. Harvested rather than invented, because it is the one shape that
          // exercises how a partial reading is written; the rest of this entry is genuine too.
          kun: [{ text: "か.く", here: false }, { text: "-がき", here: false }],
          nanori: [],
          meanings: ["write", "book"],
          strokes: 10,
          grade: 2,
          components: ["聿", "日"],
          irregular: false,
        },
        {
          character: "館",
          on: [{ text: "カン", here: true }],
          kun: [{ text: "やかた", here: false }],
          nanori: [],
          meanings: ["building", "mansion"],
          strokes: 16,
          grade: 3,
          components: ["食", "官"],
          irregular: false,
        },
      ],
    }],
  };
}

function glossaryTerms(): Term[] {
  return [
    {
      id: "conjunctive",
      label: "Conjunctive form",
      aliases: ["te-form", "連用形"],
      summary: "A form that connects a word to the construction that follows it.",
      detail: "It lets an inflected word carry into an auxiliary or another clause without pretending the pieces are unrelated.",
      examples: [{ japanese: "食べてみる。", english: "Try eating it." }],
      requires: [],
      contrasts: [],
      related: ["aux"],
      kind: "form",
    },
    {
      id: "aux",
      label: "Auxiliary",
      aliases: ["helper verb"],
      summary: "A supporting word that adds tense, voice, direction, or another grammatical meaning.",
      detail: null,
      examples: [],
      requires: ["conjunctive"],
      contrasts: [],
      related: ["conjunctive"],
      kind: "concept",
    },
    {
      id: "vt",
      label: "Transitive verb",
      aliases: ["他動詞", "verb taking an object"],
      summary: "A verb whose action is directed at an object.",
      detail: null,
      examples: [{ japanese: "ドアを開ける。", english: "Open the door." }],
      requires: [],
      contrasts: ["vi"],
      related: [],
      kind: "part-of-speech",
    },
    {
      id: "vi",
      label: "Intransitive verb",
      aliases: ["自動詞"],
      summary: "A verb describing an event without directing the action at an object.",
      detail: null,
      examples: [{ japanese: "ドアが開く。", english: "The door opens." }],
      requires: [],
      contrasts: ["vt"],
      related: [],
      kind: "part-of-speech",
    },
  ];
}

const qaPattern = {
  id: "te-kuru",
  pattern: "〜てくる",
  title: "Te-form plus 来る",
  jlpt: "N4",
  formation: "te-form + くる",
  explanation: "The action comes toward the speaker, or has been building up until now.",
  detail: "The direction may be physical, temporal, or conceptual depending on the sentence.",
  term_ids: ["conjunctive", "aux"],
  examples: [{ japanese: "分かってきた。", english: "I am starting to understand." }],
  reference: null,
  match: null,
};

function alignedTranslations(tokens: Token[], params: URLSearchParams): SentenceTranslation[] {
  const ranges: Array<{ first_token: number; last_token_end: number }> = [];
  let firstToken = 0;
  tokens.forEach((part, index) => {
    // The `\n` this used to test for never matched: no analyzer emits a token carrying a
    // line break, which is why the real rule reads `line_break` instead. Both are checked here
    // so a fixture can represent either kind of ending.
    if (part.line_break === "break" || /[。！？]/u.test(part.surface)) {
      ranges.push({ first_token: firstToken, last_token_end: index + 1 });
      firstToken = index + 1;
    }
  });
  if (firstToken < tokens.length) ranges.push({ first_token: firstToken, last_token_end: tokens.length });

  return ranges.map((range) => {
    const japanese = tokens
      .slice(range.first_token, range.last_token_end)
      .map((part) => part.surface)
      .join("");
    let english = japanese.includes("前の文")
      ? "This is the preceding sentence."
      : "My feelings are coming through.";
    if (params.get("fixture") === "long-translation") {
      english = "This intentionally long translation checks that the card clamps at exactly three readable lines until the learner asks to see the full passage. It must preserve the selected-word answer, keep both detail panes within their own scrollports, and avoid horizontal overflow at every supported window size. The same paragraph continues long enough to be genuinely clipped even at the widest test viewport, because the disclosure must be based on measured geometry rather than an assumption about character count.";
    }
    return { ...range, japanese, english };
  });
}

/**
 * What a session with no usable store says, and what every save in it then fails with.
 *
 * Taken from the real one: `open_store` writes "Nothing can be saved this session: {error}", and
 * the os error is what Windows returns when a *file* occupies the store's directory path — the
 * condition `scripts/desktop-checkpoint.ps1 -Store Unavailable` creates, and the only way
 * `StoreState.store` is `None`. Renaming a store directory produces an empty store instead.
 */
/// A write that was attempted and went wrong, with a store that opened perfectly well.
///
/// Deliberately not the sentence above: the point of the fixture is that this one has no standing
/// warning behind it, so the interface has nowhere else to say it.
const WRITE_FAILED = "Access is denied. (os error 5)";

/// Which containers this session has already resolved, so a refresh after a successful action
/// shows what a real one would: the entry gone.
const resolvedRecoveries = new Set<string>();

/// The recovery report, shaped by `?recovery=`.
///
/// Each value is one of the situations the panel must draw differently, rather than a generic
/// "something is unfinished" — the whole reason the report carries states at all.
function recoveryReport(params: URLSearchParams) {
  const how = params.get("recovery");
  if (!how) return { dictionaries: [], installs: [] };

  const unresolved = {
    name: "jitendex",
    path: "C:/scratch/dictionaries/jitendex",
    state: "unresolved" as const,
    transaction: "t-1",
    published: "g-000000000002",
    previous: "g-000000000001",
    live: null,
    detail: null,
    already_tried: false,
  };
  const dictionaries = [];
  if (how === "stale") {
    // Two, so the stale one going away leaves something behind it. The panel keeps its heading and
    // its list, and the explanation has to survive beside them.
    dictionaries.push(unresolved);
    dictionaries.push({
      ...unresolved,
      name: "jmnedict",
      path: "C:/scratch/dictionaries/jmnedict",
      transaction: "t-2",
    });
  } else if (how === "stale-last") {
    // One, so resolving it empties the panel. **This is the case the old fixture could not
    // produce**, and the case a person actually met: the entry goes, and the explanation has
    // nothing left to be attached to.
    dictionaries.push(unresolved);
  } else if (how === "conflicting") {
    dictionaries.push({ ...unresolved, state: "conflicting" as const, live: "g-000000000003" });
  } else if (how === "unreadable") {
    dictionaries.push({
      ...unresolved,
      state: "unreadable" as const,
      transaction: null,
      published: null,
      previous: null,
      detail: "the recovery record is not valid JSON",
    });
  } else {
    dictionaries.push(unresolved);
  }

  const installs =
    how === "install-failed"
      ? [
          {
            id: "kanji",
            state: "failed" as const,
            destination: "C:/scratch/kanji",
            saved: "C:/scratch/.offline-setup-1-1/backup/kanji",
            claimants: [],
            detail: "the destination could not be created",
            loading: "unknown" as const,
          },
        ]
      : how === "install-restored"
        ? [
            {
              id: "kanji",
              state: "restored" as const,
              destination: "C:/scratch/kanji",
              saved: null,
              claimants: [],
              detail: null,
              loading: "loaded" as const,
            },
          ]
        : how === "install-load-failed"
          ? [
              {
                id: "kanji",
                state: "restored" as const,
                destination: "C:/scratch/kanji",
                saved: null,
                claimants: [],
                detail: "the files were put back, but reloading them produced no characters",
                loading: "failed" as const,
              },
            ]
          : [];

  return {
    dictionaries: dictionaries.filter((entry) => !resolvedRecoveries.has(entry.name)),
    installs,
  };
}

/**
 * A path with nowhere to break, for `?fixture=long-paths`: an account name and a folder longer
 * than any pane is wide. The settings page prints paths as they are, so a long one has to wrap
 * inside its card rather than push the card, or the page, sideways.
 */
/**
 * What `runtime_status` answers: Auto loaded in the app on the processor, the built-in analyzer
 * standing in for Ichiran, and FuguMT not loaded — one of each state the Diagnostics rows draw.
 * `?fixture=long-paths` gives the recognition engine a GPU with an unbroken device label, the
 * longest thing a row can be asked to hold.
 */
function runtimeStatusFor(params: URLSearchParams, unloaded: Set<string>) {
  const long = params.get("fixture") === "long-paths";
  const recognitionLoaded = !unloaded.has("recognition");
  const translationLoaded = params.get("translator") === "loaded" && !unloaded.has("translation");
  return {
    engines: [
      {
        role: "recognition",
        configured: long ? "vlm" : "auto",
        loaded: recognitionLoaded ? (long ? "vlm" : "auto") : null,
        hosting: { kind: long ? "managed_local" : "in_process" },
        state: recognitionLoaded ? "ready" : "not_loaded",
        acceleration: !recognitionLoaded
          ? { kind: "not_running" }
          : long
          ? {
              kind: "gpu_layers",
              offloaded: 29,
              total: 29,
              device: "Vulkan0 (AGraphicsAdapterWhoseNameRunsOnWithoutASingleSpace0123456789)",
            }
          : { kind: "cpu" },
      },
      {
        role: "analysis",
        configured: "automatic",
        loaded: "lindera",
        hosting: { kind: "in_process" },
        state: "ready",
        acceleration: { kind: "cpu" },
      },
      {
        role: "translation",
        configured: "fugumt",
        loaded: translationLoaded ? "fugumt" : null,
        hosting: { kind: "in_process" },
        state: translationLoaded ? "ready" : "not_loaded",
        acceleration: translationLoaded ? { kind: "cpu" } : { kind: "not_running" },
      },
    ],
    recognition: null,
    analysis: {
      role: "analysis",
      request: null,
      engine: "lindera",
      fallback: "unavailable",
      cache: "fresh",
      outcome: { kind: "succeeded" },
      acceleration: { kind: "cpu" },
    },
    translation: null,
  };
}

/** What `diagnostic_report` answers: the shape the core builder writes, with fixed values. */
const DIAGNOSTIC_REPORT = [
  "Sakatsu Reader diagnostic report",
  "Version: 1.0.0 (development build)",
  "System: windows 10.0.26200, x86_64",
  "Recognition: configured auto, loaded auto; in the app; ready; CPU",
  "Analysis: configured automatic, loaded lindera; in the app; ready; CPU",
  "Translation: configured fugumt; in the app; not loaded; not running",
  "Last recognition: none this session",
  "Last analysis: lindera succeeded, fresh, fallback Unavailable; CPU",
  "Last translation: none this session",
  "Resources: dictionary 1 ready; translation 1 ready",
  "Setup: complete",
  "Logging: off",
  "Log file: sakatsu.log",
].join("\n");

const LONG_PATH =
  "C:\\Users\\AReaderWithAnUnusuallyLongAccountNameThatNeverBreaks\\AppData\\Local\\com.sakatsu.app\\" +
  "ResourcesMovedToAFolderWhoseNameRunsOnWithoutASingleSpaceOrHyphenToBreakAt0123456789\\records";

/** A deck name as long as the ones real collections have, and none of it Sakatsu's. */
const LONG_DECK =
  "Japanese::Core 6000 Optimised Vocabulary with Sentences and Native Audio::Unit 01";

const STORE_UNAVAILABLE =
  "Nothing can be saved this session: Cannot create a file when that file already exists. (os error 183)";

export async function installAnalysisQa(params: URLSearchParams) {
  // Dictionary index maintenance, driven from the query string so each state is reachable.
  //   upkeep=missing    English search works, the sequence bucket does not exist (the real case)
  //   upkeep=complete   every index present, so nothing is offered
  //   upkeep=none       no dictionary installed
  //   upkeep=unreadable the status request fails
  //   upkeep=duplicate  two dictionaries sharing a title — a list keyed on it collapses them
  //   build=ok|fail|busy|partial  what the rebuild answers; `partial` builds one and fails another
  const upkeepState = params.get("upkeep") ?? "complete";
  const buildOutcome = params.get("build") ?? "ok";
  let upkeepBuilt = false;
  let upkeepReadable = true;
  const managerState = params.get("manager") ?? "ready";
  const savedDictionaryPreferences = JSON.parse(sessionStorage.getItem("qa-dictionary-preferences") ?? "null");
  const manager: import("../bindings").DictionaryManagerSnapshot = {
    preferences: savedDictionaryPreferences ?? { check_updates: true, automatic: false, notify_updates: true },
    dismissed_updates: JSON.parse(sessionStorage.getItem("qa-dismissed-dictionary-updates") ?? "{}"), operation: null,
    dictionaries: managerState === "empty" ? [] : [{ id: "fixture", kind: "words", title: "Example dictionary [2026-10-03]", revision: "2026.10.03.0", generation: "fixture-generation", source: "https://example.org/index.json", home: "https://example.org/", entries: 42, sha256: null, consented: false, phase: "unchecked", detail: "", available_revision: null }],
  };
  if (managerState === "legacy") manager.dictionaries[0].source = null;
  if (managerState === "updates") manager.dictionaries[0].available_revision = params.get("revision") ?? "2026.10.18.0";
  mockWindows("main");
  let activeText = "";
  let studyCalls = 0;
  const statusCalls: Token[] = [];
  // **What each contextual lookup was asked against**: the surfaces in order and the index asked
  // about. A part's context must keep every character of its word and add nothing the text
  // does not spell, and this is the only place a spec can see the sequence the reader sent.
  const contextCalls: { surfaces: string[]; index: number }[] = [];
  let keptSettings = settings(params);
  const startupPending = new Set(params.get("startup") === "loading" ? ["analysis", "recognition"] : []);
  let startupFailed = params.get("startup") === "error";
  let startupUnavailable = params.get("startup") === "unavailable";
  let osTheme: "light" | "dark" = params.get("os") === "light" ? "light" : "dark";
  const setupMode = params.get("setup") ?? (params.get("qa") === "onboarding" ? "fresh" : "complete");
  let setupPhase = setupMode === "fresh" || setupMode === "complete-after-start"
    ? "ready_to_install"
    : setupMode === "downloading"
      ? "downloading"
      : setupMode === "paused"
        ? "paused_retrying"
        : setupMode === "error"
          ? "needs_attention"
          : "complete";
  let setupReady = setupPhase === "complete";
  // `?text-only=1`: setup unfinished, and the reader chose to use the app without capture.
  let setupTextOnly = params.get("text-only") === "1";
  let analyzerSetupPending = setupMode === "analyzer";
  let ichiranReady = params.get("ichiran") === "ready";
  let initialSetupProgressSent = false;
  let setupWelcomeSeen = setupMode !== "fresh" && setupMode !== "complete-after-start";
  const setupComponents = [
    ["dictionaries", "Dictionaries", 50_123_342],
    ["kanji", "Kanji details", 14_125_383],
    ["wordnet", "Particle senses", 11_598_399],
    ["manga-ocr", "manga-ocr", 461_042_537],
    ["paddle-ocr", "PP-OCRv6", 31_114_838],
  ] as const;
  /**
 * What the About page credits.
 *
 * **The WebView2 entry is copied verbatim from `core/src/credits.rs`, and that is the point of
 * this fixture.** Its licence is a 74-character clause where every other one is a tag three
 * characters wide, so it is the only entry that tests whether the card's corner can hold a
 * sentence. It could not: `.licence` was `flex: 0 0 auto`, so the string could neither wrap nor
 * give way, and it crushed the name to one word per line and overflowed onto the card beside it.
 *
 * Shortening it here to make a tidier fixture would retire the only case that catches that.
 */
const credits: CreditEntry[] = [
  {
    name: "TypeScript",
    licence: "Apache-2.0",
    url: "https://www.typescriptlang.org/",
    why: "Types the interface, and is where the shape of everything crossing the boundary from Rust is written down.",
    group: "The frame it runs in",
  },
  {
    name: "Microsoft Edge WebView2 Runtime",
    licence: "Microsoft Edge WebView2 Runtime distribution terms - redistribution permitted",
    url: "https://developer.microsoft.com/microsoft-edge/webview2/",
    why: "The engine this interface is drawn with. It is embedded in the installer rather than fetched during setup, because an installer that needs the network is an installer that fails without one.",
    group: "The frame it runs in",
  },
  {
    name: "Lindera",
    licence: "MIT",
    url: "https://github.com/lindera/lindera",
    why: "Reads Japanese without anything being installed, so a fresh copy is useful immediately.",
    group: "Reading Japanese",
  },
];

const optionalCatalog = [
    {
      id: "ichiran", label: "Ichiran analyzer", about: "Recommended main analyzer with Lindera assistance.",
      licence: "Ichiran — MIT", home: "https://github.com/tshatrov/ichiran", sources: [],
      size: "4.7 GB", bytes: 5_046_796_288, tier: "advanced", needs_reload: false,
      fetchable: false, script: "setup-ichiran.ps1", ready: ichiranReady, update_available: false,
    },
    {
      id: "translation", label: "Translation",
      about: "A rough English gist on request.", licence: "FuguMT — CC BY-SA 4.0",
      home: "https://huggingface.co/staka/fugumt-ja-en", sources: [], size: "310 MB",
      bytes: 325_058_560, tier: "optional", needs_reload: false, fetchable: true,
      script: null, ready: false, update_available: false,
    },
    {
      id: "pronunciation", label: "Offline pronunciation",
      about: "Human Japanese vocabulary recordings stored on this device.",
      licence: "Tofugu and WaniKani — CC BY-SA 4.0",
      home: "https://github.com/tofugu/japanese-vocabulary-pronunciation-audio",
      sources: [], size: "90 MB", bytes: 94_371_840, tier: "optional",
      needs_reload: true, fetchable: true, script: null, ready: false,
      update_available: false,
    },
  ];
  /// Engines unloaded through `unload_ocr` / `unload_translator`.
  const unloaded = new Set<string>();
  /// Whether the `optional-partial` fixture's partial has been discarded.
  let optionalPartialDiscarded = false;
  let optionalBitsPaused = false;
  const setupSnapshot = () => ({
    phase: setupPhase,
    generation: 4,
    required_ready: setupReady,
    analyzer_setup_pending: analyzerSetupPending,
    resumable: ["downloading", "paused_retrying", "needs_attention"].includes(setupPhase),
    text_only: setupTextOnly,
    welcome_seen: setupWelcomeSeen,
    existing_install: setupMode !== "fresh",
    current_component: setupPhase === "downloading" ? "manga-ocr" : null,
    current_issue: setupPhase === "paused_retrying"
      ? "Setup is paused. Your download progress is saved."
      : setupPhase === "needs_attention"
        ? "The connection stopped after eight attempts. The saved download can be retried."
        : null,
    storage: {
      target: params.get("fixture") === "long-paths"
        ? LONG_PATH
        : "C:\\Users\\Reader\\AppData\\Local\\com.sakatsu.app",
      download_bytes: 709_585_432,
      installed_bytes: 709_585_432,
      staging_bytes: 461_042_537,
      peak_bytes: 2_416_678_313,
      available_bytes: 32_212_254_720,
      reserve_bytes: 536_870_912,
    },
    components: [...setupComponents.map(([id, label, bytes], index) => {
      const installed = setupReady || (setupMode !== "fresh" && index < 2);
      return {
        id, label, required: true, installed,
        installed_version: installed ? "fixture-release" : null,
        transfer_state: installed ? "installed" : id === "manga-ocr" && setupPhase === "downloading" ? "transferring" : setupMode === "paused" ? "paused" : "not_started",
        downloaded_bytes: id === "manga-ocr" && !installed && setupMode !== "fresh" ? 188_743_680 : 0,
        total_bytes: bytes,
        verification: installed ? "verified" : "not_checked",
        repair_eligible: installed || setupMode !== "fresh",
        artifacts: [],
      };
    }),
    // `?fixture=optional-partial`: a paused optional download with part of it on disk, until it
    // is discarded.
    ...(params.get("fixture") === "optional-partial" && !optionalPartialDiscarded
      ? [{
          id: "pronunciation", label: "Offline pronunciation", required: false, installed: false,
          installed_version: null, transfer_state: "paused", downloaded_bytes: 41_943_040,
          total_bytes: 94_371_840, verification: "not_checked", repair_eligible: false,
          artifacts: [],
        }]
      : []),
    ...(params.get("fixture") === "optional-bits" && optionalBitsPaused
      ? [{
          id: "pronunciation", label: "Offline pronunciation", required: false, installed: false,
          installed_version: null, transfer_state: "paused", downloaded_bytes: 0,
          total_bytes: 94_371_840, verification: "not_checked", repair_eligible: true,
          artifacts: [{ file: "recordings.zip", version: "qa", immutable_source: "qa", required: false,
            expected_download_bytes: 94_371_840, expected_installed_bytes: 94_371_840, sha256: "",
            downloaded_bytes: 0, partial_file: null, transfer_job: "saved-qa-job", verification: "not_checked" }],
        }]
      : [])],
  });
  const commands: string[] = [];
  let finishTranslation: (() => void) | null = null;
  const waitForTranslation = () => new Promise<void>((resolve) => {
    // A loading-state assertion must control completion: a 350 ms timer can finish before
    // Edge gets CPU time to inspect the state. Manual QA keeps the short automatic delay.
    if (params.has("hold-translation")) finishTranslation = resolve;
    else window.setTimeout(resolve, 350);
  });
  const releaseTranslation = () => {
    if (!finishTranslation) return false;
    const finish = finishTranslation;
    finishTranslation = null;
    finish();
    return true;
  };

  /// Requests held open, keyed by the operation and the thing that asked for them.
  ///
  /// **Keyed, because a single slot cannot express the sequence that matters.** `hold-translation`
  /// above holds one promise and releases it with no argument, which is enough for "is the loading
  /// state drawn" and useless for "B finishes first and A's late answer must not paint". The
  /// ordering tests in `operation-controller.test.mjs` prove the controller gets that right; this
  /// is how the same sequence is driven through the real reader, where the controller is wired to
  /// the surface it guards.
  ///
  /// Keys read `study:<passage>` and `lookup:<surface>`, so a test names what it is releasing
  /// rather than counting promises. Only reachable under `?qa=`, and only for the operations
  /// `hold-study` / `hold-lookup` ask for: ordinary product flows never construct this at all.
  const heldRequests = new Map<string, { resolve: () => void; reject: (why: unknown) => void }>();
  const holding = (operation: string) => params.has(`hold-${operation}`);
  const waitForRequest = (operation: string, key: string) =>
    new Promise<void>((resolve, reject) => {
      if (!holding(operation)) {
        resolve();
        return;
      }
      heldRequests.set(`${operation}:${key}`, { resolve, reject });
    });
  /// Let one held request finish, or fail it. Returns false if nothing was holding that key, so a
  /// test that names the wrong thing fails instead of quietly proving nothing.
  const releaseRequest = (key: string, how: "complete" | "fail" = "complete") => {
    const slot = heldRequests.get(key);
    if (!slot) return false;
    heldRequests.delete(key);
    if (how === "fail") slot.reject(new Error(`qa: refused ${key}`));
    else slot.resolve();
    return true;
  };
  const pendingRequests = () => [...heldRequests.keys()];

  /// **The About page's sources, steered one call at a time.** `dictionaries` is also asked for at
  /// startup, so a URL flag cannot fail About's call without failing the app's first. A spec arms
  /// these after the app is up and before it opens About: the next call to the named command fails,
  /// waits until released, or answers with the value given — once each, then the fixture is back.
  const nextFailures = new Map<string, number>();
  const nextAnswers = new Map<string, unknown>();
  const heldCommands = new Map<string, () => void>();
  const heldCommandWaits = new Map<string, Promise<void>>();
  /// The destinations `open_creator_link` was asked for, so a spec can see the click reached it.
  const creatorLinks: unknown[] = [];
  /// What `copy_text` was asked to copy, so a spec can check exactly what reached the clipboard.
  const copied: string[] = [];
  const failNext = (command: string, times = 1) => {
    nextFailures.set(command, times);
  };
  const answerNext = (command: string, value: unknown) => {
    nextAnswers.set(command, value);
  };
  const holdNext = (command: string) => {
    heldCommandWaits.set(command, new Promise<void>((resolve) => heldCommands.set(command, resolve)));
  };
  const releaseCommand = (command: string) => {
    const release = heldCommands.get(command);
    if (!release) return false;
    heldCommands.delete(command);
    release();
    return true;
  };

  (window as Window & {
    __SAKATSU_QA__?: {
      commands: string[];
      finishStartup: (role?: string) => void;
      releaseTranslation: () => boolean;
      releaseRequest: (key: string, how?: "complete" | "fail") => boolean;
      pendingRequests: () => string[];
      statusCalls: Token[];
      contextCalls: { surfaces: string[]; index: number }[];
      failNext: (command: string, times?: number) => void;
      answerNext: (command: string, value: unknown) => void;
      holdNext: (command: string) => void;
      releaseCommand: (command: string) => boolean;
      creatorLinks: unknown[];
      copied: string[];
      emitUpdate: (status: unknown) => void;
      startColdCapture: () => void;
      emitDictionaryDrop: (paths: string[], position: { x: number; y: number }) => void;
      pauseOptionalDownload: () => void;
      setOsTheme: (theme: "light" | "dark") => void;
    };
  }).__SAKATSU_QA__ = {
    commands,
    finishStartup: (role) => { if (role) startupPending.delete(role); else startupPending.clear(); startupFailed = false; startupUnavailable = false; },
    releaseTranslation,
    releaseRequest,
    pendingRequests,
    statusCalls,
    contextCalls,
    failNext,
    answerNext,
    holdNext,
    releaseCommand,
    creatorLinks,
    copied,
    emitUpdate: (status) => { void emit("update-status", status); },
    startColdCapture: () => {
      void emit("ocr-engine", { name: "vlm", ready: false });
      void emit("capture-requested", null);
    },
    emitDictionaryDrop: (paths, position) => { void emit("tauri://drag-drop", { paths, position }); },
    pauseOptionalDownload: () => {
      optionalBitsPaused = true;
      void emit("fetch-done", { id: "pronunciation", ok: false, detail: "paused" });
    },
    // The desktop switching between light and dark. The window's own event, so it reaches the
    // listener `watchTheme` registers exactly as the real one would; the mock delivers events by
    // name, whichever window they were addressed to.
    setOsTheme: (theme) => {
      osTheme = theme;
      void emit("tauri://theme-changed", theme);
    },
  };
  mockIPC(
    async (command, payload) => {
      commands.push(command);
      const args = (payload ?? {}) as Record<string, unknown>;
      const managerAnswer = command === "dictionary_manager" ? structuredClone(manager) : null;
      const wait = heldCommandWaits.get(command);
      if (wait) {
        heldCommandWaits.delete(command);
        await wait;
      }
      const failures = nextFailures.get(command) ?? 0;
      if (failures > 0) {
        nextFailures.set(command, failures - 1);
        throw `${command} failed in this fixture`;
      }
      if (nextAnswers.has(command)) {
        const answer = nextAnswers.get(command);
        nextAnswers.delete(command);
        return answer;
      }
      switch (command) {
        case "settings": return keptSettings;
        // Dictionary index maintenance. `upkeep` chooses the state under test: `missing` is the
        // one that matters — English search works and the sequence bucket does not exist, which
        // is what nothing used to report — and `build` decides whether the rebuild succeeds,
        // fails, or is refused because another resource operation owns the dictionaries.
        case "setup_has_bundle": return params.get("bundle") === "yes";
        case "dictionary_manager": {
          if (managerState === "unreadable") throw new Error("The dictionary status could not be read.");
          return managerAnswer;
        }
        case "dictionary_preferences":
          manager.preferences = args.preferences as typeof manager.preferences;
          sessionStorage.setItem("qa-dictionary-preferences", JSON.stringify(manager.preferences));
          void emit("dictionary-manager-changed", null);
          return null;
        case "dictionary_dismiss_updates":
          for (const [id, revision] of Object.entries(args.updates as Record<string, string>)) {
            if (manager.dictionaries.some((row) => row.id === id && row.available_revision === revision)) manager.dismissed_updates[id] = revision;
          }
          sessionStorage.setItem("qa-dismissed-dictionary-updates", JSON.stringify(manager.dismissed_updates));
          void emit("dictionary-manager-changed", null);
          return null;
        case "dictionary_connect_publisher":
          manager.dictionaries[0].source = "https://jitendex.org/static/yomitan.json";
          manager.dictionaries[0].consented = true;
          void emit("dictionary-manager-changed", null);
          return null;
        case "dictionary_source_consent":
          manager.dictionaries[0].consented = Boolean(args.allowed);
          return null;
        case "dictionary_open_source": return null;
        case "dictionary_check":
          manager.operation = { id: "check-1", target: "all", phase: "checking", detail: "Checking publisher?", bytes: 0 };
          window.setTimeout(() => {
            const row = manager.dictionaries[0];
            if (row) { row.available_revision = "2026.10.18.0"; row.phase = "available"; }
            manager.operation = { id: "check-1", target: "all", phase: "complete", detail: "Publisher checks finished.", bytes: 0 };
            void emit("dictionary-manager-changed", null);
          }, 100);
          return "check-1";
        case "dictionary_update":
          manager.operation = { id: "update-1", target: "fixture", phase: "downloading", detail: "Downloading?", bytes: 1024 };
          window.setTimeout(() => {
            const row = manager.dictionaries[0];
            if (managerState === "update-fails") {
              manager.operation = { id: "update-1", target: "fixture", phase: "failed", detail: "The publisher archive was invalid. The installed resource was kept.", bytes: 0 };
            } else {
              if (row) { row.revision = row.available_revision ?? row.revision; row.available_revision = null; row.phase = "current"; }
              manager.operation = { id: "update-1", target: "fixture", phase: "complete", detail: "Dictionary ready.", bytes: 0 };
            }
            void emit("dictionary-manager-changed", null);
          }, 100);
          return "update-1";
        case "dictionary_preview":
          return { id: "preview-1", kind: "words", title: "Imported dictionary", revision: "revision-b", source: null, sha256: "a".repeat(64) };
        case "dictionary_import":
          manager.operation = { id: "import-1", target: String(args.target ?? "new"), phase: "complete", detail: "Dictionary ready.", bytes: 0 };
          return "import-1";
        case "dictionary_cancel": return null;
        case "dictionary_upkeep": {
          if (upkeepState === "unreadable" && upkeepReadable) {
            // Fails once, so "Check again" has something to recover from.
            upkeepReadable = false;
            throw new Error("the dictionary store could not be read");
          }
          if (upkeepState === "none") return [];
          if (upkeepState === "complete") {
            return [{ id: "jitendex", title: "Jitendex.org", has_glosses: true, has_kanji: true, has_sequence: true, needs_indexes: false }];
          }
          if (upkeepState === "duplicate") {
            // Two installs, one title. Keyed on the title this is a duplicate key; keyed on the
            // install name it is two rows, which is what it is.
            return [
              { id: "jitendex", title: "Jitendex.org", has_glosses: true, has_kanji: true, has_sequence: upkeepBuilt, needs_indexes: !upkeepBuilt },
              { id: "jitendex-2024", title: "Jitendex.org", has_glosses: true, has_kanji: true, has_sequence: false, needs_indexes: true },
            ];
          }
          if (upkeepState === "unreadable") {
            return [{ id: "jitendex", title: "Jitendex.org", has_glosses: true, has_kanji: true, has_sequence: true, needs_indexes: false }];
          }
          if (buildOutcome === "partial") {
            // One rebuilt, one still missing: the state the backfill leaves when it walks two
            // dictionaries and the second fails.
            return [
              { id: "jitendex", title: "Jitendex.org", has_glosses: true, has_kanji: true, has_sequence: upkeepBuilt, needs_indexes: !upkeepBuilt },
              { id: "jmnedict", title: "JMnedict", has_glosses: true, has_kanji: true, has_sequence: false, needs_indexes: true },
            ];
          }
          return [{
            id: "jitendex",
            title: "Jitendex.org",
            has_glosses: true,
            has_kanji: true,
            has_sequence: upkeepBuilt,
            needs_indexes: !upkeepBuilt,
          }];
        }
        case "build_gloss_index": {
          // The real command answers by event, and `buildGlossIndex` awaits that event. The
          // delay is what makes the in-progress label observable rather than a frame nobody
          // could catch.
          window.setTimeout(() => {
            const ok = buildOutcome === "ok";
            // `partial` publishes one dictionary and then reports the other's failure — the
            // backfill keeps walking, and `failed` holds the last one.
            if (ok || buildOutcome === "partial") upkeepBuilt = true;
            emit("gloss-index-built", {
              ok,
              detail:
                buildOutcome === "ok"
                  ? "Jitendex.org (433,885 entries)"
                  : buildOutcome === "busy"
                    ? "another resource operation is using the dictionaries"
                    : buildOutcome === "partial"
                      ? "JMnedict: a shard could not be written"
                      : "a shard could not be written",
            });
          }, 40);
          return null;
        }
        case "setup_snapshot": {
          if (setupPhase === "downloading" && !initialSetupProgressSent) {
            initialSetupProgressSent = true;
            window.setTimeout(() => {
              void emit("setup-progress", {
                generation: 4, component: "manga-ocr", artifact: "encoder_model.onnx",
                step: 1, steps: 6, done: 188_743_680, total: 343_454_249,
                speed: 4_194_304, eta: 37, attempt: 2,
              });
            });
          }
          return setupSnapshot();
        }
        case "setup_preflight": return {
          target_location: setupSnapshot().storage.target,
          writable: true,
          required_bytes: setupSnapshot().storage.download_bytes,
          available_bytes: setupSnapshot().storage.available_bytes,
          peak_bytes: setupSnapshot().storage.peak_bytes,
          enough_space: true,
          network_available: params.get("bundle") !== "yes",
          supported_windows: true,
          supported_architecture: true,
          webview2: true,
          blocking_issue: null,
        };
        case "setup_start": {
          analyzerSetupPending = true;
          setupPhase = "downloading";
          setupWelcomeSeen = true;
          void emit("setup-snapshot-changed", null);
          void emit("setup-progress", {
            generation: 4, component: "manga-ocr", artifact: "encoder_model.onnx",
            step: 1, steps: 6, done: 188_743_680, total: 343_454_249,
            speed: 4_194_304, eta: 37, attempt: 2,
          });
          if (setupMode === "complete-after-start") {
            window.setTimeout(() => {
              setupReady = true;
              setupPhase = "complete";
              void emit("setup-snapshot-changed", null);
            }, 80);
          }
          return true;
        }
        case "setup_pause":
          setupPhase = "paused_retrying";
          void emit("setup-snapshot-changed", null);
          return null;
        case "setup_finish_analyzer":
          if (params.get("ichiran") === "save-fails") throw "The setup choice could not be saved.";
          analyzerSetupPending = false;
          void emit("setup-snapshot-changed", null);
          return null;
        case "start_ichiran_setup":
          if (params.get("ichiran") !== "running") {
            window.setTimeout(() => {
              const ok = params.get("ichiran") !== "install-fails";
              ichiranReady = ok;
              void emit("ichiran-setup-finished", {
                ok, detail: ok ? "Ichiran setup completed. Choose Use Ichiran now." : "Restart Windows if requested, then choose Set up again.",
              });
            }, 80);
          }
          return { detail: "Setup opened in PowerShell. Leave that window open while it works." };
        case "setup_use_text_only":
          setupTextOnly = true;
          void emit("setup-snapshot-changed", null);
          return null;
        case "setup_choose_storage": return null;
        case "setup_import_offline": return true;
        case "save_settings":
          keptSettings = args.settings as Settings;
          return keptSettings;
        // The three binding commands, which go through `rebind` in Rust rather than through
        // `save_settings`. Kept here so a spec can record one and watch the row change: without
        // them a recorded binding came back as "no IPC fixture", which reads in the interface
        // exactly like the binding being refused.
        // Never resolves, so `capturing` stays true and the hint can be read in the state it
        // is only ever in for a few seconds on a real machine. The fixture that uses this also
        // reports the engine as not yet loaded, which is what a cold PaddleOCR-VL read is.
        case "read_frame":
          return new Promise<string>(() => {});
        case "set_capture_bind":
          keptSettings = { ...keptSettings, capture_bind: args.bind as Binding };
          return null;
        case "set_overlay_bind":
          keptSettings = { ...keptSettings, overlay_bind: args.bind as Binding };
          return null;
        case "set_switch_bind":
          keptSettings = { ...keptSettings, switch_bind: (args.bind ?? null) as Binding | null };
          return null;
        case "titlebar_metrics": return { height: 32, button_width: 46 };
        case "dictionaries": return [dictionary];
        case "analyzer_engine": return "built-in";
        // The id Rust reports, not a label. Answering "Auto" here meant `engineLabel` was
        // never exercised by a layout test, and its output happened to match by accident.
        //
        // **Ready**, because the fixture's engine is a stub that answers instantly — there is
        // no load for it to be waiting on. The one spec that wants the cold state drives it
        // explicitly, by emitting `ocr-engine` with `ready: false` after the mount.
        case "ocr_engine":
          return { name: params.get("engine") ?? "auto", ready: true };
        case "update_status":
          return { generation: 1, phase: params.get("update") ?? "not_configured", version: "1.0.1", notes: "A shorter release note.", downloaded: 1024, total: null, error: null };
        case "update_check":
          return { generation: 2, phase: "available", version: "1.0.1", notes: "A shorter release note.", downloaded: 0, total: null, error: null };
        case "update_download":
          return { generation: 3, phase: "ready", version: "1.0.1", notes: "", downloaded: 1024, total: 1024, error: null };
        case "update_install":
          return { generation: 4, phase: "ready", version: "1.0.1", notes: "", downloaded: 1024, total: 1024, error: "Installation refused by fixture." };
        case "runtime_status":
          if (startupFailed) throw new Error("Startup status fixture unavailable");
          {
            const status = runtimeStatusFor(params, unloaded);
            for (const engine of status.engines) {
              if (startupUnavailable && engine.role === "recognition") { engine.state = "not_loaded"; engine.loaded = null; }
              if (startupPending.has(engine.role)) { engine.state = "loading"; engine.loaded = null; }
            }
            return status;
          }
        case "unload_ocr":
        case "unload_translator": {
          const role = command === "unload_ocr" ? "recognition" : "translation";
          const status = runtimeStatusFor(params, unloaded);
          const wasLoaded = status.engines.some((engine) => engine.role === role && engine.state === "ready");
          unloaded.add(role);
          return wasLoaded;
        }
        case "setup_restart_artifact":
          if (args.componentId === "pronunciation") optionalPartialDiscarded = true;
          return null;
        case "diagnostic_report":
          return DIAGNOSTIC_REPORT;
        case "copy_text":
          copied.push(String(args.text ?? ""));
          return null;
        case "store_error":
          // Null for `store-refuses` on purpose: that store opened, so there is no standing
          // warning — which is exactly what makes the refusal's own message the only account of it.
          return params.get("fixture") === "store-unavailable" ? STORE_UNAVAILABLE : null;
        case "recovery_report": return recoveryReport(params);
        // Resolving succeeds unless the fixture asks for one of the two failures the panel has to
        // tell apart: a screen describing a moment that has passed, and something that genuinely
        // went wrong.
        case "recovery_keep_current":
        case "recovery_restore_previous": {
          const how = params.get("recovery");
          if (how === "stale" || how === "stale-last") {
            // **The refresh that follows must not find it.** Staleness *means* the record has
            // already gone; a fixture that threw and then kept the entry was modelling a situation
            // that cannot occur, and the test written against it asserted a message in a case the
            // real panel never reaches.
            resolvedRecoveries.add(String(args.container));
            throw { code: "stale_recovery", message: "this recovery information is out of date (transaction t-1 is no longer outstanding); refresh and look again" };
          }
          if (how === "fails") {
            throw { code: "write_failed", message: "Access is denied. (os error 5)" };
          }
          resolvedRecoveries.add(String(args.container));
          return null;
        }
        case "readiness": return { components: [], importable: [] };
        case "credits": return ["1.0.0", credits];
        case "notices_available": return true;
        case "open_notices": return null;
        // Recorded and answered, never launched: the real command hands an address to the shell.
        case "open_creator_link":
          creatorLinks.push(args.destination);
          return null;
        // One source link, so the About page's per-source line is drawn by a spec. A copy, because
        // setup and onboarding read the same list and none of them is about sources.
        case "catalog":
          return optionalCatalog.map((entry) =>
            entry.id === "translation"
              ? {
                  ...entry,
                  sources: [
                    { name: "FuguMT model card", licence: "CC BY-SA 4.0", home: "https://huggingface.co/staka/fugumt-ja-en" },
                  ],
                }
              : entry,
          );
        case "audio_known": return params.get("fixture") === "audio"
          ? [{ key: { term: "伝わる", reading: "つたわる" }, available: true }]
          : [];
        // Answered for the words actually asked about, rather than a blanket empty list. The
        // speak control moved to the front of a Reference card, so a fixture that says nothing
        // is playable leaves that control untested. 図書館 has a recording and the rest do not,
        // which covers the button appearing *and* its deliberate absence beside a silent word.
        case "audio_availability": {
          const asked = (args.words ?? []) as { term: string; reading: string }[];
          return asked.map((key) => ({ key, available: key.term === "図書館" }));
        }
        case "window_maximized": return false;
        case "word_counts": return { seen: 79, known: 16, ignored: 0 };
        // `long-paths` is a collection with nothing of Sakatsu's in it and one very long deck
        // name, which puts the "nowhere to send a card" warning on screen.
        case "anki_decks": return params.get("fixture") === "anki"
          ? ["Sakatsu"]
          : params.get("fixture") === "long-paths" ? [LONG_DECK] : [];
        case "anki_deck_info": return params.get("fixture") === "anki"
          ? [{ name: "Sakatsu", ours: 1, foreign: 0, made_here: true }]
          : params.get("fixture") === "long-paths"
            ? [{ name: LONG_DECK, ours: 0, foreign: 120, made_here: false }]
            : [];
        case "log_file": return params.get("fixture") === "long-paths"
          ? { path: `${LONG_PATH}\\sakatsu.log`, bytes: 48_213, exists: true }
          : null;
        case "corpus_file": return params.get("fixture") === "long-paths"
          ? { path: `${LONG_PATH}\\sentences.txt`, bytes: 9_120, exists: true, passages: 212 }
          : null;
        // What the desktop prefers, for `system` mode. `?os=light` seeds it, and
        // `__SAKATSU_QA__.setOsTheme` changes it the way Windows would, mid-session.
        case "plugin:window|theme": return osTheme;
        // The vision reader as a machine without it reports it — the wording is
        // `vlm_setup::current_status`'s. Unanswered, Recognition printed this harness's own
        // "no IPC fixture" error under the engine selector in every settings screenshot.
        case "vlm_setup_status": return {
          installed: false,
          ready: false,
          endpoint: "http://127.0.0.1:8140",
          model: "paddleocr-vl-manga",
          detail: "Not set up — about 1.8 GB, and nothing is installed outside this app.",
          secondsPerRead: null,
        };
        case "frame_clickthrough": return null;
        case "reload_analyzer": return ichiranReady && params.get("ichiran") !== "probe-fails" ? "ichiran" : "built-in";
        case "reload_frequency": return null;
        case "study": {
          const asked = String(args.text ?? "").trim();
          // Held before anything is recorded, so a passage that never finishes never becomes the
          // active text — which is what the reader's own staleness rule has to survive.
          await waitForRequest("study", asked);
          studyCalls += 1;
          if (params.has("reject-second-study") && studyCalls === 2) {
            // `both`: the message core sends when Ichiran failed and the built-in retry failed too.
            throw new Error(
              params.get("reject-second-study") === "both"
                ? "ichiran could not analyze this passage (analyzer timed out: no answer), and lindera could not either (analyzer unavailable: broken build)"
                : "invalid source spans: source 3..6 (\"・\") is covered by no token",
            );
          }
          activeText = asked;
          return study(activeText, params);
        }
        case "lookup_in_context": {
          const tokens = args.tokens as Token[];
          const chosen = tokens[Number(args.index ?? 0)];
          contextCalls.push({
            surfaces: tokens.map((token) => token.surface),
            index: Number(args.index ?? 0),
          });
          // Keyed by what the reader shows, so a test releases "the lookup for this word" rather
          // than an index it has to keep in step with the tokeniser.
          await waitForRequest("lookup", chosen?.surface ?? String(args.index ?? 0));
          return wordFor(chosen, params);
        }
        case "search_words":
          await waitForRequest("search", String(args.query ?? ""));
          return referenceResult(params);
        case "search_grammar": return params.get("fixture") === "mixed" ? [qaPattern] : [];
        case "search_terms": return params.get("fixture") === "mixed" ? [glossaryTerms()[0]] : [];
        case "search_particles": return params.get("fixture") === "mixed" && String(args.query ?? "") === "に"
          ? [{
              surface: "に",
              jlpt: "N5",
              note: "Marks where something is, ends up, happens, or who receives it.",
              senses: [{
                label: "where something is",
                gloss: "With いる and ある, に marks where a thing simply is.",
                examples: [{ japanese: "机の上に本があります。", english: "There is a book on the desk." }],
              }],
            }]
          : [];
        case "lookup_kanji": return params.get("fixture") === "mixed"
          ? {
              character: String(args.character ?? "図"),
              on: [{ text: "ズ", here: false }],
              kun: [{ text: "え", here: false }],
              nanori: ["のり"],
              meanings: ["map", "drawing", "plan"],
              strokes: 7,
              grade: 2,
              components: ["囗", "斗"],
              irregular: false,
            }
          : null;
        case "patterns_for_term": return String(args.id ?? "") === "conjunctive" ? [qaPattern] : [];
        case "gloss_index_ready": return true;
        case "kanji_diagram":
          await waitForRequest("diagram", String(args.character ?? ""));
          return {
          strokes: [
            { path: "M18 18L91 18" },
            { path: "M20 24L20 91" },
            { path: "M27 88L89 88" },
          ],
          numbers: [
            { number: 1, x: 10, y: 18 },
            { number: 2, x: 13, y: 30 },
            { number: 3, x: 20, y: 84 },
          ],
        };
        // 図 is the character the layout suite opens. Real words, and a `total` far larger than
        // the page shows, so the bounded-list count is exercised rather than assumed.
        case "kanji_compounds": return {
          character: String(args.character ?? ""),
          total: 214,
          indexed: true,
          words: [
            { headword: "図書館", reading: "としょかん", glosses: ["library"] },
            { headword: "地図", reading: "ちず", glosses: ["map"] },
            { headword: "意図", reading: "いと", glosses: ["intention", "aim", "design"] },
          ],
        };
        case "kanji_related": return {
          component: String(args.component ?? ""),
          total: 30,
          characters: [
            { character: "囲", meanings: ["surround"], strokes: 7, grade: 4 },
            { character: "国", meanings: ["country"], strokes: 8, grade: 2 },
            { character: "園", meanings: ["garden"], strokes: 13, grade: 2 },
          ],
        };
        case "particle_senses": return null;
        // The expression offer, driven by `?expression=`:
        //   `offer`  — a のに entry over two tokens, so the selection is *part of* the expression
        //   `whole`  — the same entry over one token, so the selection *is* it
        //   `fail`   — a dictionary that could not be read, which must look different from none
        //   absent   — no offer, the ordinary answer for most selections
        //
        // The two offer modes exist because the relationship label the reader sees differs
        // between them, and only one of the two shapes occurs in production today — so a fixture
        // is the only way to render the other at all.
        case "expression_offer": {
          // Held like the others when asked, so a test can arrange "the whole word's offer is
          // still in flight when a part is opened" — the sequence the cancellation exists for.
          await waitForRequest(
            "expression",
            (args.tokens as Token[])[Number(args.index ?? 0)]?.surface ?? String(args.index ?? 0),
          );
          const mode = params.get("expression");
          if (mode === "fail") throw new Error("the dictionary could not be read");
          // `within` and `overlap` are the two shapes production cannot reach under Lindera: a
          // fused token holding the expression, and an ending that begins inside a token which
          // also holds text before it. Both are real under other tokenizations, and each gets a
          // different sentence, so each is rendered from a fixture.
          if (!["offer", "whole", "within", "overlap"].includes(mode ?? "")) return null;
          // Only for a token the match covers, so the test can prove selecting elsewhere in the
          // sentence offers nothing.
          const matches = (args.matches ?? []) as { pattern_id: string }[];
          if (!matches.some((match) => match.pattern_id === "noni")) return null;
          // The first token is 0..9, so an offer over 0..9 *is* it, one over 0..12 contains it,
          // one over 3..6 sits inside it, and one over 6..12 overlaps it.
          const span = { whole: [0, 9], offer: [0, 12], within: [3, 6], overlap: [6, 12] }[
            mode as "whole" | "offer" | "within" | "overlap"
          ];
          const extent = {
            whole: "same",
            offer: "contains_selection",
            within: "within_selection",
            overlap: "overlapping",
          }[mode as "whole" | "offer" | "within" | "overlap"];
          return {
            expression: "のに",
            pattern_id: "noni",
            title: "のに",
            start: span[0],
            end: span[1],
            extent,
            senses: [
              { tags: ["particle"], glosses: ["although", "despite", "even though"], sequence: 1009980 },
              { tags: ["particle"], glosses: ["in order to"], sequence: 1009980 },
            ],
            source: "Jitendex.org",
          };
        }
        // The part's own stored status. `known` only for the part the tests mark, so
        // "the part is Known and the whole word is not" is visible rather than asserted about
        // two identical values.
        case "word_status": {
          const asked = (args.token as Token | undefined)?.surface ?? "";
          return params.get("partKnown") === asked ? "known" : "new";
        }
        case "explain_tags": return {
          n: { label: "noun", description: "A person, place, thing or idea." },
          prt: { label: "particle", description: "Marks a relationship in the sentence." },
          // A tag with no entry falls to the raw-code pill rather than the label, so anything a
          // test wants to see *as a label* has to be explainable here.
          particle: { label: "particle", description: "Marks a relationship in the sentence." },
          conjunction: { label: "conjunction", description: "Joins two clauses." },
          adv: { label: "adverb", description: "Says how, when or how much." },
        };
        case "translate_sentences": {
          if (params.get("fixture") === "error") throw new Error("The local translation model is unavailable.");
          if (params.get("fixture") === "loading") {
            await new Promise((resolve) => window.setTimeout(resolve, 350));
          }
          return alignedTranslations(args.tokens as Token[], params);
        }
        case "translate": {
          if (params.get("fixture") === "context-error") throw new Error("Passage context is unavailable.");
          if (params.get("fixture") === "context-loading") {
            await waitForTranslation();
          }
          // The whole-passage view needs its own long text, or `long-translation` clips line by
          // line and not as one passage — and the clamp is a property of the view, not the card.
          if (params.get("fixture") === "long-translation") {
            return "Read as one passage, this is long enough to be clipped in its own right, which is what the whole-passage view needs if its clamp and its disclosure are to be checked at all. It keeps going well past three lines at every viewport the layout suite runs, so the measurement is of real geometry rather than of a sentence that happened to fit.";
          }
          return activeText.includes("前の文")
            ? "This is the preceding sentence. My feelings are coming through."
            : "My feelings are coming through.";
        }
        case "translate_variants":
          if (params.get("fixture") === "comparison-error") {
            throw new Error("Focused comparison is unavailable.");
          }
          if (params.get("fixture") === "comparison-loading") {
            await waitForTranslation();
          }
          return comparison(activeText);
        case "set_word_status":
          statusCalls.push(args.token as Token);
          // Rejects with a `PersistenceError`, which is what the real command does now — a plain
          // JSON object, since there is no class across the IPC boundary. The `code` is what the
          // interface branches on; the fixture supplies the real sentence beside it so the
          // rendered result is what a reader would actually see.
          if (params.get("fixture") === "store-unavailable") {
            throw { code: "store_unavailable", message: STORE_UNAVAILABLE };
          }
          // A store that opened and refused **this** write. Nothing on screen explains it, so the
          // interface must print it rather than answering with the short line — the distinction
          // the codes exist to draw, and one only a rendered page can be checked for.
          if (params.get("fixture") === "store-refuses") {
            throw { code: "write_failed", message: WRITE_FAILED };
          }
          return args.status;
        case "explain_pattern": return typeof args.id === "string" ? {
          id: args.id,
          pattern: args.id === "te-kuru" ? "〜てくる" : `fixture ${args.id}`,
          title: args.id === "te-kuru" ? "Te-form plus 来る" : args.id,
          jlpt: args.id === "te-kuru" ? "N4" : null,
          formation: args.id === "te-kuru" ? "te-form + くる" : null,
          explanation: args.id === "te-kuru"
            ? "The action comes toward the speaker, or has been building up until now."
            : "A deterministic sentence-level explanation long enough to exercise the Grammar scrollport without changing production grammar data.",
          detail: null,
          term_ids: [],
          examples: [],
          reference: null,
          match: null,
        } : null;
        case "add_to_anki": return 1;
        case "all_terms": return ["glossary", "mixed"].includes(params.get("fixture") ?? "") ? glossaryTerms() : [];
        case "look_term": return null;
        default:
          throw new Error(`Analysis QA has no IPC fixture for ${command}`);
      }
    },
    { shouldMockEvents: true },
  );

  // `?ichiran=listen-fails`: the first subscription to setup completion is refused, as a closed
  // event channel would refuse it, and the next one is accepted — so the retry has something to do.
  if (params.get("ichiran") === "listen-fails") {
    const internals = (window as unknown as {
      __TAURI_INTERNALS__: { invoke: (cmd: string, args?: { event?: string }, options?: unknown) => Promise<unknown> };
    }).__TAURI_INTERNALS__;
    const through = internals.invoke.bind(internals);
    let refused = false;
    internals.invoke = (cmd, args, options) => {
      if (!refused && cmd === "plugin:event|listen" && args?.event === "ichiran-setup-finished") {
        refused = true;
        return Promise.reject("the event channel is closed");
      }
      return through(cmd, args, options);
    };
  }

  // Cold-OCR tests call startColdCapture after the app has registered its listeners.
  // A zero-delay timer can run before a dynamically loaded app module has mounted.
}
