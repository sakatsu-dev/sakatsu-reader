/**
 * The TypeScript mirror of `sakatsu_core::model` (core/src/model.rs) — the shape
 * the Rust core serializes across the Tauri IPC boundary.
 *
 * This file is hand-written and therefore *could* drift from the Rust types.
 * It does not, because `model.contract.ts` type-checks it against
 * `model.golden.json`, which a Rust test regenerates from the real types. Change
 * the Rust model and `npm run check` tells you exactly what to change here.
 *
 * Field names are snake_case because serde emits the Rust names verbatim; the
 * `| null` unions are Rust `Option<T>`.
 */

/**
 * The generated half of the contract.
 *
 * These types are **not written here**. `ui/src/bindings.ts` is generated from the Rust types by
 * `cargo run -p sakatsu-core --features typescript --bin generate-bindings`, and this file
 * re-exports it so existing imports from `./model` keep working.
 *
 * Handwritten copies used to live below. They were correct, and they were a second thing that
 * could disagree — which is what a contract file must not be. What a handwritten copy could never
 * do is carry the enums honestly: `LineBreak`, `WordStatus` and `Form` are literal unions derived
 * from the Rust enums now, so renaming a variant in Rust fails the frontend that still uses the
 * old name. `model.golden.json` cannot catch that, because a JSON import widens literals away —
 * which is exactly why `model.contract.ts` has to widen those three back to `string`.
 *
 * `FormId` is this file's name for the Rust `Form` enum. The other `Form` below mirrors
 * `commands::Form` — an alternative spelling of a word — and is a different type that happens to
 * share a word.
 */
export type {
  AlsoIn,
  Alternative,
  Analysis,
  Auxiliary,
  Breakdown,
  Conjugation,
  DictionaryUpkeep,
  Construction,
  ConstructionGrammar,
  ConstructionIdentity,
  ConstructionLookup,
  CharReading,
  CatalogSource,
  CatalogEntry,
  KanjiView,
  WordHit,
  Found,
  ContextualWordView,
  DictExample,
  DictionaryRecovery,
  DictionaryRecoveryState,
  ErrorCode,
  ExpressionOffer,
  Formation,
  LineBreak,
  Match,
  MergedSense,
  AnalyzerPolicy,
  Execution,
  Fallback,
  FallbackReason,
  FallbackScope,
  RepairNote,
  ContractionFamily,
  StemEvidence,
  InstallRecovery,
  InstallRecoveryState,
  LoadingOutcome,
  LexicalRole,
  NameHit,
  PersistenceError,
  Prop,
  RecoveryReport,
  RequestIdentity,
  Retrieved,
  Ruby,
  Sense,
  SentenceRange,
  Step,
  SourceSpan,
  StudyView,
  Token,
  VocabularyRanking,
  VocabularySenseRank,
  WordStatus,
  WordView,
  WorkPriority,
  // Runtime status: what is running, and what each operation used.
  Acceleration,
  CacheUse,
  EngineRole,
  EngineState,
  EngineStatus,
  FailureCode,
  Hosting,
  OperationResult,
  Outcome,
  RuntimeStatus,
} from "./bindings";
export type { SearchForm as Form, Form as FormId } from "./bindings";

// Re-exporting a name does not bring it into this module's own scope, and some of the
// handwritten types below refer to these.
import type { WordStatus } from "./bindings";


/**
 * The Japanese the comparison strip asked about (`sakatsu_core::translate::Variants`).
 */
export type Variants = {
  /** The sentence as written. */
  full: string;
  /** The selected words alone — one string per run of adjacent tokens. */
  runs: string[];
  /** The sentence with the selection removed; empty when nothing would be left. */
  without: string;
};

/**
 * What the sentence loses without the struck words (`commands::VariantTranslations`).
 *
 * The selection is no longer translated on its own: `core/tests/lens_agreement.rs` measured that
 * reading at 34.6% for ordinary words and 1.4% for particles, so it was removed rather than
 * dressed up. `japanese.runs` still carries the Japanese, because the measurement uses it.
 *
 * Not a grammar reference. See `project notes` §7.
 */
export type VariantTranslations = {
  japanese: Variants;
  full: string;
  /** Empty when the selection was the whole sentence. */
  without: string;
  /** `full`, in order, with the runs the removal took marked. */
  pieces: { text: string; gone: boolean }[];
  /**
   * Whether `without` is worth drawing under the marked-up line.
   *
   * False when it only repeats the words that survived the strike, which is the common case:
   * striking words out already shows what the sentence became. Decided in Rust beside
   * `word_diff`, so the panel and `lens_agreement.rs` cannot disagree about it.
   */
  result_says_more: boolean;
};

/** One exact sentence and its canonical English translation (`commands::SentenceTranslation`). */
export type SentenceTranslation = {
  /** The sentence's half-open range in the analysis token stream. */
  first_token: number;
  last_token_end: number;
  /** The exact Japanese string sent to the translator. */
  japanese: string;
  english: string;
};


/**
 * A deck, and what this app may do with it (`sakatsu_core::anki::DeckInfo`).
 *
 * `made_here && foreign === 0` is the gate on renaming and deleting, and Rust enforces it too —
 * this copy is for drawing the interface, not for deciding. The two halves answer different
 * questions: the mark says the deck is ours to act on, the count says acting on it would destroy
 * nothing. `Default` is excluded outright.
 */
/**
 * The palette, as inputs (`sakatsu_core::store::Theme`).
 *
 * Three values, not thirty: `ui/src/lib/theme.js` derives every colour token from them. The four
 * word-status hues are deliberately absent — the app owns those, so "new" and "known" cannot be
 * tuned into each other, though they do change between light and dark.
 */
export type ThemeSettings = {
  mode: "light" | "dark" | "system";
  surface_hue: number;
  accent_hue: number;
  /** Degrees the six part-of-speech hues are turned together. 0 is as designed. */
  kind_spin: number;
  /** How strong the surface cast is. 1 is as designed, 0 is neutral grey. */
  tint: number;
  /** The gap between text and what it sits on. 1 is as designed. */
  contrast: number;
  /** More ink, in either mode. 0 is as designed. */
  depth: number;
  /** Tokens set by hand, as `#rrggbb`. The only part of a palette that is not derived. */
  overrides: Record<string, string>;
};

/**
 * The look a palette carries beyond its colours (`sakatsu_core::store::ThemeDecor`).
 *
 * Apart from `ThemeSettings` because that is the input to the colour derivation and none of this
 * is a colour input. Sizes are deliberately not here: a palette that resized somebody's interface
 * when they tried it is why presets began as colour only, and a font size is how well they can
 * read rather than a matter of taste.
 */
export type ThemeDecor = {
  cloud: number;
  cloud_tint: DecorTint;
  sakura: number;
  sakura_tint: DecorTint;
  scale: number;
  cloud_pad: number;
  cloud_spread: number;
  sakura_spread: number;
  blur: boolean;
  cloud_blur: number;
  sakura_blur: number;
  animations: boolean;
  animation_ms: number;
};

/** A palette the learner named and kept (`sakatsu_core::store::NamedTheme`). */
export type NamedTheme = {
  name: string;
  theme: ThemeSettings;
  /**
   * Decoration and motion, when the palette was saved with them.
   *
   * Optional because palettes kept before this existed have none, and applying one of those
   * leaves the watermarks where they are rather than resetting them to a default nobody chose.
   */
  decor?: ThemeDecor | null;
};

export type DeckInfo = {
  name: string;
  ours: number;
  foreign: number;
  /** Whether the deck wears Sakatsu's options preset — that is, whether this app made it. */
  made_here: boolean;
};

// ---------------------------------------------------------------------------
// Grammar (sakatsu_core::grammar). Not covered by model.golden.json, which pins
// the analysis model; these are checked by usage in the components below.
// ---------------------------------------------------------------------------


/** A grammar pack entry — the single source of explanatory text. */
export type GrammarPattern = {
  id: string;
  pattern: string;
  title: string;
  jlpt: string | null;
  formation: string | null;
  explanation: string;
  /**
   * Short renderings for the vocabulary panel, when this pattern is also answered as a word.
   *
   * Empty for almost every pattern. A construction that groups into one selectable token —
   * なん — is answered from the pack rather than from a dictionary, and these fill the line a
   * dictionary sense fills; the `explanation` above stays in the grammar card.
   */
  glosses: string[];
  /** The longer version, when the pack has one. */
  detail: string | null;
  /** Explicit, reviewed glossary associations. */
  term_ids: string[];
  /** Classification codes for that same vocabulary entry, as JMdict writes them. */
  tags: string[];
  examples: { japanese: string; english: string }[];
  reference: { label: string; url: string } | null;
  match: unknown;
};

// ---------------------------------------------------------------------------
// Capture (sakatsu_core::ocr)
// ---------------------------------------------------------------------------

/** A region in pixels. The overlay reports CSS pixels; Rust scales them. */
export type Rect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

// ---------------------------------------------------------------------------
// First-run setup
// ---------------------------------------------------------------------------

/** One thing the app needs, and whether it is installed. */
export type Component = {
  name: string;
  ready: boolean;
  /** What was found, or what was looked for and missing. */
  detail: string;
  /** The command that fixes it, when one exists. */
  fix: string | null;
  /** Whether the supported installation is incomplete without it. */
  required: boolean;
  /** The catalog id this row is about, so the panel can offer its Get button. */
  catalog: string | null;
};

/** How a setup component is offered. Mirrors `setup::catalog::Tier`. */
export type { Tier } from "./bindings";

/**
 * One thing the About page credits. Mirrors `commands::CreditEntry`.
 *
 * The software only — every dictionary and model carries its licence in `CatalogEntry`, because
 * the setup screen has to show it before the download. The About page reads both and neither
 * string exists twice. See `core/src/credits.rs`.
 */
export type CreditEntry = {
  name: string;
  /** SPDX where there is one, prose where the terms are not a standard licence. */
  licence: string;
  url: string;
  /** One line on what it does here. This is the field that makes the page worth reading. */
  why: string;
  /** The heading to file it under, already named by Rust. */
  group: string;
};

/**
 * How far along a fetch is: the `fetch-progress` event, generated from the Rust type that emits it.
 *
 * It was handwritten here and declared six of the nine fields the wire carried, with `step`
 * optional because one of three anonymous payloads omitted it. Nothing could catch that; a
 * generated declaration is checked against the type serde uses.
 */
export type { FetchProgress } from "./bindings";

/** Everything the setup panel needs. */
export type Readiness = {
  components: Component[];
  /** Zips downloaded but not yet imported. */
  importable: string[];
};

// ---------------------------------------------------------------------------
// Study state (sakatsu_core::store)
// ---------------------------------------------------------------------------


/** The statuses a learner can set, in the order the controls show them. */
export const WORD_STATUSES: WordStatus[] = ["new", "seen", "known", "ignored"];

/** How many words are marked at each status. */
export type Counts = {
  seen: number;
  known: number;
  ignored: number;
};

/** Persisted preferences. */
/**
 * A mouse button that may be bound — all of them.
 *
 * Left and right included. The hook that watches them never swallows an event, so a
 * bound button goes on doing whatever it normally does; a strange choice costs a noisy
 * binding, not a machine that cannot be clicked.
 */
export type MouseButton = "left" | "middle" | "right" | "x1" | "x2";

/**
 * The mouse buttons that may be bound to an action.
 *
 * **Left and right are excluded, and it is the reader that excludes them.** A left click
 * selects a word and a right click marks one Known, and the global hook deliberately never
 * swallows an event — so binding either would fire a capture on top of a reading gesture made
 * for a different reason, with no click left over to separate them.
 *
 * Mirrors `MouseButton::bindable` in `core/src/store/mod.rs`, which is the authority: this copy
 * exists so the recorder can refuse without a round trip, and the command refuses again.
 */
export const BINDABLE_BUTTONS: readonly MouseButton[] = ["middle", "x1", "x2"];

/** Whether a recorded button may become a binding. */
export function isBindableButton(button: MouseButton): boolean {
  return BINDABLE_BUTTONS.includes(button);
}

/**
 * What fires a capture.
 *
 * Two shapes rather than one string, because the two are found by entirely different
 * machinery — the global-shortcut plugin has no notion of a mouse at all.
 */
export type Binding =
  | { kind: "key"; chord: string }
  | { kind: "mouse"; button: MouseButton };

/** Which analyzer to use. `auto` tries Ichiran and falls back to the built-in one. */
/**
 * Which analyzer breaks a passage into words.
 *
 * `ichimoe` is gone — it sent every analysed line to somebody else's server, and everything
 * else here reads Japanese on the machine it is installed on. Rust keeps the variant only so an
 * older settings file still parses, and `Settings::normalise` turns it into `auto` on the way in.
 */
export type AnalyzerChoice = "auto" | "ichiran" | "builtin";

/**
 * The analyzers offered, in the order they are offered — the same arrangement as `OCR_OPTIONS`
 * and for the same reason: the settings page and the app menu both draw this list now.
 *
 * `ichimoe` is deliberately absent. Rust still parses the old value, but nothing offers it.
 */
export const ANALYZER_OPTIONS: readonly { value: AnalyzerChoice; label: string }[] = [
  // Stored values unchanged: a saved "auto" or "builtin" means what it always did. "Auto", as the
  // recognition list says it, because the quick panel's select is 8rem wide and a longer label
  // clipped there; the settings page's note says what it does.
  { value: "auto", label: "Auto" },
  { value: "ichiran", label: "Ichiran" },
  { value: "builtin", label: "Built-in (Lindera)" },
];

/**
 * Which recognition engine reads a capture.
 *
 * Auto uses PP-OCRv6's detected direction and rejected-line signal to decide when a
 * capture also needs manga-ocr. `vlm` remains in the wire type only so settings written
 * while the paused experiment was exposed can still deserialize and migrate to Auto.
 */
export type OcrChoice = "auto" | "manga" | "paddle" | "vlm";

/**
 * The engines offered, in the order they are offered.
 *
 * **Here rather than in the control that draws them**, because two surfaces draw them now: the
 * settings page's `OcrSelect` and the title bar's menu. A second copy of four labels is how the two
 * come to offer different words for the same engine.
 *
 * Not the same map as `engineLabel`, which is a different question with a different key space: this
 * names the four things a reader may *choose*, that names whichever engine actually *ran* —
 * including `stub` and `pp-ocrv6`, which nobody picks.
 */
export const OCR_OPTIONS: readonly { value: OcrChoice; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "manga", label: "manga-ocr" },
  // PP-OCRv6 small since 2026-10-09. The stored value stays `paddle`, so a saved choice of
  // the typeset engine now means v6 without any migration.
  { value: "paddle", label: "PP-OCRv6" },
  // The label once carried "(local server)". It said nothing the settings page was not already
  // saying twice — the note under the field spells out the requirement, and `VlmSetup` mounts
  // directly beneath it — and in the app menu the qualifier cost more width than the whole rest
  // of the row. The engine is named; where to get it running is the page's job to explain.
  { value: "vlm", label: "PaddleOCR-VL" },
];

/** Preparation applied to a crop before recognition. Mirrors `store::PreprocessSettings`. */
export type PreprocessSettings = {
  /** Trim uniform margin from around the content. */
  tighten: boolean;
  /** Local contrast enhancement (CLAHE). */
  contrast: boolean;
  /** Enlarge a small crop before recognition. */
  upscale: boolean;
};

/** Where the diagnostic log is and how big it has grown. Mirrors `commands::LogFile`. */
export type LogFile = {
  path: string;
  bytes: number;
  exists: boolean;
};

/**
 * Where the collected corpus is and how much it holds. Mirrors `commands::CorpusFile`.
 *
 * `passages` rather than only `bytes`, because a size answers nothing about a corpus — what
 * decides whether it is worth sending is how many sentences are in it.
 */
export type CorpusFile = {
  path: string;
  bytes: number;
  exists: boolean;
  passages: number;
};

/** Session-only intent-prefetch counters. Mirrors `warmup::WarmupDiagnostics`. */
export type WarmupDiagnostics = {
  reference_entries: number;
  analysis_entries: number;
  reference_hits: number;
  reference_misses: number;
  analysis_hits: number;
  analysis_misses: number;
  reference_prefetches: number;
  analysis_prefetches: number;
  estimated_saved_ms: number;
  invalidations: number;
};

/** One reading a particle can have here. Mirrors `grammar::particles::RankedSense`. */
export type RankedSense = {
  id: string;
  /** A few words to scan — "where the action happens". */
  label: string;
  gloss: string;
  /**
   * Higher is better supported. **Zero means nothing pointed at it either way** — which is
   * not evidence against it, and the sense is still shown. The interface has to keep that
   * distinction visible rather than presenting a guess as an answer.
   */
  score: number;
  /** Why it scored, checkable against the sentence. Empty when nothing fired. */
  because: string[];
  /** Native sentences showing this reading; the translation is what makes them evidence. */
  examples: { japanese: string; english: string }[];
};

/**
 * One particle as the reference page shows it. Mirrors `commands::ParticleView`.
 *
 * Distinct from `Reading` below on purpose. That one belongs to a particle *in a
 * sentence* and carries scores, reasons and a favourite; this one has no sentence behind
 * it, so it has none of those and must not pretend to.
 */
export type ParticleView = {
  surface: string;
  /** Prose about the particle as a whole. May be empty. */
  note: string;
  senses: ParticleSenseView[];
};

/** One reading of a particle, with nothing said about how likely it is. */
export type ParticleSenseView = {
  id: string;
  /** A few words to scan — "where the action happens". */
  label: string;
  gloss: string;
  /**
   * The dictionary sense(s) this reading answers to, each the exact line the entry
   * prints — its glosses joined with `"; "`. Empty when the dictionary does not carve
   * this reading out separately, which is a gap in its carving and not an error.
   */
  dictionary: string[];
  examples: { japanese: string; english: string }[];
};

/** Every sense of one particle, and which the sentence points at. Mirrors `Reading`. */
export type Reading = {
  /** Best-supported first. Never filtered — the ranking reorders and nothing else. */
  senses: RankedSense[];
  /**
   * Index into `senses` of the favoured reading, or null.
   *
   * Null covers two things the panel must not present as a pick: nothing fired, and a
   * **tie at the top**. Rust decides this so the panel and the scored fixture cannot
   * drift apart about what "favoured" means.
   */
  favoured: number | null;
};

/**
 * One entry in the glossary of grammatical terms. Mirrors `sakatsu_core::terms::Term`.
 *
 * Covers two vocabularies at once: the codes a dictionary sense carries (`conj`, `vi`) and
 * the words the panels print in prose (causative, predicate). A reader wanting to know what
 * a conjunction *is* has the same question either way.
 */
/** One tag explained, as `explain_tags` answers for a whole word at once. */
export type TagExplanation = {
  label: string;
  detail: string;
};

/** Which of the glossary's five groups a term belongs to. */
export type TermKind = "form" | "concept" | "part-of-speech" | "verb-class" | "usage";

/** Headings for those groups, in the order the glossary lists them. */
export const TERM_KINDS: { kind: TermKind; label: string }[] = [
  { kind: "form", label: "Forms" },
  { kind: "concept", label: "Concepts" },
  { kind: "part-of-speech", label: "Parts of speech" },
  { kind: "verb-class", label: "Verb classes" },
  { kind: "usage", label: "Usage notes" },
];

export type Term = {
  /** For a grammatical form this is the breakdown's own `form` string. */
  id: string;
  label: string;
  /** Everything a search matches, lowercase. */
  aliases: string[];
  /** The choice-making line shared by result indexes and term details. */
  summary: string;
  /** Supporting prose, when the explanation needs more than the summary. */
  detail: string | null;
  examples: { japanese: string; english: string }[];
  /** Explicit, reviewed concept relations. They are never inferred from prose. */
  requires: string[];
  contrasts: string[];
  related: string[];
  kind: TermKind;
};

/** What colour a watermark is drawn in. Mirrors `store::DecorTint`. */
export type DecorTint = "ink" | "blossom" | "accent";

/**
 * Where each line of the passage starts in the reader. Mirrors `store::PassageAlign`; the values
 * are `text-align` keywords, so the reader can hand them straight over.
 */
export type PassageAlign = "left" | "center" | "right";

/** The alignments offered in the quick panel, in the order they are offered. */
export const PASSAGE_ALIGN_OPTIONS: readonly { value: PassageAlign; label: string }[] = [
  { value: "left", label: "Left" },
  { value: "center", label: "Center" },
  { value: "right", label: "Right" },
];

/** Under its headings, or one A–Z list. Mirrors `store::GlossarySort`. */
export type GlossarySort = "grouped" | "alpha";

/** Responsive browse presentation. Narrow pages temporarily force rows. */
export type BrowseLayout = "cards" | "rows";

export type Settings = {
  furigana: boolean;
  /** Promote a New word to Seen when a click or arrow-key move focuses it. */
  mark_seen_on_focus: boolean;
  /** Require a card to be focused before a click opens it, rather than revealing its tab on hover. */
  focus_before_open: boolean;
  /** Restore conventional Windows Tab traversal and retain clicked control focus. */
  tab_navigation: boolean;
  split_ratio: number;
  capture_bind: Binding;
  /** What shows the frame, and hides it while held. */
  overlay_bind: Binding;
  /** Where the capture frame was left: `[x, y, width, height]`, logical pixels. */
  frame_rect: [number, number, number, number] | null;
  /** A tap of the show/hide binding leaves the frame where it is, rather than bringing
      it to the pointer. Set from the frame's own toolbar. */
  frame_locked: boolean;
  /** Whether the capture overlay answers at all. Off, the frame cannot be shown and
      neither binding responds. */
  overlay_enabled: boolean;
  /** A hotkey that switches the overlay off and on. Null when none is set, which is the
      default and a legitimate state — the only binding that may be absent. */
  switch_bind: Binding | null;
  /** How long the overlay button holds a click before acting, so a double click can
      cancel it. Floored at 100 in Rust. */
  click_delay_ms: number;
  /** Which installed frequency list orders results, by directory name. Empty for none. */
  frequency_source: string;
  /** Whether that list's numbers are ranks or counts. Declared, never inferred. */
  frequency_direction: "rank" | "count";
  /** Show which frequency list ordered Reference results. Sorting is independent. */
  show_frequency_source: boolean;
  /** Which settings section was last open, by id — one of `SETTINGS_SECTIONS`. */
  settings_section: string;
  /** How long the show/hide binding must be held to count as a hold. */
  hold_ms: number;
  /** How quickly a second tap of the show/hide binding must follow the first to lock the
      frame. Zero turns the gesture off; otherwise clamped to 150–1000 in Rust. */
  lock_tap_ms: number;
  /** Hide the frame while capturing, instead of trusting its transparent interior. */
  hide_during_capture: boolean;
  /** The interface's root size in px; everything else is rem over it. */
  font_size: number;
  /** The analyzed passage's size, as a multiple of what `font_size` gives it. */
  reader_scale: number;
  /** The same, for the definitions and grammar panels. */
  panel_scale: number;
  /** Watch the clipboard and analyze Japanese as it is copied. */
  read_clipboard: boolean;
  /** Keep the analyzer sidecar's console window off the screen. */
  quiet: boolean;
  /** Keep one analyzer sidecar alive rather than starting one per passage. Ichiran only. */
  ichiran_server: boolean;
  /** Start Ichiran in the background after the window opens. */
  preload_ichiran_on_startup: boolean;
  check_updates_on_startup: boolean;
  /** Write a diagnostic log file. */
  log: boolean;
  /** Keep the Japanese you capture, as a corpus file and in the log. */
  collect_sentences: boolean;
  analyzer: AnalyzerChoice;
  ocr: OcrChoice;
  /** Where the vision-language model is served, when `ocr` is `"vlm"`. */
  vlm_endpoint: string;
  /** Which model to ask for. Empty for llama-server; Ollama and LM Studio route on it. */
  vlm_model: string;
  preprocess: PreprocessSettings;
  anki_deck: string;
  /** Hide decks an export would never go to. On by default; ignored while it would show none. */
  anki_our_decks_only: boolean;
  theme: ThemeSettings;
  theme_presets: NamedTheme[];
  animations: boolean;
  animation_ms: number;
  /** Vocabulary's share of the width when they are side by side, 0–1. */
  panels_ratio: number;
  /** The kasumi band's opacity; 0 is off. */
  decor_cloud: number;
  decor_cloud_tint: DecorTint;
  /**
   * Whether the clouds are drawn at all. Separate from the opacity so switching them off keeps
   * the strength somebody tuned. Not part of a palette's `ThemeDecor`: whether you want them is
   * yours, how they look is the palette's.
   */
  decor_cloud_on: boolean;
  /** The sakura in the rail, the same way. */
  decor_sakura: number;
  decor_sakura_tint: DecorTint;
  decor_sakura_on: boolean;
  /** One multiplier over both motifs' size. */
  decor_scale: number;
  /** How far the cloud band stops above the footer's rule, in `rem`. */
  decor_cloud_pad: number;
  /**
   * How far apart the two patterns' motifs sit, as a multiplier over their placements.
   *
   * Not CSS custom properties, unlike every other decor value: these multiply coordinates
   * *inside* the SVGs, which no custom property can reach, so they go to `Kasumi` and
   * `Sakura` as props. Clamped to 0.5–2.5 by `Settings::normalise` in Rust.
   */
  decor_cloud_spread: number;
  decor_sakura_spread: number;
  /**
   * Soften both motifs, and by how much in pixels on the display.
   *
   * **A switch as well as two sliders, which the opacities above deliberately do without.** The
   * difference is that `filter: blur(0px)` is not the same as no filter — a non-`none` value
   * promotes the element to its own composited layer whatever the radius — so zero on the slider
   * would still cost something. `App.svelte` writes `none` rather than `blur(0px)` for off.
   *
   * Pixels, and not multiplied by `decor_scale`: what the slider prints is what lands on screen.
   * The two ranges differ (0–8 and 0–3) because a cloud bar is a solid fill and a blossom is a
   * 1.4px outline, and blur past a few pixels erases the second rather than softening it.
   */
  decor_blur: boolean;
  decor_cloud_blur: number;
  decor_sakura_blur: number;
  /** Lay the passage out with the capture's own line breaks rather than as one paragraph. */
  keep_line_breaks: boolean;
  /** Where each line of the passage starts in the reader. Left by default. */
  passage_align: PassageAlign;
  /**
   * Draw the dotted rule under a reading the app is not sure of.
   *
   * Off by default: even at 12.3% of kanji-bearing tokens it reads as clutter on a page
   * somebody is trying to read, so the caveat is offered rather than imposed.
   */
  mark_reading_guesses: boolean;
  /**
   * Offer a grouped word's parts under the selected word.
   *
   * Off by default: grouping exists so the whole word is the easy lookup, and a row of buttons
   * under every selection sits in front of the definitions. It takes nothing away — the parts
   * keep their own status either way. Offered in the quick panel, beside the guess mark.
   */
  show_word_parts: boolean;
  /**
   * Put every captured passage on the clipboard as soon as it is recognised.
   *
   * Off by default — it takes over the clipboard, which belongs to whatever else is running.
   */
  copy_capture_to_clipboard: boolean;
  /** How the glossary of grammar terms is ordered. Mirrors `store::GlossarySort`. */
  glossary_sort: GlossarySort;
  /** Wide-screen preview presentation for Reference. */
  reference_layout: BrowseLayout;
  /** Wide-screen preview presentation for Glossary, independently chosen. */
  /**
   * The reference page's open tabs, by query, left to right.
   *
   * The questions, not the answers — a restored tab re-runs its search, and only once it
   * is selected. Trimmed to 12 and its index clamped by `Settings::normalise` in Rust, so
   * this is safe to index with `reference_tab` without checking.
   */
  reference_tabs: string[];
  /** Which of them is in front. Always a valid index, or 0 when the list is empty. */
  reference_tab: number;
  /**
   * Name the dictionary each definition came from, on the analysis page.
   *
   * Off by default, and not a licence question: attribution is carried by the footer credit.
   * This chip answers "where did *this* one come from", which most reading does not ask.
   */
  source_on_analysis: boolean;
  /** The same, on the reference page — separate, because eighteen results repeat it. */
  source_on_reference: boolean;
  /**
   * A wash over the capture region. `0` is off.
   *
   * Raising it forces `hide_during_capture` on, in the store: a tint the frame is still
   * wearing when the pixels are taken is a tint in the OCR. That costs a flicker per read.
   */
  frame_tint: number;
  /**
   * Let clicks reach whatever is behind the capture frame.
   *
   * The frame's chrome stays solid, so it can still be moved, resized and closed — only the
   * part you look through stops taking the pointer.
   */
  frame_clickthrough: boolean;
  /**
   * Keep the window above every other one.
   *
   * For reading alongside what is being read. The capture frame is always on top already —
   * it is a window you look *through* — and this is the same wish for the window holding
   * the analysis.
   */
  always_on_top: boolean;
  /**
   * Keep the installation panel off the main screen.
   *
   * The panel overrides this whenever it has something to say, so it cannot hide a
   * broken install. See `Setup.svelte`.
   */
  hide_setup: boolean;
};

/** How a binding reads in the UI. */
export function bindingLabel(bind: Binding): string {
  if (bind.kind === "mouse") {
    return {
      left: "Left click",
      middle: "Middle click",
      right: "Right click",
      x1: "Mouse 4 (back)",
      x2: "Mouse 5 (forward)",
    }[bind.button];
  }
  return bind.chord;
}

/**
 * How a recognition engine reads in the UI.
 *
 * **Keyed by what Rust reports, which is not what the selector is keyed by.** `ocr_engine` and
 * the `ocr-engine` event speak `auto` / `manga-ocr` / `pp-ocrv6` / `vlm` / `stub`;
 * `OcrSelect`'s options speak `OcrChoice` — `paddle`, not `pp-ocrv6`. Reusing that map here
 * would look right and be wrong for two of five engines.
 *
 * The selector's own label for the VLM is "PaddleOCR-VL (local server)". The parenthetical is
 * the selector explaining what choosing it entails; it is not part of the name, and it does not
 * belong in a 0.7rem hint.
 *
 * An unknown string comes back unchanged rather than becoming "unknown": a name this does not
 * recognise is a new engine somebody added, and showing it is more useful than hiding it.
 */
export function engineLabel(engine: string): string {
  return (
    {
      auto: "Auto",
      "manga-ocr": "manga-ocr",
      "pp-ocrv6": "PP-OCRv6",
      vlm: "PaddleOCR-VL",
      stub: "no model installed",
    }[engine] ?? engine
  );
}


// ---------------------------------------------------------------------------
// Dictionaries (sakatsu_core::dict)
// ---------------------------------------------------------------------------

/**
 * Which page is on screen.
 *
 * A UI concern with no Rust counterpart — it lives here because the rail and App both
 * need it and neither owns the other.
 *
 * `help` is one of these rather than a drawer, so the rail's rule still holds: exactly one is
 * current, and the one you are not in is the way out of the one you are. That is what lets none
 * of them carry a back button.
 *
 * `settings` joined them for the same reason, and it used to be the exception — a drawer that slid
 * over the reader with a draggable width. As a page it is one of the set, and the two settings the
 * drawer needed to remember (which sections were folded, how wide it had been dragged) stopped
 * meaning anything.
 *
 * **Not every view is in the rail.** `settings` and `help` are reached from the title bar's menu,
 * `about` from the rail's `?`. The rail shows the three views that are about *Japanese*; the menu
 * holds the ones that are about the app.
 */
export type View = "read" | "reference" | "glossary" | "help" | "settings" | "about";

/**
 * The settings sections, in the order they are offered.
 *
 * **One list, and it is the only one.** Three places read it — the title bar's dropdown, the nav
 * down the left of the settings page, and the search that lists what it found under the section
 * holding it — so a section cannot exist in one and be missing from another, which is where a
 * hand-written menu beside a hand-written page arrives within a week.
 *
 * Here rather than in `Settings.svelte` because `Caption.svelte` builds the dropdown from it, and a
 * value exported from a component's instance script is not something another component can import.
 *
 * **Advanced is gone rather than empty.** It held the analyzer and the frequency-number
 * direction, and both moved to the sections that use them — an analyzer is how a passage is
 * read, and how to read a frequency number belongs with the ranking that prints it. What was
 * left was a nav entry leading to nothing.
 *
 * The ids are what `Settings.settings_section` stores, so **renaming one loses which section a
 * reader was last on** — not fatal, since an unrecognised id falls back to showing the nav with
 * nothing selected, but not free either.
 */
export const SETTINGS_SECTIONS = [
  // The id stays `reading` while the label says General: the id is what `settings_section`
  // stores, so renaming it would lose which section every existing reader was last on.
  { id: "reading", label: "General" },
  { id: "appearance", label: "Appearance" },
  { id: "capture", label: "Capture" },
  { id: "search", label: "Search" },
  { id: "recognition", label: "Recognition" },
  { id: "anki", label: "Anki" },
  { id: "diagnostics", label: "Diagnostics" },
  { id: "updates", label: "Updates" },
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]["id"];

/**
 * One line on what each settings section is for.
 *
 * **Read in two places, written in one.** The settings page prints it under the section's title,
 * and Help lists every section with it beside an Open button — so the two cannot describe a
 * section differently. A `Record` over the ids rather than a field on the entries above, for two
 * reasons: svelte-check rejects a section added without one, and `settings-index.test.mjs` parses
 * those entries as `{ id, label }` literals, which a third field would break.
 *
 * A sentence, not a list of every row. The rows have their own lines; this says why you would
 * open the section at all.
 */
export const SETTINGS_ABOUT: Record<SettingsSectionId, string> = {
  reading: "How the passage behaves, how large things are, and what reads your Japanese.",
  appearance: "Palette, light or dark, the colours words wear, and the decoration around them.",
  capture: "The shortcuts that read the screen, and how the capture frame behaves.",
  search: "Dictionary upkeep, how results are ranked, and what each result shows.",
  recognition: "The engines that read text off the screen, and the files they need.",
  anki: "Which deck your cards go to, and the decks this app made.",
  diagnostics: "Records that help when something goes wrong, and a restart for when it has.",
  updates: "Check for new versions and choose when to install them.",
};

export type KanjiStroke = { path: string };
export type StrokeNumber = { number: number; x: number; y: number };
export type KanjiDiagram = { strokes: KanjiStroke[]; numbers: StrokeNumber[] };

export type RelatedKanji = {
  character: string;
  meanings: string[];
  strokes: number;
  grade: number | null;
};

export type RelatedKanjiResult = {
  component: string;
  total: number;
  characters: RelatedKanji[];
};

/** One word written with a character. Mirrors `commands::CompoundWord`. */
export type CompoundWord = {
  headword: string;
  reading: string;
  glosses: string[];
};

/** The commonest words written with a character. Mirrors `commands::CompoundsResult`. */
export type CompoundsResult = {
  character: string;
  /** Every entry written with it, not the number returned — so a page can say "6 of 1369". */
  total: number;
  words: CompoundWord[];
  /**
   * False when no installed dictionary carries the bucket. An empty list then means "not indexed"
   * rather than "nothing uses this character", and the page has to say which — the same
   * distinction `needs_gloss_index` draws for English search.
   */
  indexed: boolean;
};

/** A dictionary's identity — `attribution` is a licence obligation, not a nicety. */
export type DictionaryMeta = {
  title: string;
  revision: string;
  format: number;
  author: string | null;
  url: string | null;
  description: string | null;
  attribution: string | null;
  entry_count: number;
};
