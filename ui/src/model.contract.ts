/**
 * The IPC contract guard. Nothing imports this file — it exists so that
 * `npm run check` (svelte-check) fails when `model.ts` stops describing what the
 * Rust core actually sends.
 *
 * `model.golden.json` is generated from the real `sakatsu_core::model` types by a
 * test in src-tauri (`the_serialized_model_matches_the_golden_the_ui_checks`), so
 * it cannot lie about the wire format. The two assertions below close both
 * directions of drift:
 *
 *   1. A field RENAMED or REMOVED in Rust → the golden loses it → assigning the
 *      golden to `Analysis` fails with "missing property <name>".
 *   2. A field ADDED in Rust → the golden gains it → `Untyped` resolves to the
 *      new key's name instead of `never`, and the error names the exact field
 *      you still have to add to `model.ts`.
 *
 * If either fails: run `cargo test -p sakatsu-desktop` and follow its message to
 * regenerate the golden, then add or rename the fields it reports here.
 */

import golden from "./model.golden.json";
import lookupGolden from "./lookup.golden.json";
import setupGolden from "./setup.golden.json";
import type {
  AlsoIn,
  Alternative,
  Analysis,
  Breakdown,
  Conjugation,
  Construction,
  ContextualWordView,
  DictExample,
  Execution,
  Fallback,
  RepairNote,
  Match,
  MergedSense,
  NameHit,
  Prop,
  Ruby,
  Sense,
  SentenceRange,
  Step,
  StudyView,
  Token,
  VocabularyRanking,
  VocabularySenseRank,
  WordView,
} from "./model";

/**
 * The wire token, with its one enum widened back to `string`.
 *
 * `line_break` is the first enum to cross this boundary, and TypeScript widens a JSON import's
 * string literals — so the golden offers `string` where `model.ts` declares
 * `"none" | "wrap" | "break"`, and the assignment below would fail for a reason that has nothing
 * to do with drift. Widening that one field keeps the check honest about every other: a field
 * renamed or removed in Rust still fails here, which is what direction 1 is for.
 *
 * `components` is restated because it is recursive and would otherwise carry the narrow token.
 */
type WireConstruction = Omit<Construction, "identity" | "lookup" | "grammar"> & {
  identity: string;
  lookup: string;
  grammar: string;
};

type WireToken = Omit<Token, "line_break" | "components" | "constructions" | "lexical_role"> & {
  line_break: string;
  lexical_role: string | null;
  components: WireToken[];
  constructions: WireConstruction[];
};

/**
 * The wire study view, with the two enums widened for the same reason `WireToken` widens one.
 *
 * `statuses` is `WordStatus[]` in `model.ts` and a JSON import offers `string[]`; widening it
 * keeps this check honest about every other field, which is what direction 1 is for. What stops
 * the widening from hiding a real rename is the Rust side: `ipc.rs` asserts that every variant of
 * both enums actually reaches the golden, so a variant renamed in Rust changes these strings and
 * fails the golden comparison before it ever gets here.
 */
/**
 * The wire execution record, its three enums widened for the same reason. The Rust side keeps it
 * honest: `typescript_contract.rs` pins every variant of each in the generated declarations.
 */
type WireFallback = Omit<Fallback, "reason" | "scope"> & { reason: string; scope: string };
type WireRepairNote = Omit<RepairNote, "family" | "stem"> & { family: string; stem: string };
type WireExecution = Omit<Execution, "requested" | "fallback" | "repairs"> & {
  requested: string;
  fallback: WireFallback | null;
  repairs: WireRepairNote[];
};
type WireAnalysis = Omit<Analysis, "tokens" | "execution"> & {
  tokens: WireToken[];
  execution: WireExecution | null;
};

type WireStudyView = Omit<StudyView, "analysis" | "statuses" | "breakdowns"> & {
  analysis: WireAnalysis;
  statuses: string[];
  breakdowns: WireBreakdown[];
};

/**
 * The wire breakdown, with `Step.form` widened back to `string`.
 *
 * The third enum to cross this boundary and the same widening `line_break` and `statuses` get:
 * `FormId` is a literal union in `model.ts` and a JSON import offers `string`. The Rust side is
 * what keeps this honest — a renamed variant changes the golden and fails there first.
 */
type WireBreakdown = Omit<Breakdown, "steps"> & {
  steps: (Omit<Step, "form"> & { form: string })[];
};

// Direction 1 — the wire format satisfies the types the UI programs against.
//
// The root is the whole `StudyView` now, not just its `analysis`. It covers `Analysis` as before
// and additionally `SentenceRange`, `Breakdown`, `Match`, `WordStatus` and `Ruby`, whose
// declarations below this line were previously checked against nothing at all.
export const goldenStudyView: WireStudyView = golden;
export const goldenAnalysis: WireAnalysis = golden.analysis;

/** Keys present on the wire (`G`) that the UI's type (`T`) does not declare. */
// Distribute over JSON array members: keyof a union alone sees only shared keys.
type Untyped<G, T> = G extends unknown ? Exclude<keyof G, keyof T> : never;
// @ts-expect-error A field emitted by only one case must still fail completeness.
export type UnionFieldRegression = NoUntypedFields<Untyped<{ kept: string } | { kept: string; added: boolean }, { kept: string }>>;

/** Fails to compile unless `T` is `never`, naming the offending key if not. */
type NoUntypedFields<T extends never> = T;

type GoldenStudyView = typeof golden;
type GoldenAnalysis = GoldenStudyView["analysis"];
type GoldenToken = GoldenAnalysis["tokens"][number];
type GoldenSentenceRange = GoldenStudyView["sentences"][number];
type GoldenBreakdown = GoldenStudyView["breakdowns"][number];
type GoldenMatch = GoldenStudyView["patterns"][number];
type GoldenRuby = GoldenStudyView["ruby"][number][number];
type GoldenSense = GoldenToken["senses"][number];
/**
 * `NonNullable`, because the golden now contains a token with no conjugation at all.
 *
 * That token is deliberate — an `Option` that is only ever `Some` leaves `null` untested — and it
 * makes the union nullable here. The completeness check below is about the *fields of a
 * conjugation*, so it has to look at the one that exists.
 */
type GoldenConjugation = NonNullable<GoldenToken["conjugation"]>;
type GoldenProp = GoldenConjugation["props"][number];
type GoldenAlternative = GoldenToken["alternatives"][number];
type GoldenConstruction = GoldenToken["constructions"][number];
type GoldenExecution = NonNullable<GoldenAnalysis["execution"]>;
type GoldenFallback = NonNullable<GoldenExecution["fallback"]>;
type GoldenRepairNote = GoldenExecution["repairs"][number];

// Direction 2 — every field the core emits is typed in model.ts.
export type AnalysisIsComplete = NoUntypedFields<Untyped<GoldenAnalysis, Analysis>>;
export type TokenIsComplete = NoUntypedFields<Untyped<GoldenToken, Token>>;
export type SenseIsComplete = NoUntypedFields<Untyped<GoldenSense, Sense>>;
export type ConjugationIsComplete = NoUntypedFields<Untyped<GoldenConjugation, Conjugation>>;
export type PropIsComplete = NoUntypedFields<Untyped<GoldenProp, Prop>>;
export type AlternativeIsComplete = NoUntypedFields<Untyped<GoldenAlternative, Alternative>>;
export type ConstructionIsComplete = NoUntypedFields<Untyped<GoldenConstruction, Construction>>;
export type StudyViewIsComplete = NoUntypedFields<Untyped<GoldenStudyView, StudyView>>;
export type SentenceRangeIsComplete = NoUntypedFields<Untyped<GoldenSentenceRange, SentenceRange>>;
export type BreakdownIsComplete = NoUntypedFields<Untyped<GoldenBreakdown, Breakdown>>;
export type MatchIsComplete = NoUntypedFields<Untyped<GoldenMatch, Match>>;
export type RubyIsComplete = NoUntypedFields<Untyped<GoldenRuby, Ruby>>;
export type ExecutionIsComplete = NoUntypedFields<Untyped<GoldenExecution, Execution>>;
export type FallbackIsComplete = NoUntypedFields<Untyped<GoldenFallback, Fallback>>;
export type RepairNoteIsComplete = NoUntypedFields<Untyped<GoldenRepairNote, RepairNote>>;

// ---------------------------------------------------------------------------
// The lookup contract
//
// `lookup.golden.json` holds three serialized `ContextualWordView`s — an ordinary word, a name,
// and an unknown katakana word — because one value cannot show an `Option` in both states and
// `WordView` has four of them. `src-tauri/tests/ipc.rs` asserts that coverage rather than trusting
// it, so a fixture that drifts into showing fewer fails there before it silently weakens this.
//
// Both directions are checked exactly as they are for the study view above.
// ---------------------------------------------------------------------------

/**
 * **Each case on its own, never the union of them.**
 *
 * This began as `lookupGolden[keyof typeof lookupGolden]`, and that silently disabled half the
 * check: `keyof` a union is the *intersection* of its members' keys, so a field present on only
 * one case was invisible to the completeness test below. Demonstrated rather than reasoned about
 * — adding `"dialect"` to the ordinary case alone passed, while adding it after this change fails
 * and names it.
 */
type GoldenOrdinary = (typeof lookupGolden)["ordinary"];
type GoldenNamed = (typeof lookupGolden)["name"];
type GoldenUnknown = (typeof lookupGolden)["unknown_katakana"];

/**
 * The wire word, with the one field a JSON import cannot carry precisely.
 *
 * `also_in[].senses` and `inline` are the same `MergedSense` type, and TypeScript infers the
 * *narrowest* array type for each JSON literal — so a case whose `inline` is `[]` gives
 * `never[]`, which is assignable, while one whose senses all happen to have a `sequence` gives a
 * non-nullable `number`. Restating the two collections keeps the assignment about field names and
 * shapes rather than about which values these three examples happened to contain.
 */
// JSON imports widen tagged-union literals to string. The generated declaration and Rust
// golden tests pin the variants; this wrapper checks all the remaining wire fields.
type WireRetrieved = { kind: string; query?: string };
type WireWordView = Omit<WordView, "inline" | "inline_retrieved" | "also_in" | "examples" | "name"> & {
  inline: (Omit<MergedSense, "sequence"> & { sequence: number | null })[];
  inline_retrieved: WireRetrieved;
  also_in: { title: string; retrieved: WireRetrieved; senses: (Omit<MergedSense, "sequence"> & { sequence: number | null })[] }[];
  examples: (Omit<DictExample, "retrieved"> & { retrieved: WireRetrieved })[];
  name: (Omit<NameHit, "retrieved"> & { retrieved: WireRetrieved }) | null;
};

type WireContextualWordView = Omit<ContextualWordView, "word"> & { word: WireWordView };


// Direction 1 — the wire format satisfies the types the UI programs against.
export const goldenOrdinaryWord: WireContextualWordView = lookupGolden.ordinary;
export const goldenNamedWord: WireContextualWordView = lookupGolden.name;
export const goldenUnknownWord: WireContextualWordView = lookupGolden.unknown_katakana;

// From the one case that has them; an empty array in the others would type as `never`.
type GoldenMergedSense = GoldenOrdinary["word"]["inline"][number];
type GoldenAlsoIn = GoldenOrdinary["word"]["also_in"][number];
type GoldenDictExample = GoldenOrdinary["word"]["examples"][number];
type GoldenVocabularySenseRank = GoldenOrdinary["ranking"]["senses"][number];
type GoldenNameHit = NonNullable<GoldenNamed["word"]["name"]>;

// Direction 2 — every field the core emits is typed in model.ts, checked per case.
export type OrdinaryIsComplete = NoUntypedFields<Untyped<GoldenOrdinary, ContextualWordView>>;
export type NamedIsComplete = NoUntypedFields<Untyped<GoldenNamed, ContextualWordView>>;
export type UnknownIsComplete = NoUntypedFields<Untyped<GoldenUnknown, ContextualWordView>>;
export type OrdinaryWordIsComplete = NoUntypedFields<Untyped<GoldenOrdinary["word"], WordView>>;
export type NamedWordIsComplete = NoUntypedFields<Untyped<GoldenNamed["word"], WordView>>;
export type UnknownWordIsComplete = NoUntypedFields<Untyped<GoldenUnknown["word"], WordView>>;
export type OrdinaryRankingIsComplete = NoUntypedFields<
  Untyped<GoldenOrdinary["ranking"], VocabularyRanking>
>;
export type NamedRankingIsComplete = NoUntypedFields<
  Untyped<GoldenNamed["ranking"], VocabularyRanking>
>;
export type MergedSenseIsComplete = NoUntypedFields<Untyped<GoldenMergedSense, MergedSense>>;
export type AlsoInIsComplete = NoUntypedFields<Untyped<GoldenAlsoIn, AlsoIn>>;
export type DictExampleIsComplete = NoUntypedFields<Untyped<GoldenDictExample, DictExample>>;
export type NameHitIsComplete = NoUntypedFields<Untyped<GoldenNameHit, NameHit>>;
export type VocabularySenseRankIsComplete = NoUntypedFields<
  Untyped<GoldenVocabularySenseRank, VocabularySenseRank>
>;


// A flattened search hit must keep both its lookup fields and its own enrichment.
import searchGolden from "./search.golden.json";
import type { Found, WordHit, KanjiView, CharReading, Form } from "./model";
type WireWordHit = Omit<WordHit, keyof WordView> & WireWordView;
type WireFound = Omit<Found, "words"> & { words: WireWordHit[] };
export const goldenSearch: WireFound = searchGolden;
export type SearchIsComplete = NoUntypedFields<Untyped<typeof searchGolden, Found>>;
export type SearchHitIsComplete = NoUntypedFields<Untyped<(typeof searchGolden)["words"][number], WordHit>>;
export type SearchKanjiIsComplete = NoUntypedFields<Untyped<(typeof searchGolden)["words"][number]["chars"][number], KanjiView>>;
export type SearchReadingIsComplete = NoUntypedFields<Untyped<(typeof searchGolden)["words"][number]["chars"][number]["on"][number], CharReading>>;
export type SearchFormIsComplete = NoUntypedFields<Untyped<(typeof searchGolden)["words"][number]["forms"][number], Form>>;


// ---------------------------------------------------------------------------
// The setup contract
//
// This differs from the study and lookup contracts above in one way that matters: the setup
// declarations are **generated** from the Rust types rather than handwritten in `model.ts`. So
// direction 2 is not about somebody forgetting to add a field by hand — it is about the golden and
// the generated file being regenerated together. Regenerate one and not the other and this fails,
// which is the only place that mismatch shows up: `typescript_contract` compares the bindings with
// the Rust types, and `setup_ipc` compares the bytes with the Rust types, and neither compares the
// two artifacts the frontend actually imports.
// ---------------------------------------------------------------------------

import type {
  FetchProgress,
  SetupComponentState,
  SetupPreflight,
  SetupProgress,
  SetupSnapshot,
  StorageSummary,
} from "./bindings";

/**
 * The wire snapshot, with its one enum widened back to `string`.
 *
 * `SetupPhase` is a literal union in the generated bindings and a JSON import offers `string` —
 * the same widening `line_break` gets, for the same reason. What keeps it honest is the Rust side:
 * the golden carries **every** phase in its own array, so a renamed variant changes those bytes and
 * fails the golden comparison before it reaches here.
 */
type WireSetupSnapshot = Omit<SetupSnapshot, "phase"> & { phase: string };

type GoldenSetup = typeof setupGolden;
type GoldenSnapshot = GoldenSetup["snapshot"];
type GoldenComponent = GoldenSnapshot["components"][number];

export const goldenSetupSnapshot: WireSetupSnapshot = setupGolden.snapshot;
export const goldenSetupPreflight: SetupPreflight = setupGolden.preflight;
export const goldenSetupProgress: SetupProgress = setupGolden.setup_progress;
// All three emissions, because they are one type now and were three payloads before.
export const goldenFetchStarting: FetchProgress = setupGolden.fetch_progress_starting;
export const goldenFetchDuring: FetchProgress = setupGolden.fetch_progress_during;
export const goldenFetchWorking: FetchProgress = setupGolden.fetch_progress_working;

export type SetupSnapshotIsComplete = NoUntypedFields<Untyped<GoldenSnapshot, SetupSnapshot>>;
export type SetupComponentIsComplete = NoUntypedFields<
  Untyped<GoldenComponent, SetupComponentState>
>;
export type StorageSummaryIsComplete = NoUntypedFields<
  Untyped<GoldenSnapshot["storage"], StorageSummary>
>;
export type SetupPreflightIsComplete = NoUntypedFields<
  Untyped<GoldenSetup["preflight"], SetupPreflight>
>;
export type SetupProgressIsComplete = NoUntypedFields<
  Untyped<GoldenSetup["setup_progress"], SetupProgress>
>;
export type FetchProgressIsComplete = NoUntypedFields<
  Untyped<GoldenSetup["fetch_progress_during"], FetchProgress>
>;
