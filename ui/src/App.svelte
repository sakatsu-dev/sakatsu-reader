<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, untrack } from "svelte";
  import type { UpdateStatus } from "./bindings";
  // The model mirrors sakatsu_core and is kept honest by model.contract.ts.
  import { bindingLabel, engineLabel } from "./model";
  import { captureHint } from "./lib/capture-hint.js";
  // Aliased: `study` is already the name of the view this component holds.
  import { study as requestStudy } from "./lib/studyClient";
  import { pronunciation } from "./lib/pronunciation.svelte";
  import { defaultTheme, paint, watchTheme, type Theme } from "./lib/theme-live";
  import AppMenu from "./lib/AppMenu.svelte";
  import Startup from "./lib/Startup.svelte";
  let startupDone = $state(false);
  let startupSettingsFailed = $state(false);
  import { PRESETS, palettesFor } from "./lib/theme.js";
  import type {
    AnalyzerChoice,
    Binding,
    Counts,
    ContextualWordView,
    DictionaryMeta,
    Found,
    GrammarPattern,
    KanjiView,
    LogFile,
    CorpusFile,
    DictionaryUpkeep,
    GlossarySort,
    BrowseLayout,
    OcrChoice,
    ParticleView,
    PassageAlign,
    ExpressionOffer,
    Reading,
    Settings,
    SettingsSectionId,
    StudyView,
    TagExplanation,
    Term,
    Token,
    View,
    WordStatus,
    WordView,
    VocabularyRanking,
    DecorTint,
    DeckInfo,
    NamedTheme,
    SentenceTranslation,
    VariantTranslations,
    WarmupDiagnostics,
    RuntimeStatus,
  } from "./model";
  // The same word/punctuation rule the reader draws with, so the arrow keys stop on
  // exactly the tokens that are clickable.
  import { isWord } from "./lib/japanese";
  import { latestRequest } from "./lib/latest-request.js";
  import { latestLoad } from "./lib/latest-load.js";
  import { requestOwner } from "./lib/request-owner.js";
  import { operationController } from "./lib/operation-controller.js";
  import { expandForContext, resolvePath } from "./lib/component-context.js";
  import { noticeFor } from "./lib/execution-notice.js";
  import { repairNoteFor } from "./lib/repair-note.js";
  import { settingsPersistence } from "./lib/settings-persistence.js";
  import { errorMessage, saveFailure, SETTINGS_NOT_SAVED, STATUS_NOT_SAVED } from "./lib/save-notice.js";
  import { sharedOperation } from "./lib/shared-operation.js";
  import { toggleComparisonSelection } from "./lib/comparison.js";
  import { exportTarget, offerableDecks } from "./lib/decks.js";
  // The tabs themselves live in the store singleton, because the reference page is
  // unmounted on every trip to the reader. Only their queries come through here, on
  // the way to and from disk.
  import { glossary, search as reference } from "./lib/pages.svelte";
  import { GLOSSARY_OPEN } from "./lib/glossary-navigation";
  import Grammar from "./lib/Grammar.svelte";
  import Reference from "./lib/Reference.svelte";
  import FirstRun from "./lib/FirstRun.svelte";
  import Reader from "./lib/Reader.svelte";
  import TranslationCard from "./lib/TranslationCard.svelte";
  import SettingsPanel from "./lib/Settings.svelte";
  import DictionaryUpdateNotice from "./lib/DictionaryUpdateNotice.svelte";
  import Glossary from "./lib/Glossary.svelte";
  import Kasumi from "./lib/Kasumi.svelte";
  import Help from "./lib/Help.svelte";
  import About from "./lib/About.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Caption from "./lib/Caption.svelte";
  import Setup from "./lib/Setup.svelte";
  import { setup } from "./lib/setup-store.svelte";
  import { fetching } from "./lib/fetching.svelte";
  import Split from "./lib/Split.svelte";
  import Vocabulary from "./lib/Vocabulary.svelte";
  import SelectedWordHeader from "./lib/SelectedWordHeader.svelte";
  import { fitPassageRatio, isConstrainedStudy, passageFitContent } from "./lib/analysis-layout.js";
  import { cancelSourceDraft, compactSource, draftSource } from "./lib/source-state.js";
  import { neighboringWordIndices } from "./lib/intent-warmup.js";

  let text = $state("気持ちが伝わってきます。");
  type SourceState =
    | { kind: "draft"; previous: string | null }
    | { kind: "compact"; canonical: string };
  let sourceState = $state<SourceState>(draftSource());
  let sourceField = $state<HTMLTextAreaElement | undefined>();
  let study = $state<StudyView | null>(null);
  let error = $state("");
  let busy = $state(false);

  // Which page the rail is showing. The reader is not torn down when it is not on
  // screen, so coming back lands on the same passage with the same word selected.
  let view = $state<View>("read");

  /// Where Settings was opened from, so leaving it goes back rather than somewhere fixed.
  ///
  /// A page rather than a drawer means there is no "underneath" to return to on its own, and
  /// settings are usually a detour: you are reading, you change one thing, you want the reading
  /// back. Sending everyone to the reader instead would be wrong for the detour that starts in
  /// Reference, which is the other common one.
  ///
  /// Seeded with `read` for the one case that has no previous page: the first run hands straight
  /// to Settings from onboarding.
  let cameFrom = $state<View>("read");
  /// The pages you go to and come back from. Both wear a × and both leave on Escape; neither is
  /// ever what `cameFrom` remembers.
  const DETOURS: View[] = ["settings", "help"];

  /// The one way the view changes, so that arriving at Settings always records the way back.
  ///
  /// There are five doors into Settings — the title bar's gear, its section menu, the rail, the
  /// button first run offers, and Help's list of sections — and an assignment at each is five
  /// chances for one to forget.
  ///
  /// Guarded against a detour recording another detour. Settings could already reach itself — the
  /// section menu can be used while on the page — and Help reaches Settings, so without the check
  /// the way out of one would become the way into the other and `cameFrom` would stop naming a
  /// page you were actually reading.
  function show(next: View) {
    startupDone = true;
    if (DETOURS.includes(next) && !DETOURS.includes(view)) cameFrom = view;
    view = next;
  }

  /// Open Settings on one section. The two doors that name a section — first run's and Help's —
  /// both come through here, so neither can set the section and forget to record the way back.
  function openSettingsAt(section: SettingsSectionId) {
    settingsSection = section;
    show("settings");
  }

  onMount(() => {
    const open = (event: Event) => {
      const id = (event as CustomEvent<string>).detail;
      if (!id) return;
      glossary.selectedId = id;
      show("glossary");
    };
    window.addEventListener(GLOSSARY_OPEN, open);
    return () => {
      window.removeEventListener(GLOSSARY_OPEN, open);
      setup.dispose();
      dictionaryLoader.cancel();
      analysisController.dispose();
      lookupController.dispose();
      particleController.dispose();
      captureController.dispose();
      sentenceTranslationController.dispose();
      passageTranslationController.dispose();
      comparisonController.dispose();
      settingsController.dispose();
      nativeRequests.dispose();
    };
  });

  /// What the way back is called. The rail says these too, in its `aria-label`s.
  const VIEW_LABELS: Record<View, string> = {
    read: "Reading",
    reference: "Reference",
    glossary: "Glossary",
    help: "Help",
    settings: "Settings",
    about: "About",
  };

  let selected = $state<number | null>(null);
  /// Which part of the selected word the panel is describing.
  ///
  /// **Empty is the whole word**, which is the default and stays the default: the passage always
  /// selects a top-level token, because that is what reading is. A part is reached from the panel
  /// and nowhere else.
  ///
  /// A path rather than an index, because a component can have components: 見ているかもしれない
  /// nests, and "the second part of the first part" needs saying. Every action below reads
  /// `activeToken`, so there is one answer to "which word is this about" rather than one per
  /// feature.
  let selectedPath = $state<number[]>([]);
  /// The part's own stored status, fetched when a part is opened. `null` until it arrives.
  let partStatus = $state<WordStatus | null>(null);
  let word = $state<WordView | null>(null);
  let vocabularyRanking = $state<VocabularyRanking | null>(null);
  // Every reading of the selected particle, or null when the selection is not one.
  let particleSenses = $state<Reading | null>(null);
  // The expression entry for a construction around the selection, and the failure separately.
  // **Two fields, not one nullable:** "no offer here" is the ordinary answer and "the dictionary
  // could not be read" is a fault, and a panel that showed nothing for both would hide the fault.
  let expressionOffer = $state<ExpressionOffer | null>(null);
  let expressionFailed = $state(false);
  let lookingUp = $state(false);
  let readerFocusRequest = $state(0);

  type PassageSizing = { kind: "auto" } | { kind: "manual" };
  let passageSizing = $state<PassageSizing>({ kind: "auto" });
  let outerRatio = $state(0.45);
  let analysisWorkspace = $state<HTMLDivElement | undefined>();
  let readingMeasure = $state<HTMLDivElement | undefined>();
  let analysisWidth = $state(0);
  let analysisHeight = $state(0);
  let narrowPanel = $state<"vocabulary" | "grammar">("vocabulary");
  let panelSwitch = $state<HTMLDivElement | undefined>();
  let vocabularyScroll = $state<HTMLDivElement | undefined>();
  let grammarScroll = $state<HTMLDivElement | undefined>();

  let detailOpen = $state({ meanings: false, examples: false, grammar: false });
  const anyDetailOpen = $derived(detailOpen.meanings || detailOpen.examples || detailOpen.grammar);
  function resetDetails() {
    detailOpen.meanings = false;
    detailOpen.examples = false;
    detailOpen.grammar = false;
  }

  function chooseNarrowPanel(panel: "vocabulary" | "grammar", focus = false) {
    narrowPanel = panel;
    if (focus) {
      queueMicrotask(() =>
        panelSwitch?.querySelector<HTMLElement>("[role=tab][aria-selected=true]")?.focus(),
      );
    }
  }

  function onPanelSwitchKey(event: KeyboardEvent) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const panel =
      event.key === "ArrowLeft" || event.key === "Home" ? "vocabulary" : "grammar";
    chooseNarrowPanel(panel, true);
    event.preventDefault();
  }

  let dictionaries = $state<DictionaryMeta[]>([]);
  const dictionaryLoader = latestLoad(
    () => invoke<DictionaryMeta[]>("dictionaries"),
    (found) => (dictionaries = found),
  );
  let counts = $state<Counts>({ seen: 0, known: 0, ignored: 0 });
  let capturing = $state(false);
  let sentenceTranslations = $state<SentenceTranslation[]>([]);
  let sentenceTranslationError = $state("");
  let translatingSentences = $state(false);
  /// The passage the canonical sentence batch belongs to, so reopening does not re-run it.
  let translatedSentences = $state("");
  let contextTranslation = $state("");
  let contextTranslationError = $state("");
  let translatingContext = $state(false);
  /// Whole-passage context is a separate, lazy question and therefore has its own cache key.
  let translatedContext = $state("");
  let translationOpen = $state(false);
  let translationView = $state<"aligned" | "context">("aligned");

  /// The words ticked into the comparison set with Ctrl+click.
  ///
  /// Separate from `selected`, which is the single word the panels describe. A word can be in
  /// both, and the reader draws them differently for exactly that reason.
  let studied = $state<Set<number>>(new Set());
  let carries = $state<VariantTranslations | null>(null);
  let carriesBusy = $state(false);
  let carriesError = $state("");

  /// What the app actually *knows* about the studied words, as against what the model guessed.
  ///
  /// Kept apart from `carries` on purpose, and not merged into one command: these come out of the
  /// authored particle pack and the grammar packs — cited, cue-ranked, written by a person — while
  /// `carries` is a 61M-parameter model's impression. Fetching them separately is also what lets
  /// them render *first*, because they are in memory and the translation is not. The order on
  /// screen is the order of authority.


  const contextCheckAvailable = $derived((study?.sentences.length ?? 0) > 1);
  const comparisonToken = $derived(studied.size === 0 ? null : Math.min(...studied));
  const translationCardMaxHeight = $derived(
    Math.max(96, analysisHeight * Math.max(0, 1 - outerRatio) * 0.4),
  );

  /// Tick a word in or out, then ask again.
  function toggleStudied(index: number) {
    const adding = !studied.has(index);
    const next = toggleComparisonSelection(studied, index, sentences);
    studied = next;
    // **Striking a word also selects it.** The authored particle sense and pack explanation used
    // to be printed beside the passage for each
    // struck word — a stripped-down copy of what Grammar shows, a few centimetres above Grammar
    // showing it. Selecting here points Vocabulary and Grammar at the word just struck, so the
    // cited answer arrives in the panel built for it, with the JLPT level and the examples.
    //
    // Only on the way in. Un-striking a word is not a request to read about it.
    if (adding) select(index);
    // The gesture takes you where its result is. Striking a word and staying on a vocabulary
    // entry would be a feature running where nobody can see it — the failure the disclosure had
    // before it opened itself.
    //
    // A new comparison always returns to the aligned flow so the affected sentence can change in
    // place. Whole-passage context remains an explicit alternate question.
    translationView = "aligned";
    if (next.size > 0) {
      translationOpen = true;
      void translateSentences();
      void askWhatTheyCarry();
    } else {
      resetComparison();
    }
  }

  function clearStudied() {
    resetComparison();
  }

  /// Return the comparison surface to its one valid empty state.
  ///
  /// Cancellation, selection, result, wait and error move together. Clearing only the visible set
  /// used to leave a request with permission to put an old sentence back after Clear or Analyze.
  function resetComparison() {
    comparisonController.cancel();
    studied = new Set();
    carries = null;
    carriesBusy = false;
    carriesError = "";
  }

  /// Translate the sentence, the selection and the remainder.
  ///
  /// Guarded by `latestRequest` like every other lookup here: ticking words on and off faster
  /// than the model answers would otherwise leave the strip showing a comparison for a selection
  /// that is no longer on screen — the same failure the dictionary lookup has a generation for.
  async function askWhatTheyCarry() {
    const analysis = study?.analysis;
    const picked = [...studied].sort((a, b) => a - b);
    if (!analysis || picked.length === 0) {
      resetComparison();
      return;
    }
    const request = comparisonController.begin();
    carries = null;
    try {
      const result = await invoke<VariantTranslations | null>("translate_variants", {
        tokens: analysis.tokens,
        selected: picked,
        request: request.request,
      });
      if (study?.analysis !== analysis) return;
      comparisonController.accept(request, () => (carries = result));
    } catch (thrown) {
      comparisonController.reject(request, thrown);
    } finally {
      comparisonController.finish(request);
    }
  }
  let ankiBusy = $state(false);
  let ankiMessage = $state("");
  let ankiFailed = $state(false);
  /// The deck to add to. Anki's first deck until there is a settings screen.
  let ankiDeck = $state("Default");
  /// Hide decks an export would never go to. On by default — see `Settings.anki_our_decks_only`.
  ///
  /// The initial value has to match Rust's default, because it is what the deck list uses for the
  /// moment between mount and the settings arriving. Starting it `false` would show the whole
  /// collection and then visibly cull it a beat later.
  let ankiOurDecksOnly = $state(true);
  /** The one pronunciation request currently starting, wherever its button is rendered. */
  let audioBusy = $state<string | null>(null);
  /** A miss or failure belongs only beside the word that was asked to speak. */
  let audioNotice = $state<{ key: string; message: string } | null>(null);
  let activeAudio: { player: HTMLAudioElement; url: string } | null = null;
  const audioRequest = latestRequest();
  const ankiRequest = latestRequest();
  const nativeRequests = requestOwner(crypto.randomUUID(), (request) =>
    invoke("cancel_request", { request }),
  );
  const analysisController = operationController({
    operation: "study",
    requests: nativeRequests,
    changed: (state) => {
      busy = state.busy;
      error = state.error;
    },
  });
  const lookupController = operationController({
    operation: "lookup",
    requests: nativeRequests,
    changed: (state) => {
      lookingUp = state.busy;
      if (state.error) error = state.error;
    },
  });
  const particleController = operationController({
    operation: "particle_senses",
    requests: nativeRequests,
  });
  const expressionController = operationController({
    operation: "expression_offer",
    requests: nativeRequests,
  });
  const captureController = operationController({
    operation: "capture",
    requests: nativeRequests,
    changed: (state) => {
      capturing = state.busy;
      error = state.error;
    },
  });
  const sentenceTranslationController = operationController({
    operation: "translate_sentences",
    requests: nativeRequests,
    changed: (state) => {
      translatingSentences = state.busy;
      sentenceTranslationError = state.error;
    },
  });
  const passageTranslationController = operationController({
    operation: "translate_passage",
    requests: nativeRequests,
    changed: (state) => {
      translatingContext = state.busy;
      contextTranslationError = state.error;
    },
  });
  const comparisonController = operationController({
    operation: "comparison",
    requests: nativeRequests,
    changed: (state) => {
      carriesBusy = state.busy;
      carriesError = state.error;
    },
  });
  /// Which recognition engine is loaded. `"stub"` means none — said plainly rather
  /// than letting a capture look like it silently failed.
  let ocrEngine = $state("stub");
  /// Whether that engine is loaded *yet*.
  ///
  /// False until the backend reports otherwise. Conventional OCR remains lazy, while a remembered
  /// PaddleOCR-VL choice warms after the window appears. Kept current by the `ocr-engine` event,
  /// which is the only thing that can report it — asking through a command would queue behind the
  /// very load being asked about.
  let ocrReady = $state(false);
  /// Engines and last results, for Settings → Diagnostics. Read when that section opens.
  let runtimeStatus = $state<RuntimeStatus | null>(null);
  /// Whether the last read of it failed — distinct from not having been read yet.
  let runtimeStatusFailed = $state(false);
  /// Which of the idle hint's two faces is showing. Meaningless unless it is cycling.
  let hintFace = $state(0);

  /// Set when word status cannot be saved. Shown, because letting the learner mark
  /// words that silently go nowhere is worse than saying so up front.
  let storeError = $state<string | null>(null);
  let settingsSaveError = $state("");
  /**
   * One line saying the last save did not happen, when the reason is already on screen.
   *
   * A single slot on purpose: three failed marks in a degraded session are one fact, and
   * assigning here replaces rather than accumulates. It sits under the standing warning and
   * says only which action failed — `saveFailure` decides when it is used, and the store
   * warning above carries the reason.
   */
  let saveNotice = $state("");

  // Settings live in the store, so they are loaded before first paint matters and
  // saved when changed. `loaded` guards the initial write-back: without it, applying
  // the loaded values would immediately save them again.
  let furigana = $state(true);
  let markSeenOnFocus = $state(true);
  // Round-trip the legacy preference; Reference and Glossary now always use one click.
  let focusBeforeOpen = $state(true);
  let tabNavigation = $state(false);
  let splitRatio = $state(0.45);
  // Ordering and the two browse presentations are independent preferences. Narrow pages force
  // Rows in CSS without overwriting either saved wide-screen choice.
  let glossarySort = $state<GlossarySort>("grouped");
  let referenceLayout = $state<BrowseLayout>("cards");
  let panelsRatio = $state(0.5);

  /// The reference page's divider — the words' share against the characters.

  /// The footer's height, so the cloud band can stop above its rule rather than run under
  /// it. Measured because the footer wraps on a narrow window, which is exactly when a
  /// guessed offset would be wrong.
  let footerHeight = $state(0);

  /// The decorative watermarks: whether each is shown, and its strength and colour. The
  /// starting values are `Settings::default`, which since 2026-10-19 is the maintainer's own
  /// tuning; the saved settings replace them before anything is painted.
  ///
  /// **A switch and a strength, not a strength alone.** Zero opacity is still nothing drawn, but
  /// switching a decoration off by dragging it to zero lost the strength you had chosen. The
  /// switch keeps it: off paints nothing, on paints exactly what was there.
  let decorCloud = $state(0.02);
  let decorCloudTint = $state<DecorTint>("ink");
  let decorCloudOn = $state(true);
  let decorSakura = $state(0.2);
  let decorSakuraTint = $state<DecorTint>("blossom");
  let decorSakuraOn = $state(true);
  /// One multiplier over both, and how far the band stops above the footer's rule.
  let decorScale = $state(1.5);
  let decorCloudPad = $state(1.5);
  // How far apart the motifs sit, as against `decorScale`, which is how big each is
  // drawn. These reach inside the SVGs, so they travel as props rather than as tokens.
  let decorCloudSpread = $state(0.5);
  let decorSakuraSpread = $state(0.7);
  // Softness, on and at these radii by default since the tuned look became the default.
  let decorBlur = $state(true);
  let decorCloudBlur = $state(3.0);
  let decorSakuraBlur = $state(0.0);
  let keepLineBreaks = $state(false);
  // From the left edge by default, matching `Settings::default`.
  let passageAlign = $state<PassageAlign>("left");
  // Off by default, matching `Settings::default`: at 12.3% of kanji-bearing tokens the caveat
  // reads as clutter on a page somebody is trying to read, so it is offered rather than imposed.
  let markReadingGuesses = $state(false);
  // Off by default, matching `Settings::default`: grouping exists so the whole word is the easy
  // lookup, and a row of part buttons under every selection is clutter in front of the
  // definitions. Reaching a part is the exception, so it is asked for.
  let showWordParts = $state(false);
  let copyCaptureToClipboard = $state(false);
  let settingsLoaded = $state(false);
  // What fires a capture, and where the frame was left. Both persist: a frame that
  // reopens somewhere else every session is not persistent in the way that matters.
  let captureBind = $state<Binding>({ kind: "key", chord: "CmdOrCtrl+Shift+Q" });
  let overlayBind = $state<Binding>({ kind: "key", chord: "CmdOrCtrl+Shift+E" });
  let frameRect = $state<[number, number, number, number] | null>(null);
  // Mirrored, not owned: the frame's own toolbar is what sets this. It has to be held here
  // anyway, because `save_settings` replaces the whole object — a stale value would write
  // the lock back off on the next unrelated change.
  let frameLocked = $state(false);
  let overlayEnabled = $state(true);
  // Applied by Rust, which owns the window; this is only what the button reads.
  let alwaysOnTop = $state(false);
  let showSourceAnalysis = $state(false);
  let showSourceReference = $state(false);
  /// The title bar's measurements, from Rust rather than restated here.
  ///
  /// `titlebar.rs` sizes the hit-test overlay that gives the maximise button its snap-layout
  /// flyout from these exact numbers, so a second copy in CSS would drift and put the flyout
  /// over the wrong button — with nothing failing loudly to say so. Fetched for the same
  /// reason `Frame.svelte` fetches `frame_limits`.
  ///
  /// The fallbacks stand for the instant before the answer lands, and are Windows' own
  /// defaults, so even that instant is not visibly wrong.
  let titlebarHeight = $state(32);
  let captionWidth = $state(46);
  invoke<{ height: number; button_width: number }>("titlebar_metrics")
    .then((got) => {
      titlebarHeight = got.height;
      captionWidth = got.button_width;
    })
    .catch(() => {});

  let frameTint = $state(0);
  let frameClickthrough = $state(false);
  let switchBind = $state<Binding | null>(null);
  let clickDelayMs = $state(250);
  let frequencySource = $state("");
  let frequencyDirection = $state<"rank" | "count">("rank");
  let showFrequencySource = $state(false);
  /// What the backend has actually loaded, so a reload only happens on a real change.
  ///
  /// Set when the settings arrive, because Rust has already read that list by then —
  /// without this the first run of the effect below would re-read it for nothing, and
  /// BCCWJ is a million rows to re-read for nothing.
  let frequencyLoaded = "";
  /// Which settings section is on screen. One page shows one section now, where the
  /// drawer showed all eight and remembered which of them were folded shut.
  let settingsSection = $state("reading");
  /// Swap the frequency list in place when the choice changes.
  ///
  /// It used to be read once at startup, so switching lists saved a value nothing looked
  /// at until the app was reopened — which from outside is indistinguishable from a
  /// setting that does not work: the choice sticks and the order never moves. Reloading
  /// here is why there is no "restart to apply" on that row any more.
  $effect(() => {
    // The separator is written as an escape, not as the byte itself. Literally, it
    // makes this file binary to ripgrep, which then skips it in silence -- every
    // `grep -r` over the interface was quietly missing App.svelte, including mine. It
    // only has to be a character no id or direction can contain.
    const key = `${frequencySource}\u0000${frequencyDirection}`;
    if (!settingsLoaded || key === frequencyLoaded) return;
    frequencyLoaded = key;
    invoke("reload_frequency", {
      source: frequencySource,
      direction: frequencyDirection,
    }).catch((thrown) => (error = String(thrown)));
  });

  /// What analyzer the backend is actually running, so a swap only happens on a real change.
  let analyzerLoaded = "";
  /// Swap the analyzer when the choice changes — the same move as the frequency list above,
  /// and for the same reason.
  ///
  /// This row used to read "changing this needs a restart", printed under a dropdown that had
  /// already changed: the setting saved, the engine did not, and until the app was reopened the
  /// two disagreed with no way to tell from the outside. Nothing required it. The analyzer is
  /// read by every command and written by none, which is the shape a swap is trivial for.
  ///
  /// The passage on screen is re-analysed afterwards when there is one, because the analyzer is
  /// the one setting whose whole effect is on results already displayed. Leaving them would show
  /// Lindera's segmentation under a dropdown saying Ichiran.
  $effect(() => {
    const choice = analyzer;
    if (!settingsLoaded || choice === analyzerLoaded) return;
    analyzerLoaded = choice;
    void swapAnalyzer(choice);
  });

  /// Choose the analyzer again and take what it lands on.
  ///
  /// Two callers, and the second is why this is a function rather than living in the effect:
  /// changing the setting, and finishing Ichiran's guided setup. The second passes the choice
  /// that is *already* selected, which the effect would refuse as no change — but the answer
  /// has genuinely changed, because Ichiran now answers where a moment ago it did not.
  async function swapAnalyzer(choice: AnalyzerChoice) {
    try {
      // Not always what was asked: Automatic falls back to the built-in analyzer when Ichiran
      // does not answer, and the settings row says which it landed on rather than leaving it
      // to be inferred from the segmentation.
      analyzerEngine = await invoke<string>("reload_analyzer", { choice });
      // The one setting whose whole effect is on results already on screen. Left alone, the
      // passage would keep the old engine's segmentation under a dropdown naming the new one.
      if (text.trim()) await analyze();
    } catch (thrown) {
      error = String(thrown);
    }
  }

  /// Whether the capture frame is on screen right now.
  ///
  /// Reported by Rust rather than tracked here, and that is the whole point. The button
  /// is not the only thing that puts the frame up or takes it down — both bindings do, the
  /// frame's own toolbar does, and switching the feature off does — so anything this
  /// window inferred for itself would drift the first time one of those was used. It is
  /// also why there is still no command for *asking*: the answer would be stale before it
  /// arrived. See `FRAME_VISIBLE` in commands.rs.
  let overlayShowing = $state(false);
  let holdMs = $state(500);
  let lockTapMs = $state(400);
  let hideDuringCapture = $state(false);
  let fontSize = $state(15);
  const constrainedStudy = $derived(isConstrainedStudy(analysisWidth, fontSize));
  let readerScale = $state(1);
  let panelScale = $state(1);
  let readClipboard = $state(false);
  let quiet = $state(true);
  let ichiranServer = $state(true);
  let preloadIchiran = $state(true);
  let checkUpdatesOnStartup = $state(true);
  let logging = $state(false);
  let collectSentences = $state(false);
  /// Where the log is and how big it is, refreshed when the settings open rather than
  /// polled — nobody needs to watch a file grow.
  let logFile = $state<LogFile | null>(null);
  let corpusFile = $state<CorpusFile | null>(null);
  let warmupDiagnostics = $state<WarmupDiagnostics | null>(null);
  let analyzer = $state<AnalyzerChoice>("auto");
  /// The engine actually running, as the backend reports it after a swap — `auto` resolves to
  /// one of the others and the reader should be told which.
  let analyzerEngine = $state("");
  let ocr = $state<OcrChoice>("auto");
  // Match Rust's fresh defaults for the brief interval before persisted settings arrive. Keeping
  // the retired Ollama pair here made a failed settings read quietly point at a service Sakatsu
  // no longer manages.
  let vlmEndpoint = $state("http://127.0.0.1:8140");
  let vlmModel = $state("paddleocr-vl-manga");
  // Three flats rather than a nested object, because `bind:` wants a writable target and
  // binding through a property of a `$state` object into a child is more indirection than
  // three booleans deserve. They are reassembled on the way to the store.
  let preprocessTighten = $state(false);
  let preprocessContrast = $state(false);
  let preprocessUpscale = $state(false);
  let animations = $state(true);
  /// The palette, as inputs. `theme.js` derives every colour token from these three.
  ///
  /// `main.ts` has already painted the window from the same saved value before this component
  /// mounted, so nothing here is racing a first paint — this is what keeps it current afterwards.
  let theme = $state<Theme>(defaultTheme());
  /// Palettes the learner saved. The shipped ones live in `theme.js` and are not in here.
  let themePresets = $state<NamedTheme[]>([]);

  /**
   * Take a palette's values as the live look — **one answer for both places that offer the list.**
   *
   * It lived in `Settings.svelte`, and the title bar's menu grew its own shorter copy: the page
   * applied a palette's colours *and* its decoration, the menu applied only the colours, so the
   * same palette gave two different results depending on where you picked it. The state is all
   * here, so the function belongs here too and neither surface can answer differently again.
   *
   * **A copy, never a reference.** Nothing afterwards points back at the palette it came from, so
   * moving a slider changes only what is on screen and a shipped palette can never be edited by
   * accident.
   *
   * Decoration comes with it only when the palette carries some. A shipped palette is a colour
   * statement and carries none, and neither does one saved before palettes kept it — in both cases
   * the watermarks stay exactly where they were, which is better than resetting them to a default
   * nobody chose.
   */
  function applyPalette(name: string) {
    const palette = palettesFor(PRESETS, themePresets).find((entry) => entry.name === name);
    if (!palette) return;
    theme = { ...palette.theme };
    const decor = palette.decor;
    if (!decor) return;
    decorCloud = decor.cloud;
    decorCloudTint = decor.cloud_tint;
    decorSakura = decor.sakura;
    decorSakuraTint = decor.sakura_tint;
    decorScale = decor.scale;
    decorCloudPad = decor.cloud_pad;
    decorCloudSpread = decor.cloud_spread;
    decorSakuraSpread = decor.sakura_spread;
    decorBlur = decor.blur;
    decorCloudBlur = decor.cloud_blur;
    decorSakuraBlur = decor.sakura_blur;
    animations = decor.animations;
    animationMs = decor.animation_ms;
  }
  let animationMs = $state(180);

  /// What the capture hint says. The rules are in `capture-hint.js`, where they can be tested.
  ///
  /// Cycling is gated on the animation setting rather than on `prefers-reduced-motion`
  /// directly, because that setting is already how the rest of the app decides whether to
  /// move — `--anim` is written from it, and the media query is its default rather than an
  /// override. Off, the hint states both halves at once instead of alternating them.
  const hint = $derived(
    captureHint({
      capturing,
      ready: ocrReady,
      engine: engineLabel(ocrEngine),
      binding: bindingLabel(captureBind),
      cycling: animations,
    }),
  );

  /// Turn the idle hint over, slowly.
  ///
  /// A timer rather than an infinite CSS animation, and that is the load-bearing choice: an
  /// infinite animation cannot take `var(--anim)` — zero-length and looping is not stillness
  /// but the same loop running infinitely fast — so it would need a literal duration and its
  /// own reduced-motion guard. Driving finite transitions from here keeps the crossfade on the
  /// app's own motion token, and stopping the timer is what "off" means.
  $effect(() => {
    if (hint.faces.length < 2) return;
    const turn = setInterval(() => (hintFace = hintFace === 0 ? 1 : 0), 4000);
    return () => clearInterval(turn);
  });
  let hideSetup = $state(false);
  // A successful first-run should end with an answer, not make the whole wizard disappear between
  // two frames. Remember only a *blocking* setup journey: healthy existing installs never enter
  // it, and somebody already reading in the explicit text-only fallback is not interrupted later.
  // This is intentionally not reactive. It records a transition that the effect consumes; making
  // it an effect dependency lets dismissing Ready immediately re-run and re-latch the wizard.
  let blockingSetupSeen = false;
  let setupReadyAwaitingDismissal = $state(false);

  $effect(() => {
    const snapshot = setup.snapshot;
    if (!setup.loaded || !snapshot) return;
    if ((!snapshot.required_ready || snapshot.analyzer_setup_pending) && !snapshot.text_only) {
      blockingSetupSeen = true;
    } else if (snapshot.required_ready && blockingSetupSeen) {
      blockingSetupSeen = false;
      setupReadyAwaitingDismissal = true;
    }
  });

  function finishFirstRun() {
    setupReadyAwaitingDismissal = false;
    void setup.refresh();
  }

  async function adoptFirstRunIchiran() {
    const engine = await invoke<string>("reload_analyzer", { choice: "auto" });
    analyzerEngine = engine;
    if (engine !== "ichiran") {
      throw new Error("Ichiran is not answering yet. Continue guided setup, try again, or use Lindera for now.");
    }
    // Publish the setting only after the probe succeeds, without scheduling a second swap.
    analyzerLoaded = "auto";
    analyzer = "auto";
  }

  /// "Use Lindera for now", as it says: the built-in analyzer, selected and saved. The swap and the
  /// save follow from the setting, the same as choosing it in Settings.
  function useLinderaForNow() {
    analyzer = "builtin";
  }

  /// "Use Ichiran now" once guided setup finishes, from Settings or the setup page.
  ///
  /// Re-asks the selected choice, because the answer changed even though the choice did not. A
  /// built-in choice is moved to Auto first: re-asking Built-in would land on Lindera again and
  /// leave the button that said "Use Ichiran now" having done nothing.
  async function adoptIchiran() {
    if (analyzer === "builtin") analyzer = "auto";
    else await swapAnalyzer(analyzer);
  }

  function exploreAfterFirstRun() {
    finishFirstRun();
    openSettingsAt("reading");
  }

  /// First run is a persisted installation state, not a guess based on one dictionary.
  /// Text-only is an explicit fallback and remains associated with resumable setup.
  const needsSetup = $derived(
    setup.loaded
      ? Boolean(
          setupReadyAwaitingDismissal ||
            (setup.snapshot && (!setup.snapshot.required_ready || setup.snapshot.analyzer_setup_pending) && !setup.snapshot.text_only),
        )
      : dictionaries.length === 0,
  );

  const tokens = $derived(study?.analysis.tokens ?? []);
  const sentences = $derived(study?.sentences ?? []);
  const patterns = $derived(study?.patterns ?? []);
  const statuses = $derived(study?.statuses ?? []);
  const ruby = $derived(study?.ruby ?? []);
  const token = $derived<Token | null>(selected === null ? null : (tokens[selected] ?? null));

  /// **The selection, resolved once.** `resolvePath` follows the path through *selectable* parts
  /// only and stops at the first step that is missing or withheld, so a stale path — or one naming
  /// a grammatical form the text does not spell — lands on the deepest word it still names. The
  /// heading, status, audio and lookup all read this, and `selectPart` resolves with the same
  /// function before it asks anything, so no two of them can be about different tokens.
  const resolvedSelection = $derived(resolvePath(token, selectedPath));
  /// **The word every action is about.** The whole word, or the part being inspected.
  const activeToken = $derived<Token | null>(resolvedSelection.token);
  const inspectingPart = $derived(resolvedSelection.path.length > 0);
  /// The parts offered below the heading: the active word's own components.
  ///
  /// **Only the selectable ones**, each with its index in the full list. `components` is the word's
  /// grammatical decomposition and can name forms the text does not spell — いる in 食べてる — so the
  /// row offers what core verified as slices of the source and keeps the index a path needs.
  const activeParts = $derived<{ part: Token; at: number }[]>(
    (activeToken?.components ?? [])
      .map((part, at) => ({ part, at }))
      .filter(({ part }) => part.selectable),
  );
  /// Each ancestor of the active word, outermost first — the trail back to the whole word.
  const partTrail = $derived<{ label: string; path: number[] }[]>(
    resolvedSelection.path.reduce<{ label: string; path: number[] }[]>((trail, _, depth) => {
      const path = resolvedSelection.path.slice(0, depth);
      const at = resolvePath(token, path).token;
      if (at) trail.push({ label: at.surface, path });
      return trail;
    }, []),
  );

  /// **Which analyzer read this passage**, said as loudly as the record warrants — see
  /// `execution-notice.js`. Recorded by core at execution, so it describes this passage and not
  /// whichever engine happens to be current now.
  const engineNotice = $derived(noticeFor(study?.analysis.execution));
  /// Where the selected word came from, when core rejoined it after the engine split it
  /// (`repair-note.js`). The whole word only: a part was never rejoined, it was cut from one.
  const repairNote = $derived(
    inspectingPart ? null : repairNoteFor(study?.analysis.execution, activeToken),
  );

  const breakdown = $derived(selected === null ? null : (study?.breakdowns[selected] ?? null));
  // Only the patterns covering the selected word, so the grammar panel stays about
  // the word in hand rather than the whole sentence.
  const wordPatterns = $derived.by(() => {
    const index = selected;
    if (index === null) return [];
    return patterns.filter(
      (match) => index >= match.first_token && index < match.last_token_end,
    );
  });
  const status = $derived<WordStatus>(
    selected === null
      ? "new"
      : inspectingPart
        ? (partStatus ?? "new")
        : (study?.statuses[selected] ?? "new"),
  );

  // Loaded once: the attribution line is a licence obligation for both shipped
  // dictionaries, not a nicety (project notes §8).
  void dictionaryLoader.run();
  // Which analyzer is actually running. Asked rather than assumed, because Automatic is the
  // default and resolves to one of the others — the settings row would otherwise say
  // "Automatic" and nothing about whether Ichiran answered.
  const readAnalyzerEngine = () =>
    invoke<string>("analyzer_engine")
      .then((engine) => (analyzerEngine = engine))
      .catch(() => {});
  readAnalyzerEngine();

  invoke<Settings>("settings")
    .then((saved) => {
      furigana = saved.furigana;
      markSeenOnFocus = saved.mark_seen_on_focus;
      focusBeforeOpen = saved.focus_before_open;
      tabNavigation = saved.tab_navigation;
      splitRatio = saved.split_ratio;
      outerRatio = saved.split_ratio;
      glossarySort = saved.glossary_sort;
      referenceLayout = saved.reference_layout;
      // The tabs come back with their questions and without their answers. Each runs
      // when it is first brought forward -- restoring six must not fire six searches.
      reference.restore(saved.reference_tabs, saved.reference_tab);
      panelsRatio = saved.panels_ratio;
      decorCloud = saved.decor_cloud;
      decorCloudTint = saved.decor_cloud_tint;
      decorCloudOn = saved.decor_cloud_on;
      decorSakura = saved.decor_sakura;
      decorSakuraTint = saved.decor_sakura_tint;
      decorSakuraOn = saved.decor_sakura_on;
      decorScale = saved.decor_scale;
      decorCloudPad = saved.decor_cloud_pad;
      decorCloudSpread = saved.decor_cloud_spread;
      decorSakuraSpread = saved.decor_sakura_spread;
      decorBlur = saved.decor_blur;
      decorCloudBlur = saved.decor_cloud_blur;
      decorSakuraBlur = saved.decor_sakura_blur;
      keepLineBreaks = saved.keep_line_breaks;
      passageAlign = saved.passage_align;
      markReadingGuesses = saved.mark_reading_guesses;
      showWordParts = saved.show_word_parts;
      copyCaptureToClipboard = saved.copy_capture_to_clipboard;
      captureBind = saved.capture_bind;
      overlayBind = saved.overlay_bind;
      frameRect = saved.frame_rect;
      frameLocked = saved.frame_locked;
      overlayEnabled = saved.overlay_enabled;
      alwaysOnTop = saved.always_on_top;
      showSourceAnalysis = saved.source_on_analysis;
      showSourceReference = saved.source_on_reference;
      frameTint = saved.frame_tint;
      frameClickthrough = saved.frame_clickthrough;
      switchBind = saved.switch_bind;
      clickDelayMs = saved.click_delay_ms;
      frequencySource = saved.frequency_source;
      frequencyDirection = saved.frequency_direction;
      showFrequencySource = saved.show_frequency_source;
      frequencyLoaded = `${saved.frequency_source}\u0000${saved.frequency_direction}`;
      settingsSection = saved.settings_section;
      holdMs = saved.hold_ms;
      lockTapMs = saved.lock_tap_ms;
      hideDuringCapture = saved.hide_during_capture;
      fontSize = saved.font_size;
      readerScale = saved.reader_scale;
      panelScale = saved.panel_scale;
      readClipboard = saved.read_clipboard;
      quiet = saved.quiet;
      ichiranServer = saved.ichiran_server;
      preloadIchiran = saved.preload_ichiran_on_startup;
      checkUpdatesOnStartup = saved.check_updates_on_startup;
      logging = saved.log;
      collectSentences = saved.collect_sentences;
      analyzer = saved.analyzer;
      // **Taken as saved.** This used to rewrite `vlm` to `auto`, from when the engine was in
      // source but not offered, and the comment here claimed Rust normalised the same value so
      // neither boundary depended on the other. Rust stopped doing that when the engine was
      // un-paused and this did not — leaving the selector showing Auto while PaddleOCR-VL did
      // the reading, which is the exact failure the original pairing existed to prevent.
      ocr = saved.ocr;
      vlmEndpoint = saved.vlm_endpoint;
      vlmModel = saved.vlm_model;
      preprocessTighten = saved.preprocess.tighten;
      preprocessContrast = saved.preprocess.contrast;
      preprocessUpscale = saved.preprocess.upscale;
      ankiDeck = saved.anki_deck;
      ankiOurDecksOnly = saved.anki_our_decks_only;
      animations = saved.animations;
      theme = saved.theme;
      themePresets = saved.theme_presets;
      animationMs = saved.animation_ms;
      hideSetup = saved.hide_setup;
    })
    .catch(() => { startupSettingsFailed = true; })
    .finally(() => {
      // Record what is already stored, so applying the loaded values does not
      // immediately save them straight back.
      settingsController.loaded(signature());
      // Rust chose the analyzer from this same value at startup, so the effect above has
      // nothing to do on the first pass — without this it would swap to what is already
      // running, and on Automatic that is a cold WSL probe for an answer already known.
      //
      // **In `finally`, not in the block above, and that is the whole point.** The read is
      // allowed to fail — an unreadable store is a supported state here, which is what
      // `store_error` exists to report — and `catch` swallows it while this still runs. Set
      // only on success, it stayed empty exactly when settings could not be read, and the
      // first thing a reader with a damaged store got was a startup stall. `analyzer` is the
      // saved value by now, or the default if there was none, and Rust fell back to the same
      // default from the same failure.
      analyzerLoaded = analyzer;
      settingsLoaded = true;
    });

  invoke<string | null>("store_error")
    .then((reason) => (storeError = reason))
    .catch(() => {});

  // Both facts together. Readiness used to arrive only by event, which meant a session began
  // assuming the engine was cold — harmless while nothing loaded before a capture, wrong now
  // that choosing PaddleOCR-VL starts loading it at once.
  invoke<{ name: string; ready: boolean }>("ocr_engine")
    .then(({ name, ready }) => {
      ocrEngine = name;
      ocrReady = ready;
    })
    .catch(() => {});

  refreshCounts();
  setup.initialise();
  fetching.refresh();

  // The hotkey is global, so it arrives as an event rather than a click.
  listen("capture-requested", () => capture());
  // The frame reports where it was dragged or resized to, so it reopens there next time.
  listen<{ x: number; y: number; width: number; height: number }>("frame-moved", (event) => {
    const { x, y, width, height } = event.payload;
    frameRect = [x, y, width, height];
  });
  /// Turn click-through on or off, now rather than when the settings next save.
  ///
  /// The command applies it immediately; the state is what gets written down. Those are two
  /// steps because the save is debounced, and a toggle that takes half a second to do
  /// anything reads as broken.
  ///
  /// **The frame no longer reports this back.** Its controls used to live on the overlay,
  /// which meant a second webview writing a setting this one owns; now the button that
  /// changes it and the code that saves it are the same window, and the round trip is gone.
  function toggleFrameClickthrough() {
    frameClickthrough = !frameClickthrough;
    invoke("frame_clickthrough", { on: frameClickthrough }).catch(() => {});
  }
  // The switch hotkey changes `overlay_enabled` in Rust, which is the only thing that ever
  // does. Without this the header button would keep its old colour and report a state the
  // app is no longer in.
  // Which engine, and whether it is up. Both answers change without the interface asking:
  // a cold read loads a model, and saving the settings picks a different one.
  listen<{ name: string; ready: boolean }>("ocr-engine", (event) => {
    ocrEngine = event.payload.name;
    ocrReady = event.payload.ready;
  });
  listen<boolean>("overlay-enabled", (event) => (overlayEnabled = event.payload));
  // Double-tapping the show/hide binding locks the frame, and that is the second thing
  // besides this window that writes the setting. Without hearing it the lock button would go
  // on drawing an open shackle over a frame that no longer travels — and the next debounced
  // save would post the stale value straight back over Rust's.
  listen<boolean>("frame-locked", (event) => (frameLocked = event.payload));
  listen<boolean>("frame-visible", (event) => (overlayShowing = event.payload));
  // Setup adopts the local pronunciation index before this arrives. Reloading the durable
  // answers upgrades earlier remote misses now covered by the pack, so buttons appear in place.
  listen<string>("reloaded", (event) => {
    if (event.payload === "pronunciation") pronunciation.load();
    // Ichiran is probed after the window is up, so the row that names the engine is
    // written before the answer exists — it would otherwise say the built-in one until
    // the settings page was closed and reopened.
    if (event.payload === "analyzer") void readAnalyzerEngine();
    if (event.payload === "dictionaries") {
      void dictionaryLoader.run();
      // The buckets belong to the dictionaries that were just replaced, so the status read
      // before them describes a store that is gone.
      void refreshDictionaryUpkeep();
    }
  });

  /// A line copied while an analysis was already running.
  ///
  /// Kept rather than dropped, and only ever the newest one. Reading with a text hooker
  /// means lines arrive at whatever pace the reader advances, and the one that would go
  /// missing is the one they just moved to — they would have to copy it again to get it
  /// back. Two lines deep is not worth queuing: nobody wants the one before last.
  let pendingClipboard: string | null = null;

  listen<string>("clipboard-text", (event) => {
    if (busy || capturing) {
      pendingClipboard = event.payload;
      return;
    }
    void fromClipboardWatcher(event.payload);
  });

  async function fromClipboardWatcher(line: string) {
    error = "";
    text = line;
    sourceState = draftSource(study?.analysis.text ?? null);
    await analyze();
    await drainPendingClipboard();
  }

  async function drainPendingClipboard() {
    if (busy || capturing) return;
    const next = pendingClipboard;
    pendingClipboard = null;
    if (next !== null && next !== text) await fromClipboardWatcher(next);
  }

  /// Read through the frame. Only that.
  ///
  /// It used to summon the frame when there was none, which meant the capture binding
  /// brought back an overlay that had just been deliberately hidden — and moved it to its
  /// stored position on the way. Putting the overlay up is `toggleOverlay`'s job now, so
  /// this can fail honestly instead: `read_frame` already refuses both when the frame is
  /// hidden and when there is none, and says which.
  async function capture() {
    startupDone = true;
    if (capturing || busy) return;
    const request = captureController.begin();
    let captureFinished = false;
    try {
      const text_ = await invoke<string>("read_frame", { request: request.request });
      if (!captureController.current(request)) return;
      if (!text_.trim()) {
        captureController.reject(request, "Nothing was recognized in the frame.");
        return;
      }
      text = text_;
      // **Before analysis, and never allowed to fail the capture.** What goes over is exactly what
      // was recognised — the point is to hand the passage to something else, so anything this app
      // did to it afterwards would be the wrong text. A clipboard the OS refuses is not a reason
      // to lose a capture, so the failure is swallowed rather than surfaced.
      //
      // `copy_text` rather than the webview clipboard API, so the clipboard *watcher* recognises
      // the app's own write and does not read the passage straight back in as a new capture.
      if (copyCaptureToClipboard) {
        invoke("copy_text", { text: text_ }).catch(() => {});
      }
      sourceState = draftSource(study?.analysis.text ?? null);
      captureController.finish(request);
      captureFinished = true;
      await analyze();
    } catch (thrown) {
      captureController.reject(request, thrown);
    } finally {
      if (!captureFinished) captureController.finish(request);
      await drainPendingClipboard();
    }
  }

  /// Put the overlay on screen, or take it off — create it the first time, unhide it
  /// after that.
  ///
  /// It used to only ever show, because a control *labelled* "Show overlay" that also hid
  /// it would have been lying about what it does. That was true of the label and retired
  /// with it: this is an icon now, and its tooltip says show or hide.
  ///
  /// Which of the two happens is decided in Rust, by `toggle_frame`. There is deliberately
  /// no command for asking whether the frame is visible — `commands.rs` records what
  /// happened the last time there was one — and asking here would mean holding the answer
  /// across an IPC round trip that the binding can show or hide the frame inside of.
  ///
  /// It leaves the frame where it was rather than centring on the pointer the way a tap of
  /// the binding does, because the pointer is on this button.
  /// One gesture per job: a click works the overlay, a double click works the feature.
  ///
  /// The click is deferred a quarter second and cancelled by a double click, because a
  /// double click delivers two `click` events before its `dblclick` — and since the click
  /// now toggles, those two would show *and* hide the overlay before the double click was
  /// even seen. Deferring means the double click is the only thing that happens. A quarter
  /// second before a window appears is not perceptible; a flicker on the way to switching
  /// the feature off is.
  let pendingClick: ReturnType<typeof setTimeout> | undefined;

  function overlayClick() {
    clearTimeout(pendingClick);
    pendingClick = setTimeout(() => {
      // Switched off, the click does nothing at all. Turning the feature back on is the
      // double click's job, and one gesture doing two jobs is what this replaced.
      if (overlayEnabled) toggleOverlay();
    }, clickDelayMs);
  }

  /// Switch the whole feature off, or back on.
  function overlayDoubleClick() {
    clearTimeout(pendingClick);
    overlayEnabled = !overlayEnabled;
    // Off, the window goes with it: an overlay left on screen with both its keys dead is
    // the most confusing state available. On, it does *not* come back — enabling the
    // feature and putting the window up are different things, and the click asks for the
    // second one.
    if (!overlayEnabled) invoke("close_frame").catch(() => {});
  }

  async function toggleOverlay() {
    // Refused here as well as behind the button, because the button is not the only thing
    // that can reach this.
    if (!overlayEnabled) return;
    error = "";
    try {
      // **No rect. The frame knows where it is.**
      //
      // This passed `frameRect`, on the reasoning that the frame should come back where it was
      // left. It does that by itself: hiding a window does not move or resize it. What the rect
      // actually did was overwrite the window's own geometry with this window's remembered copy
      // — and that copy arrives over an event and is written on a debounce, so for a moment
      // after every resize it is the *old* size. Toggle within that moment and the resize was
      // undone. A remembered rect is for building the frame the first time; after that the
      // window is the record.
      await invoke("toggle_frame", { rect: null });
    } catch (thrown) {
      error = String(thrown);
    }
  }

  /// Apply a new capture binding, returning why it was refused, or "" if it took.
  ///
  /// A chord another application already owns cannot be taken. Rather than silently
  /// ending up with nothing bound, the backend puts the old one back and says so.
  /// `null` is refused rather than handled: the recorder only offers Clear where it was
  /// given `clearable`, and this was not. Something must always fire a capture.
  async function rebind(next: Binding | null): Promise<string> {
    if (!next) return "this binding cannot be left unset";
    try {
      await invoke("set_capture_bind", { bind: next });
      captureBind = next;
      return "";
    } catch (thrown) {
      return String(thrown);
    }
  }

  /// Change what shows the frame and hides it while held.
  async function rebindOverlay(next: Binding | null): Promise<string> {
    if (!next) return "this binding cannot be left unset";
    try {
      await invoke("set_overlay_bind", { bind: next });
      overlayBind = next;
      return "";
    } catch (thrown) {
      return String(thrown);
    }
  }

  /// The one binding that may be absent, so `null` is a real value here rather than a
  /// refusal — it clears the hotkey.
  async function rebindSwitch(next: Binding | null): Promise<string> {
    try {
      await invoke("set_switch_bind", { bind: next });
      switchBind = next;
      return "";
    } catch (thrown) {
      return String(thrown);
    }
  }

  // Anki's decks, fetched once. Failure here just means Anki is not running, which is
  // the normal state — so it is not an error worth showing until the learner asks.
  //
  // Kept, rather than only used to pick a default, because the settings panel offers them
  // as a list. An empty list there means Anki is not running, which is worth saying.
  let ankiDecks = $state<string[]>([]);
  /// What is in each deck, which is what says whether we may rename or delete it — and, since
  /// the export button has to know a deck this app made from somebody else's, whether there is
  /// anywhere safe to send a card. Fetched whenever the decks are read, including the first
  /// probe; it costs a couple of local requests per deck, once.
  /// `null` until they have been read once — see `offerableDecks` for why that is not `[]`.
  let ankiDeckInfo = $state<DeckInfo[] | null>(null);

  /// Whether a card can be exported *right now*.
  ///
  /// Two questions, and this used to ask only the first: is Anki answering, **and is there
  /// somewhere safe to put the card**. A button that is live while the chosen deck is stale, or
  /// is one the "only Sakatsu's decks" filter excludes, is a button that files vocabulary
  /// somewhere it does not belong.
  const ankiTarget = $derived(
    exportTarget(ankiDeck, offerableDecks(ankiDecks, ankiDeckInfo, ankiOurDecksOnly)),
  );
  const ankiReady = $derived(ankiDecks.length > 0 && ankiTarget !== null);


  /// Look for Anki, and learn enough about its decks to know whether any of them are ours.
  ///
  /// **The counts are fetched here and not only with the settings open**, because the export
  /// button lives on the reading page and cannot tell a deck this app made from somebody's
  /// twelve-year-old kanji deck without them. It costs a couple of requests per deck against a
  /// local server, once, at the moment Anki is first found.
  function findAnki() {
    void refreshDecks();
  }
  findAnki();

  // What earlier sessions learned about words with no recording, so a button is not offered
  // again for a word already known to be silent. Failure leaves the list empty, which only
  // costs one press to relearn.
  pronunciation.load();

  /// Re-read the decks and their contents after anything that changes them.
  async function refreshDecks() {
    try {
      ankiDecks = await invoke<string[]>("anki_decks");
      ankiDeckInfo = await invoke<DeckInfo[]>("anki_deck_info");
      // **No falling back to the first deck.** Both this and the startup probe used to do that
      // when the chosen deck was missing, and on a collection whose first deck is somebody
      // else's — `All in One Kanji`, alphabetically — it quietly pointed the export at it. That
      // is how vocabulary ends up in a deck that has nothing to do with this app.
      //
      // A deck that is gone leaves nothing chosen. The settings say so, the export button
      // refuses to act, and both are recoverable in one press; spoiling a deck somebody has
      // spent years on is not.
      if (ankiDeck && !ankiDecks.includes(ankiDeck)) ankiDeck = "";
    } catch {
      // **What was known is kept.** This used to set the counts back to `[]`, which reads as
      // "never asked" and reopened the list to every deck in the collection — under the switch
      // that hides the ones that are not ours. The counts are the expensive call, two or three
      // requests a deck, so it is the one that fails, and it fails most often right after a deck
      // action when it is asked again. Stale evidence is better than none: it was true a moment
      // ago, and the next successful read replaces it.
    }
  }

  /// Run a deck change, then re-read. The message is returned for the panel to show.
  ///
  /// Errors come back as text rather than thrown: every one of these is a refusal a learner
  /// needs to read — "that deck holds cards 屠つ did not make" is the feature working, not a
  /// fault — and Rust has already phrased them.
  async function deckAction(
    command: string,
    args: Record<string, string>,
  ): Promise<string | null> {
    try {
      await invoke(command, args);
      await refreshDecks();
      return null;
    } catch (error) {
      await refreshDecks();
      return String(error);
    }
  }

  /// Ask again when the window is focused, and only while Anki has not been found.
  ///
  /// Anki is usually started *after* the reader, and a button that stayed hidden until the
  /// app restarted would look broken rather than accurate. Coming back from starting it is
  /// exactly the moment to look again — and once it has answered there is nothing left to
  /// poll for, so this stops asking. If Anki later closes, the add fails and says so.
  function lookForAnki() {
    // Keyed on Anki having answered, not on there being a target: with no deck of ours yet,
    // `ankiReady` is false forever and this would re-probe on every focus.
    if (ankiDecks.length === 0) findAnki();
  }

  // Plays the recording for a requested word, whether it came from Analysis or Reference.
  //
  // Bytes come over the IPC rather than a URL because the endpoint answers 200 with a
  // placeholder clip for words it has no recording of — Rust checks the content and
  // sends null instead, so nothing here can play the placeholder by accident.
  function stopActiveAudio() {
    audioRequest.cancel();
    if (activeAudio) {
      activeAudio.player.pause();
      URL.revokeObjectURL(activeAudio.url);
      activeAudio = null;
    }
    audioBusy = null;
  }

  async function playWordAudio(expression: string, reading: string, key: string) {
    if (!expression && !reading) return;

    stopActiveAudio();
    const request = audioRequest.begin();
    audioBusy = key;
    audioNotice = null;
    try {
      const bytes = await invoke<number[] | null>("word_audio", { expression, reading });
      if (!audioRequest.isCurrent(request)) return;
      if (!bytes) {
        // Remembered rather than announced. The button now stops being drawn for this word
        // wherever it appears, so the answer to "why is there no sound" is the absence of the
        // control rather than a line of text beside one that does nothing — and no later press
        // spends another request on a word already known to be silent. See
        // `pronunciation.svelte.ts` for why this can only be learned by asking.
        pronunciation.markSilent(expression, reading);
        return;
      }
      const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "audio/mpeg" }));
      const player = new Audio(url);
      // Revoke once it has played, or the object URL leaks for the window's lifetime.
      const release = () => {
        URL.revokeObjectURL(url);
        if (activeAudio?.player === player) activeAudio = null;
      };
      player.addEventListener("ended", release, { once: true });
      player.addEventListener("error", release, { once: true });
      activeAudio = { player, url };
      try {
        await player.play();
      } catch (thrown) {
        release();
        throw thrown;
      }
      if (!audioRequest.isCurrent(request)) stopActiveAudio();
    } catch (thrown) {
      if (audioRequest.isCurrent(request)) audioNotice = { key, message: String(thrown) };
    } finally {
      if (audioRequest.isCurrent(request)) audioBusy = null;
    }
  }

  function playAudio() {
    const expression = word?.headword ?? token?.dict_form ?? token?.surface ?? "";
    const reading = word?.reading ?? token?.lemma_reading ?? "";
    return playWordAudio(expression, reading, "analysis");
  }

  async function addToAnki() {
    if (!word) return;
    const request = ankiRequest.begin();
    // The vocabulary pane groups separated senses by their grammatical classification. A card's
    // first three meanings must be the same first three the learner just read, not the dictionary's
    // pre-presentation sequence.
    const chosenWord = word;
    const chosenTokens = tokens;
    const chosenIndex = selected ?? 0;
    const chosenDeck = ankiDeck;
    ankiBusy = true;
    ankiMessage = "";
    ankiFailed = false;
    audioNotice = null;
    try {
      // The sentence travels with the word: a card without it teaches a definition
      // rather than a language.
      //
      // **The tokens and which one, not a string.** This sent `analysis.text` — the whole
      // passage — under this same comment, so a card made from a paragraph carried the
      // paragraph. Rust picks the sentence out and writes the furigana, because only the tokens
      // know where a sentence ends and only they carry the readings.
      await invoke<number>("add_to_anki", {
        deck: chosenDeck,
        word: chosenWord,
        tokens: chosenTokens,
        index: chosenIndex,
      });
      // Naming the deck is the useful half. It is chosen in a drawer the reader is not
      // looking at and remembered across sessions, so "added" alone leaves the one question
      // a confirmation is for — *where* — unanswered.
      if (ankiRequest.isCurrent(request)) ankiMessage = `added to ${chosenDeck}`;
    } catch (thrown) {
      if (ankiRequest.isCurrent(request)) {
        ankiMessage = String(thrown);
        ankiFailed = true;
      }
    } finally {
      if (ankiRequest.isCurrent(request)) ankiBusy = false;
    }
  }

  /// Translation is an opt-in study card, never a competing reader column and never persisted.
  ///
  /// **Opens on the whole-passage reading whenever there is more than one sentence**, and falls
  /// back to the sentence-aligned flow otherwise. It used to always open aligned and leave the
  /// context reading behind a button, which asked the reader to know when Japanese had dropped
  /// a subject — a judgement they came to the translation to have made for them.
  ///
  /// `core::translate::passage` measured the two against human translations, and on the material
  /// this app is pointed at the whole passage wins: 0.865 against 0.852 where punctuation is
  /// absent, and 0.847 against 0.819 where a line is wrapped. Aligned is ahead only on properly
  /// punctuated prose, and by 0.008. A capture from a manga panel or a game box is the first two.
  ///
  /// The aligned flow stays one button away and stays canonical for everything structural — a
  /// comparison transforms its one affected sentence in place, and both answers cache separately.
  function openTranslation() {
    translationOpen = true;
    if (contextCheckAvailable) {
      translationView = "context";
      void translatePassage();
    } else {
      translationView = "aligned";
      void translateSentences();
    }
    if (studied.size > 0) {
      if (!carries && !carriesBusy) void askWhatTheyCarry();
    }
  }

  function toggleTranslation() {
    if (translationOpen) {
      translationOpen = false;
      // Reopening returns to whichever reading `openTranslation` considers the default for this
      // passage, rather than being pinned to one of them. Neither cached answer is discarded.
      translationView = "aligned";
    }
    else openTranslation();
  }

  function showContext(context: boolean) {
    if (context && contextCheckAvailable) {
      translationView = "context";
      void translatePassage();
    } else {
      translationView = "aligned";
      void translateSentences();
    }
  }

  /// Translate the exact sentence partition Rust sent with the analysis.
  ///
  /// The batch is atomic at IPC: an error leaves no partial flow on screen, but Rust retains any
  /// successful exact-input cache entries so Retry does not repeat completed inference.
  async function translateSentences() {
    const analysis = study?.analysis;
    const passage = analysis?.text ?? "";
    if (!analysis || !passage || passage === translatedSentences || translatingSentences) return;

    const request = sentenceTranslationController.begin();
    sentenceTranslations = [];
    try {
      const result = await invoke<SentenceTranslation[]>("translate_sentences", {
        tokens: analysis.tokens,
        request: request.request,
      });
      if (study?.analysis !== analysis) return;
      sentenceTranslationController.accept(request, () => {
        sentenceTranslations = result;
        translatedSentences = passage;
      });
    } catch (thrown) {
      sentenceTranslationController.reject(request, thrown);
    } finally {
      sentenceTranslationController.finish(request);
    }
  }

  /// Translating the passage as one string is an explicit context check, useful when Japanese
  /// omits a subject. It never competes on screen with the sentence-aligned rendering.
  ///
  /// **Called from its callers rather than driven by an `$effect` on `translationOpen`.** Such an
  /// effect would read request state and then write it, which is the
  /// self-invalidating shape that made the settings page unusable earlier — `refreshDecks` reads
  /// `ankiDecks` and writes it, and never settles. Three call sites and one function cannot loop.
  async function translatePassage() {
    const passage = study?.analysis.text ?? "";
    if (!passage || passage === translatedContext || translatingContext) return;

    const request = passageTranslationController.begin();
    contextTranslation = "";
    try {
      const result = await invoke<string>("translate", { text: passage, request: request.request });
      if (study?.analysis.text !== passage) return;
      passageTranslationController.accept(request, () => {
        contextTranslation = result;
        translatedContext = passage;
      });
    } catch (thrown) {
      passageTranslationController.reject(request, thrown);
    } finally {
      passageTranslationController.finish(request);
    }
  }

  /// A dictionary arrived — from a download or from a zip dropped in the folder.
  ///
  /// Rust has already opened it by the time this runs (`commands::adopt`, before the event
  /// this is hung off). All that is left is the credits list on screen, which was read once
  /// at mount and is now one dictionary out of date.
  ///
  /// This used to set `error` to "Dictionary imported — restart Sakatsu Reader to use it",
  /// which was true when the open set was fixed at startup and is now both false and in the
  /// wrong place: it is not an error, and there is nothing to do about it.
  function dictionariesChanged() {
    void dictionaryLoader.run();
  }

  function refreshCounts() {
    invoke<Counts>("word_counts")
      .then((found) => (counts = found))
      .catch(() => {});
  }

  /// Refresh the log's size whenever the settings open, and after logging is switched.
  ///
  /// Not polled: the number is there to answer "is this recording", which is a question
  /// asked when you look, not continuously.
  $effect(() => {
    if (view !== "settings" && !logging) return;
    invoke<LogFile>("log_file")
      .then((found) => (logFile = found))
      .catch(() => (logFile = null));
    invoke<CorpusFile>("corpus_file")
      .then((found) => (corpusFile = found))
      .catch(() => (corpusFile = null));
  });

  /// Repaint whenever the palette changes, and follow the desktop while the mode is `system`.
  ///
  /// The save path emits to the capture frame; this window is the one doing the saving, so it
  /// paints directly rather than waiting for its own event to come back round.
  $effect(() => {
    void paint(document.documentElement, theme);
  });
  $effect(() => watchTheme(document.documentElement, () => theme));

  /// Ask Anki what decks it has whenever the settings open.
  ///
  /// The Anki section is where "Anki is not running" is shown, so it should be current when you
  /// look at it rather than as stale as the last time the window was focused. This used to hang off
  /// the gear's click; the settings can now be reached from the title bar, from the nav, and from a
  /// search result, so it hangs off *arriving* instead of off one of the ways in.
  $effect(() => {
    if (view === "settings") {
      // The deck panel needs the counts, and they are only worth fetching while Settings is on
      // screen. Do not let the state read inside `refreshDecks` become an effect dependency: an
      // empty Anki result is still a fresh array, and tracking it turns this into an endless probe.
      untrack(() => void refreshDecks());
    }
  });

  /// Read session acceleration counters only where they are explained. They are diagnostics,
  /// not another progress indicator to poll while somebody is reading.
  $effect(() => {
    if (view !== "settings" || settingsSection !== "diagnostics") return;
    invoke<WarmupDiagnostics>("warmup_diagnostics")
      .then((found) => (warmupDiagnostics = found))
      .catch(() => (warmupDiagnostics = null));
    refreshRuntimeStatus();
  });

  /// Read what is running, for Settings > Diagnostics: when the section opens, and after an unload.
  function refreshRuntimeStatus() {
    runtimeStatusFailed = false;
    invoke<RuntimeStatus>("runtime_status")
      .then((found) => (runtimeStatus = found))
      .catch(() => {
        runtimeStatus = null;
        runtimeStatusFailed = true;
      });
  }

  /// How often to look for decks made, renamed or removed in Anki itself.
  ///
  /// Anki offers nothing to subscribe to — AnkiConnect is request/response over loopback — so
  /// noticing an outside change means asking. Two and a half seconds is fast enough that a deck
  /// made in Anki appears here before anybody goes looking for it, and slow enough to be a
  /// rounding error against an add-on that answers in milliseconds.
  const DECK_WATCH_MS = 2_500;

  /// Watch Anki for deck changes, but **only while the panel that shows them is open**.
  ///
  /// The tick is deliberately one call: `deck_names` is a bare list of strings, where the counts
  /// and marks behind `deck_info` cost two or three requests *per deck* and would be an unkind
  /// thing to run every few seconds against somebody's collection. The full read happens only
  /// when that list actually differs — which is what a deck created, renamed or deleted in Anki
  /// looks like from here.
  ///
  /// Cards moved or added in Anki change no name at all, so the cheap tick cannot see them. Every
  /// [`DECK_WATCH_FULL`] ticks the full read runs anyway, which is what keeps "holds 3 cards" and
  /// the enabled buttons honest while somebody works in both windows at once.
  ///
  /// Even then the interface's `managed` is only a hint: Rust re-checks the real gate on every
  /// action, so the worst a stale count can cost is a refusal with a clear message, never a wrong
  /// deletion.
  const DECK_WATCH_FULL = 6;

  $effect(() => {
    if (view !== "settings" || settingsSection !== "anki") return;

    let stop = false;
    // One tick at a time. A slow reply must not let a second request overlap the first and
    // deliver the two answers out of order.
    let checking = false;
    let ticks = 0;

    const timer = setInterval(async () => {
      if (stop || checking) return;
      checking = true;
      ticks += 1;
      try {
        if (ticks % DECK_WATCH_FULL === 0) {
          await refreshDecks();
          return;
        }
        const names = await invoke<string[]>("anki_decks");
        if (stop) return;
        const changed =
          names.length !== ankiDecks.length || names.some((name, at) => name !== ankiDecks[at]);
        // Only when something really moved. An unconditional refresh every tick would reset the
        // deck being managed, and with it any half-typed name.
        if (changed) await refreshDecks();
      } catch {
        // Anki closed, or was never open. The panel already says so.
      } finally {
        checking = false;
      }
    }, DECK_WATCH_MS);

    return () => {
      stop = true;
      clearInterval(timer);
    };
  });

  async function revealLog() {
    try {
      await invoke("reveal_log");
    } catch (thrown) {
      error = String(thrown);
    }
  }

  async function revealCorpus() {
    try {
      await invoke("reveal_corpus");
      // Re-read after opening it: the count is what the row is for, and somebody who just went
      // to look at the file is the likeliest person to have changed it.
      corpusFile = await invoke<CorpusFile>("corpus_file");
    } catch (thrown) {
      error = String(thrown);
    }
  }

  // Reading this inside `signature()` is what makes opening, closing or running a tab
  // count as a settings change -- the effect below subscribes to the tab list through it,
  // so no separate save path is needed for the strip.
  const referenceTabs = $derived(reference.stored());

  const preprocessSettings = $derived({
    tighten: preprocessTighten,
    contrast: preprocessContrast,
    upscale: preprocessUpscale,
  });

  function signature() {
    // The binding is deliberately absent: it is applied and saved by `set_capture_bind`,
    // which has to try registering it before it can be stored. Including it here would
    // write it back a second time, unvalidated.
    return JSON.stringify({
      furigana,
      mark_seen_on_focus: markSeenOnFocus,
      focus_before_open: focusBeforeOpen,
      tab_navigation: tabNavigation,
      split_ratio: splitRatio,
      panels_ratio: panelsRatio,
      decor_cloud: decorCloud,
      decor_cloud_tint: decorCloudTint,
      decor_cloud_on: decorCloudOn,
      decor_sakura: decorSakura,
      decor_sakura_tint: decorSakuraTint,
      decor_sakura_on: decorSakuraOn,
      decor_scale: decorScale,
      decor_cloud_pad: decorCloudPad,
      decor_cloud_spread: decorCloudSpread,
      decor_sakura_spread: decorSakuraSpread,
      decor_blur: decorBlur,
      decor_cloud_blur: decorCloudBlur,
      decor_sakura_blur: decorSakuraBlur,
      glossary_sort: glossarySort,
      reference_layout: referenceLayout,
      reference_tabs: referenceTabs.tabs,
      reference_tab: referenceTabs.active,
      keep_line_breaks: keepLineBreaks,
      passage_align: passageAlign,
      mark_reading_guesses: markReadingGuesses,
      show_word_parts: showWordParts,
      copy_capture_to_clipboard: copyCaptureToClipboard,
      frame_rect: frameRect,
      frame_locked: frameLocked,
      overlay_enabled: overlayEnabled,
      always_on_top: alwaysOnTop,
      source_on_analysis: showSourceAnalysis,
      source_on_reference: showSourceReference,
      frame_tint: frameTint,
      frame_clickthrough: frameClickthrough,
      click_delay_ms: clickDelayMs,
      frequency_source: frequencySource,
      frequency_direction: frequencyDirection,
      show_frequency_source: showFrequencySource,
      settings_section: settingsSection,
      hold_ms: holdMs,
      lock_tap_ms: lockTapMs,
      hide_during_capture: hideDuringCapture,
      font_size: fontSize,
      reader_scale: readerScale,
      panel_scale: panelScale,
      read_clipboard: readClipboard,
      quiet,
      ichiran_server: ichiranServer,
      preload_ichiran_on_startup: preloadIchiran,
      check_updates_on_startup: checkUpdatesOnStartup,
      log: logging,
      collect_sentences: collectSentences,
      analyzer,
      ocr,
      vlm_endpoint: vlmEndpoint,
      vlm_model: vlmModel,
      preprocess: preprocessSettings,
      anki_deck: ankiDeck,
      anki_our_decks_only: ankiOurDecksOnly,
      theme,
      theme_presets: themePresets,
      animations,
      animation_ms: animationMs,
      hide_setup: hideSetup,
    });
  }

  function currentSettings(): Settings {
    return {
      furigana,
      mark_seen_on_focus: markSeenOnFocus,
      focus_before_open: focusBeforeOpen,
      tab_navigation: tabNavigation,
      split_ratio: splitRatio,
      panels_ratio: panelsRatio,
      decor_cloud: decorCloud,
      decor_cloud_tint: decorCloudTint,
      decor_cloud_on: decorCloudOn,
      decor_sakura: decorSakura,
      decor_sakura_tint: decorSakuraTint,
      decor_sakura_on: decorSakuraOn,
      decor_scale: decorScale,
      decor_cloud_pad: decorCloudPad,
      decor_cloud_spread: decorCloudSpread,
      decor_sakura_spread: decorSakuraSpread,
      decor_blur: decorBlur,
      decor_cloud_blur: decorCloudBlur,
      decor_sakura_blur: decorSakuraBlur,
      glossary_sort: glossarySort,
      reference_layout: referenceLayout,
      reference_tabs: referenceTabs.tabs,
      reference_tab: referenceTabs.active,
      keep_line_breaks: keepLineBreaks,
      passage_align: passageAlign,
      mark_reading_guesses: markReadingGuesses,
      show_word_parts: showWordParts,
      copy_capture_to_clipboard: copyCaptureToClipboard,
      capture_bind: captureBind,
      overlay_bind: overlayBind,
      frame_rect: frameRect,
      frame_locked: frameLocked,
      overlay_enabled: overlayEnabled,
      always_on_top: alwaysOnTop,
      source_on_analysis: showSourceAnalysis,
      source_on_reference: showSourceReference,
      frame_tint: frameTint,
      frame_clickthrough: frameClickthrough,
      switch_bind: switchBind,
      click_delay_ms: clickDelayMs,
      frequency_source: frequencySource,
      frequency_direction: frequencyDirection,
      show_frequency_source: showFrequencySource,
      settings_section: settingsSection,
      hold_ms: holdMs,
      lock_tap_ms: lockTapMs,
      hide_during_capture: hideDuringCapture,
      font_size: fontSize,
      reader_scale: readerScale,
      panel_scale: panelScale,
      read_clipboard: readClipboard,
      quiet,
      ichiran_server: ichiranServer,
      preload_ichiran_on_startup: preloadIchiran,
      check_updates_on_startup: checkUpdatesOnStartup,
      log: logging,
      collect_sentences: collectSentences,
      analyzer,
      ocr,
      vlm_endpoint: vlmEndpoint,
      vlm_model: vlmModel,
      preprocess: preprocessSettings,
      anki_deck: ankiDeck,
      anki_our_decks_only: ankiOurDecksOnly,
      theme,
      theme_presets: themePresets,
      animations,
      animation_ms: animationMs,
      hide_setup: hideSetup,
    };
  }

  /// Settings writes are one ordered lifecycle. The controller owns the debounce, the newest
  /// response and the restart barrier; this component only applies values Rust normalised.
  const settingsController = settingsPersistence<Settings>({
    read: currentSettings,
    signature,
    save: (settings) => invoke<Settings>("save_settings", { settings }),
    apply: (kept) => {
      // **What came back, not what went out.** Setting a tint forces hiding the frame during
      // capture on, so the saved response owns those two values.
      hideDuringCapture = kept.hide_during_capture;
      frameTint = kept.frame_tint;
      settingsSaveError = "";
      saveNotice = "";
    },
    failed: (thrown) => {
      // Unprefixed. The message arrives complete from the side that knows what happened.
      const outcome = saveFailure(thrown, SETTINGS_NOT_SAVED);
      if (outcome.notice !== null) {
        saveNotice = outcome.notice;
        settingsSaveError = "";
      } else {
        settingsSaveError = outcome.error ?? "";
      }
    },
    restart: () => invoke("restart_app"),
  });

  async function restartApp() {
    try {
      await settingsController.restart();
    } catch (thrown) {
      error = `Could not restart Sakatsu Reader: ${errorMessage(thrown)}`;
    }
  }

  // Save when a setting actually changes — and only then.
  //
  // Two things this has to avoid: writing the loaded values straight back on
  // startup, and writing once per pointer move while the split is being dragged.
  // Hence the comparison and the debounce.
  $effect(() => {
    const current = signature();
    if (!settingsLoaded) return;
    settingsController.schedule(current);
  });

  // Applied to the document rather than passed down: both are read by CSS everywhere,
  // and threading them through every component would be a prop with no logic attached.
  $effect(() => {
    const root = document.documentElement;
    root.style.fontSize = `${fontSize}px`;
    // Multipliers over the rem sizes each component already declares, so 1.0 is exactly
    // what it looked like before these existed and the interface size still moves
    // everything together.
    root.style.setProperty("--reader-scale", String(readerScale));
    root.style.setProperty("--panel-scale", String(panelScale));
  });
  $effect(() => {
    // Off means zero, so every transition still *runs* — it just takes no time. That is
    // one code path instead of a `.no-animation` class every component has to honour.
    document.documentElement.style.setProperty(
      "--anim",
      animations ? `${animationMs}ms` : "0ms",
    );
  });

  /// The decorative watermarks, for the same reason the sizes above are done this way:
  /// two SVGs read them from CSS and nothing else needs to know.
  ///
  /// The tint maps to a `var()` reference rather than a literal — a custom property may
  /// hold one — so the three colours stay in `theme.css` where the palette lives instead
  /// of being restated here.
  $effect(() => {
    const root = document.documentElement;
    const tint = { ink: "var(--text)", blossom: "var(--red)", accent: "var(--accent)" };
    // A switched-off decoration paints at zero, which is how the motifs have always been
    // turned off; the strength stays in `decorCloud` for when it comes back.
    root.style.setProperty("--decor-cloud", String(decorCloudOn ? decorCloud : 0));
    root.style.setProperty("--decor-cloud-tint", tint[decorCloudTint]);
    root.style.setProperty("--decor-sakura", String(decorSakuraOn ? decorSakura : 0));
    root.style.setProperty("--decor-sakura-tint", tint[decorSakuraTint]);
    root.style.setProperty("--decor-scale", String(decorScale));
    root.style.setProperty("--decor-cloud-pad", `${decorCloudPad}rem`);
    // A whole filter value, so that off is `none` rather than `blur(0px)`. The difference is
    // not cosmetic: any non-`none` filter promotes the element to its own composited layer and
    // re-rasterises it, so `blur(0px)` pays the cost of the effect and shows none of it. The
    // zero check is there for the same reason — dragging a slider to the bottom is also off.
    // And a motif that is switched off costs no filter either: nothing drawn needs no softening.
    const soften = (shown: boolean, radius: number) =>
      shown && decorBlur && radius > 0 ? `blur(${radius}px)` : "none";
    root.style.setProperty("--decor-cloud-blur", soften(decorCloudOn, decorCloudBlur));
    root.style.setProperty("--decor-sakura-blur", soften(decorSakuraOn, decorSakuraBlur));
  });

  /// The identity Rust stores for a word. Kept here only to reconcile every occurrence in
  /// the passage after one write; Rust remains the authority for what was persisted.
  function statusKey(chosen: Token): string {
    return JSON.stringify([chosen.dict_form || chosen.surface, chosen.lemma_reading]);
  }

  /// Writes in flight for each word, and the newest status queued behind them.
  ///
  /// Held outside Svelte's reactive state because this is concurrency bookkeeping, not UI
  /// state. It prevents a held arrow key from queuing the same New → Seen write more than
  /// once, and serialises a right-click arriving behind that promotion so Known wins last.
  const statusWrites = new Map<string, Promise<void>>();
  const pendingStatuses = new Map<string, WordStatus>();

  function effectiveStatus(index: number): WordStatus {
    const chosen = tokens[index];
    if (!chosen) return "new";
    return pendingStatuses.get(statusKey(chosen)) ?? study?.statuses[index] ?? "new";
  }

  function setStatusAt(index: number, next: WordStatus): Promise<void> {
    const chosen = tokens[index];
    if (!chosen || !study) return Promise.resolve();
    return markToken(chosen, next);
  }

  /// Mark one word, whatever its place in the passage.
  ///
  /// **By token, not by index**, because a component of a grouped word has no index of its own.
  /// Everything else is unchanged: the same queue per word, the same trust in what came back, the
  /// same key-rather-than-index rule that makes a late reply safe. A part that happens to spell a
  /// word standing elsewhere in the passage updates that word too — they are the same word, which
  /// is the behaviour whole words already have.
  function markToken(chosen: Token, next: WordStatus): Promise<void> {
    if (!study) return Promise.resolve();

    const key = statusKey(chosen);
    pendingStatuses.set(key, next);
    const previous = statusWrites.get(key) ?? Promise.resolve();

    let write: Promise<void>;
    write = previous
      .then(async () => {
        try {
          const stored = await invoke<WordStatus>("set_word_status", {
            token: chosen,
            status: next,
          });
          // Trust what came back, and update every occurrence of this stored word in the
          // passage that is current when the reply lands. A key, rather than the captured
          // index, is what makes a late reply safe after selection or analysis has moved.
          const current = study;
          if (pendingStatuses.get(key) === next && current) {
            tokens.forEach((candidate, candidateIndex) => {
              if (statusKey(candidate) === key) current.statuses[candidateIndex] = stored;
            });
            // The open part's own status, which no entry in `statuses` covers.
            if (activeToken && statusKey(activeToken) === key) partStatus = stored;
            refreshCounts();
            saveNotice = "";
          }
        } catch (thrown) {
          // A refusal for the reason already showing gets a line; anything else keeps its
          // own words. The status itself is untouched either way — this `catch` is reached
          // instead of the assignment above, so a word never renders as marked on a save
          // that did not happen.
          const outcome = saveFailure(thrown, STATUS_NOT_SAVED);
          if (outcome.notice !== null) saveNotice = outcome.notice;
          else error = outcome.error ?? "";
        }
      })
      .finally(() => {
        if (statusWrites.get(key) === write) {
          statusWrites.delete(key);
          pendingStatuses.delete(key);
        }
      });

    statusWrites.set(key, write);
    return write;
  }

  function promote(index: number) {
    if (markSeenOnFocus && effectiveStatus(index) === "new") void setStatusAt(index, "seen");
  }

  function mark(next: WordStatus) {
    const index = selected;
    if (index === null) return;
    // **The part, when a part is open.** Marking かっこ Known must not mark かっこいい Known, and
    // the two are different keys, so routing by token rather than by index is the whole of it.
    if (inspectingPart && activeToken) {
      void markToken(activeToken, next);
      return;
    }
    void setStatusAt(index, next);
  }

  function markKnown(index: number) {
    void setStatusAt(index, "known");
  }

  function editSource() {
    const canonical = study?.analysis.text ?? text;
    text = canonical;
    sourceState = draftSource(canonical);
    queueMicrotask(() => sourceField?.focus());
  }

  function cancelSourceEdit() {
    const restored = cancelSourceDraft(sourceState);
    if (!restored) return;
    text = restored.text;
    sourceState = restored.source;
  }

  function fitPassage() {
    passageSizing = { kind: "auto" };
  }

  function useManualPassage(ratio: number) {
    passageSizing = { kind: "manual" };
    outerRatio = ratio;
    splitRatio = ratio;
  }

  // A fit is derived from the content the browser actually laid out. The floor may outrank
  // the 40% cap: clipping one Japanese line is never an acceptable way to honour a percentage.
  $effect(() => {
    void study;
    void furigana;
    void readerScale;
    void analysisWidth;
    void analysisHeight;
    if (passageSizing.kind !== "auto" || !study) return;
    const frame = requestAnimationFrame(() => {
      if (!analysisWorkspace || !readingMeasure || passageSizing.kind !== "auto") return;
      const span = analysisWorkspace.clientHeight;
      const reader = readingMeasure.querySelector<HTMLElement>(".reader");
      if (!reader || span <= 0) return;
      const floor = Number.parseFloat(getComputedStyle(reader).minHeight) || 0;
      outerRatio = fitPassageRatio({
        workspace: span,
        content: passageFitContent({ japanese: reader.scrollHeight }),
        floor,
      });
    });
    return () => cancelAnimationFrame(frame);
  });

  /// Whether the passage is actually being cut off at the size it has been given.
  ///
  /// **This is what decides whether "Fit passage" is worth offering, and it is a different
  /// question from the one the button used to ask.** It appeared for as long as the sizing was
  /// manual, so a hand-dragged split that happened to show the whole passage still carried a
  /// button that would change nothing — which reads as though something is wrong when nothing is.
  ///
  /// The measure is the reading pane's own overflow. `.column-body` is the scroller, so content
  /// taller than its box is precisely a passage with a line out of sight; one pixel of tolerance
  /// for subpixel rounding, the same allowance the layout checks use.
  ///
  /// Only asked while the sizing is manual. Under `auto` the ratio is already the fit, and the
  /// button is not offered at all.
  let passageClipped = $state(false);

  /// Measured on the same signals as the auto fit above, plus the ratio itself — dragging the
  /// splitter is the one thing that changes the answer without changing the passage.
  $effect(() => {
    void study;
    void furigana;
    void readerScale;
    void analysisWidth;
    void analysisHeight;
    void outerRatio;
    if (passageSizing.kind !== "manual" || !study) {
      passageClipped = false;
      return;
    }
    // A frame, so the split has been laid out at its new ratio before it is measured.
    const frame = requestAnimationFrame(() => {
      if (!readingMeasure) return;
      passageClipped = readingMeasure.scrollHeight > readingMeasure.clientHeight + 1;
    });
    return () => cancelAnimationFrame(frame);
  });

  /// Put down the analysis, and keep the passage.
  ///
  /// **The text box is deliberately left alone.** What this clears is everything `analyze`
  /// produced — the reader, the selected word, the entry beside it, the particle senses and
  /// the translation. The text stays because it is what was typed or pasted or captured,
  /// and getting it back is not always possible: a line that arrived from the clipboard
  /// watcher or out of a capture is gone once it is gone.
  ///
  /// The overlay is not touched either. It is a window the reader placed over something
  /// they are reading, and putting the analysis down is not a request to take it away.
  /// Clear the analysis **and the box it came from**.
  ///
  /// The text used to stay, and the tooltip said so — which was answering the wrong question. A
  /// button called Clear, sitting beside a full text box, looks like it empties that box; the old
  /// behaviour was to keep the label and explain the exception in a bubble nobody reads before
  /// pressing. Doing what the label says is the smaller surprise, and re-pasting is cheap where
  /// hunting for the way to empty the box is not.
  function clearReader() {
    analysisController.cancel();
    captureController.cancel();
    sentenceTranslationController.cancel();
    passageTranslationController.cancel();
    resetComparison();
    stopActiveAudio();
    ankiRequest.cancel();
    lookupController.cancel();
    particleController.cancel();
    nativeRequests.cancel("prefetch_analysis");
    busy = false;
    translatingSentences = false;
    translatingContext = false;
    ankiBusy = false;
    text = "";
    sourceState = draftSource();
    passageSizing = { kind: "auto" };
    study = null;
    selected = null;
    word = null;
    vocabularyRanking = null;
    particleSenses = null;
    error = "";
    sentenceTranslations = [];
    sentenceTranslationError = "";
    translatedSentences = "";
    contextTranslation = "";
    contextTranslationError = "";
    translatedContext = "";
    translationOpen = false;
    translationView = "aligned";
  }

  async function analyze() {
    startupDone = true;
    if (busy) return;
    const passage = text;
    const request = analysisController.begin();
    sentenceTranslationController.cancel();
    passageTranslationController.cancel();
    // A failed draft analysis leaves the previous successful passage on screen. Cancel work that
    // could still land, but do not erase that passage's comparison or translations until a new
    // StudyView has actually succeeded.
    comparisonController.cancel();
    stopActiveAudio();
    ankiRequest.cancel();
    lookupController.cancel();
    particleController.cancel();
    nativeRequests.cancel("prefetch_analysis");
    try {
      const result = await requestStudy(passage, request.request);
      if (!analysisController.current(request) || text !== passage) return;
      resetComparison();
      sentenceTranslations = [];
      sentenceTranslationError = "";
      translatedSentences = "";
      contextTranslation = "";
      contextTranslationError = "";
      translatedContext = "";
      study = result;
      sourceState = compactSource(result.analysis.text);
      passageSizing = { kind: "auto" };
      translationView = "aligned";
      vocabularyRanking = null;
      particleSenses = null;
      // Land on the first real word so the panels are never blank after analysing — and mark
      // it like any other, which is what selecting a New word means everywhere else.
      const first = study.analysis.tokens.findIndex((t) => t.senses.length > 0 || t.conjugation);
      if (first >= 0) select(first);
      // Remaining in the open card is itself an opt-in request, but expansion preferences belong
      // only to the passage that opened them and were reset above.
      if (translationOpen) void translateSentences();
    } catch (thrown) {
      analysisController.reject(request, thrown);
    } finally {
      analysisController.finish(request);
    }
  }

  /// **Every route here promotes.** There used to be an `origin` parameter whose only job was to
  /// stop the word `analyze` lands on from being marked Seen — the argument being that analysis
  /// finishing is not a reading gesture. On screen it is indistinguishable: the first word wears
  /// the same highlight as one reached with the arrow keys, and only one of the two was recorded.
  /// The rule is now simply that the selector sitting on a New word is what marks it.
  ///
  /// The visible consequence is that analysis nobody asked for marks a word too — the clipboard
  /// watcher runs one per copied line. That is the intended trade rather than an oversight.
  async function select(index: number, moveFocus = false) {
    selected = index;
    // **A new word is selected whole.** Carrying a part index across words would open "the second
    // part" of something that has no parts, or a different word's.
    selectedPath = [];
    partStatus = null;
    vocabularyScroll?.scrollTo({ top: 0, left: 0 });
    grammarScroll?.scrollTo({ top: 0, left: 0 });
    if (moveFocus && tabNavigation) readerFocusRequest += 1;
    const chosen = tokens[index];
    if (!chosen) return;
    promote(index);

    const lookupRequest = lookupController.begin();
    const particleRequest = particleController.begin();
    stopActiveAudio();
    ankiRequest.cancel();
    ankiBusy = false;
    word = null;
    vocabularyRanking = null;
    particleSenses = null;
    expressionOffer = null;
    expressionFailed = false;
    ankiMessage = "";
    ankiFailed = false;
    audioNotice = null;
    // Alongside rather than after: the senses come out of the bundled pack and a table
    // already in memory, so they land long before the dictionary does and there is no
    // reason to make them wait for it. Same generation guard, same reason.
    invoke<Reading | null>("particle_senses", {
      tokens,
      index,
      request: particleRequest.request,
    })
      .then((ranked) => {
        particleController.accept(particleRequest, () => (particleSenses = ranked));
      })
      // A particle with no senses written for it is the ordinary case, not a fault, and
      // the grammar panel simply shows what it showed before.
      .catch(() => {})
      .finally(() => particleController.finish(particleRequest));

    // The expression entry for a construction covering this selection, asked for only now — a
    // passage full of recognised constructions should not pay a dictionary query for offers
    // nobody opened.
    //
    // `wordPatterns` rather than every pattern in the passage: it is already the covering set the
    // grammar panel is drawn from, so the offer is about the same match the reader can see.
    const expressionRequest = expressionController.begin();
    invoke<ExpressionOffer | null>("expression_offer", {
      tokens,
      index,
      matches: wordPatterns,
      request: expressionRequest.request,
    })
      .then((offer) => {
        // `accept` is what makes a slow answer for an abandoned selection harmless: it runs the
        // assignment only while this request is still the current one.
        expressionController.accept(expressionRequest, () => {
          expressionOffer = offer;
          expressionFailed = false;
        });
      })
      // A failure is recorded rather than swallowed. Most selections have no offer at all and that
      // is silent; a dictionary that could not be read is not the same thing and says so.
      .catch(() => {
        expressionController.accept(expressionRequest, () => {
          expressionOffer = null;
          expressionFailed = true;
        });
      })
      .finally(() => expressionController.finish(expressionRequest));

    // Start the visible answer first, then prepare only its immediate reading neighbours. The
    // backend gives foreground work priority and coalesces an exact race; this call never selects
    // or marks those neighbours.
    const lookup = invoke<ContextualWordView>("lookup_in_context", {
      tokens,
      index,
      request: lookupRequest.request,
    });
    const nearby = neighboringWordIndices(wordIndices, index);
    if (nearby.length > 0) {
      const prefetch = nativeRequests.begin("prefetch_analysis", "speculative");
      void invoke<number>("prefetch_analysis", { tokens, indices: nearby, request: prefetch })
        .catch(() => {})
        .finally(() => nativeRequests.complete(prefetch));
    }

    try {
      const found = await lookup;
      lookupController.accept(lookupRequest, () => {
        word = found.word;
        vocabularyRanking = found.ranking;
      });
    } catch (thrown) {
      // A dictionary failure must not take the grammar view down with it.
      lookupController.reject(lookupRequest, thrown);
    } finally {
      lookupController.finish(lookupRequest);
    }
  }

  /// Open a part of the selected word, or the whole word again with an empty path.
  ///
  /// **The same shape as `select`**, deliberately: the panes reset, every in-flight request for
  /// the previous selection is superseded through the same controllers, and the answers that land
  /// are the new word's. Whole → part → a different word must not let the first answer overwrite
  /// the third, and it is the controllers rather than a flag that make that so.
  async function selectPart(path: number[]) {
    const index = selected;
    if (index === null) return;
    // **The whole word goes through the ordinary route.** Returning from a part is a selection
    // like any other: it promotes, it scrolls, and — the reason this matters — it asks for the
    // expression offer again. Rebuilding half of `select` here left the offer gone for good.
    // **Resolved before anything is asked.** The lookup's expansion and the panel's token come
    // from one resolution: the path `expandForContext` accepted, which drops any step that is
    // withheld, missing, or could not be expanded without moving a sentence boundary. Status,
    // audio and lookup all act on that path and nothing else.
    const asked = expandForContext(tokens, index, path, study?.analysis.text ?? "");
    if (asked.path.length === 0) {
      await select(index);
      return;
    }
    const chosen = resolvePath(tokens[index] ?? null, asked.path).token;
    if (!chosen) return;

    selectedPath = asked.path;
    partStatus = null;
    vocabularyScroll?.scrollTo({ top: 0, left: 0 });
    grammarScroll?.scrollTo({ top: 0, left: 0 });

    const lookupRequest = lookupController.begin();
    const particleRequest = particleController.begin();
    stopActiveAudio();
    ankiRequest.cancel();
    ankiBusy = false;
    word = null;
    vocabularyRanking = null;
    particleSenses = null;
    // **No expression offer for a part**, and the one in flight is cancelled rather than merely
    // cleared. An offer is about a construction covering a *top-level* token, and the matches
    // were computed over that token list — asking about a part would be asking a question those
    // matches cannot answer. Clearing alone left a slow answer for the whole word free to arrive
    // afterwards and describe the part as though it were one.
    expressionController.cancel();
    expressionOffer = null;
    expressionFailed = false;
    ankiMessage = "";
    ankiFailed = false;
    audioNotice = null;

    // The part's own stored status, which the passage response never carried.
    const whose = statusKey(chosen);
    void invoke<WordStatus>("word_status", { token: chosen })
      .then((stored) => {
        // Keyed rather than positional, so a slow reply after the reader moved on is dropped.
        if (activeToken && statusKey(activeToken) === whose) partStatus = stored;
      })
      .catch(() => {});

    invoke<Reading | null>("particle_senses", {
      tokens: asked.tokens,
      index: asked.index,
      request: particleRequest.request,
    })
      .then((ranked) => {
        particleController.accept(particleRequest, () => (particleSenses = ranked));
      })
      .catch(() => {})
      .finally(() => particleController.finish(particleRequest));

    try {
      const found = await invoke<ContextualWordView>("lookup_in_context", {
        tokens: asked.tokens,
        index: asked.index,
        request: lookupRequest.request,
      });
      lookupController.accept(lookupRequest, () => {
        word = found.word;
        vocabularyRanking = found.ranking;
      });
    } catch (thrown) {
      lookupController.reject(lookupRequest, thrown);
    } finally {
      lookupController.finish(lookupRequest);
    }
  }

  /// **Turning the parts row off returns to the whole word.**
  ///
  /// Hiding the row while a part is open would leave the panel describing かっこ with nothing on
  /// screen saying so and no way back — and every action reads `activeToken`, so Known, Anki and
  /// audio would all still be about the part. The setting hides an affordance; it must not strand
  /// the selection behind it.
  ///
  /// Through `selectPart`, so the return is an ordinary selection: the panes reset and the
  /// expression offer for the whole word is asked for again.
  $effect(() => {
    if (!showWordParts && selectedPath.length > 0) void selectPart([]);
  });

  /// The selectable tokens, in reading order.
  ///
  /// Punctuation is skipped rather than stepped over-and-through: it has no dictionary
  /// entry, so landing on it would empty the panel for a keystroke on the way past.
  const wordIndices = $derived(
    tokens.reduce<number[]>((keep, token, index) => {
      if (isWord(token)) keep.push(index);
      return keep;
    }, []),
  );

  /// Move the selection one word along the text.
  function step(delta: number) {
    if (wordIndices.length === 0) return;

    // Nothing selected yet: come in from whichever end you are heading away from.
    if (selected === null) {
      select(delta > 0 ? wordIndices[0] : wordIndices[wordIndices.length - 1], true);
      return;
    }

    const at = wordIndices.indexOf(selected);
    if (at === -1) {
      select(wordIndices[0], true);
      return;
    }

    // Stop at the ends rather than wrapping. Holding the key to the end of a passage
    // and silently reappearing at the start loses your place, which is the one thing
    // reading along cannot afford.
    const next = at + delta;
    if (next < 0 || next >= wordIndices.length) return;
    select(wordIndices[next], true);
  }

  /// Let go of whatever the click focused, so nothing is left armed.
  ///
  /// **Focus is not a navigation mechanism in this app.** `onWindowKey` below already swallows
  /// bare Tab, so a focused control cannot be reached from another one — and a control that holds
  /// focus after its click has been dealt with is not waiting to be used, it is waiting to
  /// misfire. Press Space to scroll the passage after clicking Analyze and the browser runs
  /// Analyze again, because Space activates the focused button.
  ///
  /// Bubble phase, so every component's own `onclick` has already run and this only tidies up
  /// after it.
  ///
  /// **`data-keeps-focus` is the opt-out, and two of its three users need it to work at all.**
  /// `OcrSelect` and `Caption` close themselves on `focusout`: each focuses something inside
  /// itself as it opens, so blurring that would slam the thing shut in the same frame it
  /// appeared. `Split`'s divider is the third, and wants focus for a different reason — it is a
  /// `separator` whose arrow keys resize the panes, and with Tab gone a click is the only way it
  /// can ever be reached. The marker sits on those elements rather than as a list of selectors
  /// here, because the reason belongs with the widget and a list in this file goes stale the
  /// first time a class is renamed.
  function onWindowClick() {
    if (tabNavigation) return;
    const held = document.activeElement;
    if (
      held instanceof HTMLElement &&
      // Text entry is the obvious exemption: the paste box and the search field are useless
      // without the focus they were just given.
      !held.closest("input, textarea, select, [contenteditable], [data-keeps-focus]")
    ) {
      held.blur();
    }
  }

  /// Own the keys whose browser defaults do not belong in the desktop reader.
  ///
  /// On the window rather than individual controls so Tab cannot begin browser-style focus
  /// traversal from whichever control happened to receive the last click. Buttons remain
  /// focusable by mouse — `onWindowClick` above is what stops that focus outliving the click.
  function onWindowKey(event: KeyboardEvent) {
    if (!startupDone) {
      if (event.key === "Escape") { startupDone = true; event.preventDefault(); }
      return;
    }
    // **Bare Tab only.** This exists to stop browser-style focus traversal, which is what an
    // unmodified Tab does. `Ctrl+Tab` is not traversal: the Reference page's tab strip claims it
    // to cycle searches the way every browser does, and swallowing it here would have meant the
    // app both preventing and acting on the same key.
    if (!tabNavigation && event.key === "Tab" && !event.ctrlKey && !event.metaKey) {
      event.preventDefault();
      return;
    }

    // Arrow keys walk the analyzed text. Keeping this here rather than on the reader means it
    // works while reading without having to click into anything first — which is the point.
    //
    // Right is forward and Left is back. This used to ask which way the text ran, because a
    // vertical passage advances down and leftward; that setting is gone, and with it the branch.
    // Up and Down do nothing here, so they stay available to whatever else wants them.
    const delta = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (delta === 0) return;

    // Something nearer the key has already dealt with it. The split dividers are the ones that
    // matter: they resize on the arrows too, and the side-by-side divider takes Left/Right, which
    // is the reader's forward and back. Without this, focusing a divider and pressing an arrow
    // resizes the pane *and* walks the passage.
    //
    // `defaultPrevented` rather than `stopPropagation` on the divider's side, because
    // both listen on the window and stopping propagation there would not reach a second
    // listener on the same target.
    if (event.defaultPrevented) return;

    // A modifier means the key belongs to something else — word-wise caret movement,
    // a browser shortcut, or a chord being recorded in the settings.
    if (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return;

    // In standard Windows navigation mode, arrows belong to the control that currently has
    // focus. The reader is a roving-focus composite, so it claims them only while one of its
    // word buttons is focused. The reader-first default deliberately keeps the historical
    // global stepping behaviour.
    const target = event.target;
    if (
      tabNavigation &&
      (!(target instanceof HTMLElement) || !target.closest("[data-reader-token]"))
    ) {
      return;
    }

    // Only while reading. Two reasons, and the settings becoming a page collapsed them into one
    // check: on any other page the reader is unmounted, so stepping its selection would move a
    // highlight nobody can see and change what greets them on the way back — and on the settings
    // page specifically, BindRecorder listens on the window too and `stopPropagation` does not
    // reach a second listener on the same target, so recording a binding would also walk the text.
    // That second case needed its own `settingsOpen` check while the settings were a drawer over
    // the reader; a page is covered by this line.
    if (view !== "read") return;

    // Not while typing. `closest` rather than a tag check so a control nested in a
    // label still counts.
    if (
      target instanceof HTMLElement &&
      target.closest("input, textarea, select, [contenteditable='true']")
    ) {
      return;
    }

    if (wordIndices.length === 0) return;
    event.preventDefault();
    step(delta);
  }



  // The glossary. Free of dictionary state on the Rust side, so these answer on a fresh
  // install where nothing has been downloaded.
  const lookTerm = (id: string) => invoke<Term | null>("explain_term", { id });
  // Every tag on one word at once. The chips are drawn together, so they are
  // resolved together.
  const explainTags = (codes: string[]) =>
    invoke<Record<string, TagExplanation>>("explain_tags", { codes });
  const searchTerms = (query: string) => invoke<Term[]>("search_terms", { query });
  // The five particles that have senses written for them. Bundled JSON, parsed once, so
  // this answers instantly and can ride along with the dictionary search.
  const searchParticles = (query: string) =>
    invoke<ParticleView[]>("search_particles", { query });
  /// The whole glossary, fetched at most once for the life of the window.
  ///
  /// It is a `const` table compiled into the binary and cannot change while the app runs,
  /// but it was being asked for again on every Grammar mount — so once per tab switch —
  /// and the reference page kept a second copy of the answer. Sharing the promise rather
  /// than the array means callers that arrive together also share the one request.
  ///
  /// A failure is not cached: the slot is cleared so the next caller starts a fresh
  /// attempt instead of being handed the old rejection forever.
  let glossaryOnce: Promise<Term[]> | null = null;
  const allTerms = () =>
    (glossaryOnce ??= invoke<Term[]>("all_terms").catch((thrown) => {
      glossaryOnce = null;
      throw thrown;
    }));
  const explainPattern = (id: string) => invoke<GrammarPattern | null>("explain_pattern", { id });
  const searchGrammar = (query: string) => invoke<GrammarPattern[]>("search_grammar", { query });
  const patternsForTerm = (id: string) => invoke<GrammarPattern[]>("patterns_for_term", { id });
  const searchWords = (query: string) => invoke<Found>("search_words", { query });
  const lookupKanji = (character: string) => invoke<KanjiView | null>("lookup_kanji", { character });
  const prefetchReference = (query: string) => invoke<boolean>("prefetch_reference", { query });

  /// Which secondary indexes each installed dictionary has.
  ///
  /// **Not the same question as `needs_gloss_index`.** That one asks whether English search works,
  /// and because it does for a dictionary that merely has glosses, nothing ever reported the other
  /// buckets. An index without `sequence` cannot follow a cross-reference to the word it points
  /// at, so a spelling filed as one shows an arrow where a definition belongs — and the reader is
  /// given no way to find that out, let alone fix it.
  /// The maintenance status, and which of the four states it is in.
  ///
  /// **Four, not two.** `loading` is not `failed` — the panel used to say "the status could not be
  /// read" for the whole time the first request was in flight — and an empty array is *no
  /// dictionary installed*, which is not "every index is present". Announcing the last of those to
  /// someone whose status never arrived is a claim with nothing behind it, about a repair the same
  /// panel is failing to offer.
  let dictionaryUpkeep = $state<DictionaryUpkeep[] | null>(null);
  let upkeepState = $state<"loading" | "ready" | "failed">("loading");
  let upkeepError = $state("");
  /// Generation, so a slow answer cannot overwrite a newer one. Dictionaries are replaced while
  /// the panel is open — an install finishes, a recovery republishes — and the request that was
  /// already in flight describes the store as it was.
  let upkeepRequest = 0;

  async function refreshDictionaryUpkeep() {
    const mine = ++upkeepRequest;
    if (dictionaryUpkeep === null) upkeepState = "loading";
    try {
      const found = await invoke<DictionaryUpkeep[]>("dictionary_upkeep");
      if (mine !== upkeepRequest) return;
      dictionaryUpkeep = found;
      upkeepState = "ready";
      upkeepError = "";
    } catch (thrown) {
      if (mine !== upkeepRequest) return;
      dictionaryUpkeep = null;
      upkeepState = "failed";
      upkeepError = errorMessage(thrown);
    }
  }

  // **Read where the state exists, not at the top of the script.** Called earlier, this threw
  // `Cannot access 'dictionaryUpkeep' before initialization` — the declaration is further down, and
  // a `$state` binding is in its temporal dead zone until then. The panel then showed "the status
  // could not be read" to everyone, in the app as much as in the harness.
  void refreshDictionaryUpkeep();

  /// Rebuild the missing indexes, then show the result without making the reader start again.
  ///
  /// The command is the existing claimed backfill — it owns the dictionaries, publishes a new
  /// generation, rebuilds this session's readers and drops the cached answers. What is left for
  /// here is the *visible* part: the status the panel reads, and the word already on screen, whose
  /// definition was looked up before the index existed. `select` re-runs that lookup alone; the
  /// passage is untouched, because nothing about the analysis changed.
  /// Build the English gloss index, resolving once the backend says it is done.
  ///
  /// It runs on a background thread and reports by event, because reading Jitendex's
  /// 433,885 entries takes about ten seconds and the command thread is the window.
  /// The rebuild, shared by both places that can ask for it.
  ///
  /// **Because the event is not correlated.** `gloss-index-built` carries no request id, and
  /// Settings and Reference each registered their own listener — so two overlapping requests meant
  /// two listeners for one event, and a refusal meant for one of them settled both. See
  /// `sharedOperation` for why joining beats a busy flag.
  const glossIndexBuild = sharedOperation<string>();

  /// Rebuild the missing indexes, and leave the interface showing what changed.
  ///
  /// **The refresh belongs to the operation, not to one of its callers.** It used to live in a
  /// wrapper that only Settings called; Reference asked for the rebuild directly, so a rebuild
  /// started there published a new index and left the status stale and the word on screen still
  /// showing the answer from before it. There is one function now, and both callers have it —
  /// which is why this is not something a test has to keep checking.
  ///
  /// In a `finally`, because a rejection does not mean nothing happened: the backfill walks every
  /// dictionary and reports the *last* failure, so one can publish while another fails. And inside
  /// the shared operation, so callers that join get one refresh between them rather than one each.
  function buildGlossIndex(): Promise<string> {
    return glossIndexBuild.run(async () => {
      try {
        return await startGlossIndexBuild();
      } finally {
        await refreshDictionaryUpkeep();
        // Read here rather than captured before the build: the reader may have selected a
        // different word while it ran, and the one to refresh is the one they are looking at.
        if (selected !== null) await select(selected);
      }
    });
  }

  async function startGlossIndexBuild(): Promise<string> {
    // Registered and *awaited* before the work is asked for. `listen` is async, and
    // `build_gloss_index` emits immediately when there is nothing to build — so starting
    // the two together let the event beat the listener, leaving a promise that never
    // settled and a "Build it" button disabled for the rest of the session.
    let settle: (detail: string) => void;
    let fail: (error: Error) => void;
    const done = new Promise<string>((resolve, reject) => {
      settle = resolve;
      fail = reject;
    });
    const unlisten = await listen<{ ok: boolean; detail: string }>(
      "gloss-index-built",
      (event) => {
        if (event.payload.ok) settle(event.payload.detail || "Indexed.");
        else fail(new Error(event.payload.detail));
      },
    );

    try {
      await invoke("build_gloss_index");
      return await done;
    } finally {
      // In a `finally`, so a failed invoke does not leak a listener that would then
      // resolve somebody else's promise.
      unlisten();
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && sourceState.kind === "draft" && sourceState.previous !== null) {
      event.preventDefault();
      cancelSourceEdit();
      return;
    }
    // Enter analyses; Shift+Enter keeps its normal newline.
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      if (!busy && !capturing) void analyze();
    }
  }
</script>

<svelte:window onkeydown={onWindowKey} onclick={onWindowClick} onfocus={lookForAnki} />

<div class="app">
  <!-- The window's title bar, which is ours now.

       It spans the whole width, above the rail as well as the reading area, because that is
       where a title bar goes and because the strip over the rail has to be draggable too.
       `Caption.svelte` draws the three OS buttons; everything Windows still does for us — the
       resize borders, snapping, the shadow, the flyout on the maximise button — is explained
       in `src-tauri/src/titlebar.rs`.

       `data-tauri-drag-region` is on this element and on the wordmark, and nowhere else:
       Tauri matches the element the pointer is actually on, so the buttons inside are safe
       from it without having to opt out. Height comes from Rust — see `--titlebar-h`. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- The handler is a right-click on a title bar, which has no ARIA role to give it and no
       keyboard equivalent to add: the keyboard route to this menu is Alt+Space, which is
       Windows' own and does not pass through the page. Everything the menu offers — minimise,
       maximise, close — is a focusable button in this same bar. -->
  <div
    class="titlebar"
    style="--titlebar-h: {titlebarHeight}px"
    data-tauri-drag-region
    oncontextmenu={(event) => {
      // The system menu, which `DefWindowProc` cannot offer any more: there is no native
      // caption for it to open over. Right-clicking a title bar does this on every other
      // window, and its absence is the kind of small thing that reads as "not a real window".
      event.preventDefault();
      invoke("window_menu", { x: event.clientX, y: event.clientY }).catch(() => {});
    }}
  >
    <span class="wordmark" data-tauri-drag-region>Sakatsu Reader</span>

    <!-- The one control of ours that lives up here, and the reason is that it is about the
         *window* rather than about the page: "keep this above other applications" belongs
         beside minimise and maximise, which is where a Windows user looks for it. The reading
         toggles stay in the header below, next to what they change.

         It is drawn inside `Caption` rather than beside it so that it shares those buttons'
         shape rather than copying it — scoped styles do not travel between components, which
         is a trap this repo has already paid for once. -->
    <Caption
      width={captionWidth}
      pin={!needsSetup}
      pinned={alwaysOnTop}
      onpin={() => {
        alwaysOnTop = !alwaysOnTop;
        // Now, not in 400ms when the debounced save runs. The setting still travels the
        // ordinary way; this is the window obeying the click that asked.
        invoke("pin_window", { on: alwaysOnTop }).catch(() => {});
      }}
    >
      <!-- **The state stays here.** The panel is a second view of settings this component already
           owns, so it binds them rather than keeping its own — the two surfaces cannot drift
           because there is only one value behind each. Hidden during setup for the pin's reason:
           there is nothing to look at yet. -->
      {#snippet menu()}
        {#if !needsSetup}
          <AppMenu
            {view}
            settingsOn={view === "settings"}
            {theme}
            onpalette={applyPalette}
            {themePresets}
            bind:fontSize
            bind:readerScale
            bind:panelScale
            bind:ocr
            bind:analyzer
            bind:keepLineBreaks
            bind:decorCloudOn
            bind:decorSakuraOn
            bind:passageAlign
            bind:copyCaptureToClipboard
            onsettings={() => show("settings")}
            onhelp={() => show("help")}
            onrestart={restartApp}
          />
        {/if}
      {/snippet}
    </Caption>
  </div>

  {#if !startupDone}
    <Startup settings={settingsLoaded} settingsFailed={startupSettingsFailed}
      resources={setup.loaded} resourcesFailed={Boolean(setup.error)}
      needsSetup={Boolean(setup.snapshot && !setup.snapshot.required_ready)}
      {titlebarHeight} animationMs={settingsLoaded ? animationMs : 0} oncontinue={() => { startupDone = true; }} />
  {/if}
  <div class="workspace" inert={!startupDone} style:z-index={startupDone ? undefined : 0}>
  <Sidebar
    {view}
    sakuraSpread={decorSakuraSpread}
    switchable={!needsSetup}
    onview={(chosen) => show(chosen)}
  />

  <main style="--footer-h: {footerHeight}px">
    <DictionaryUpdateNotice onmanage={() => { settingsSection = "search"; show("settings"); }} />
  <header>
    <!-- Off screen, not gone. The title bar draws "Sakatsu Reader" a centimetre above this,
         so printing it again would say the name twice — but that one is a `<span>`, because a
         drag region is not a heading, and the seal in the rail is `aria-hidden` precisely
         because this carries the name. Deleting it would leave the page with no heading. -->
    <h1 class="name offscreen">Sakatsu Reader</h1>
    <!-- Both stay in the header: they are toggled constantly while reading, and burying
         them two clicks deep in a drawer would be a downgrade.

         Only while reading, though. Neither does anything to a page of search results,
         and a control that visibly does nothing is worse than one that is not there. -->
    {#if view === "read" && !needsSetup}
      <!-- Icons rather than labelled checkboxes. That trades a word you can read for a
           shape you have to learn, so each carries the sentence its label used to be as a
           `title`, and `aria-pressed` keeps the on/off state readable to a screen reader
           where a checkbox used to carry it for free. -->
      <div class="toggles">
        <button
          class="toggle tip-right"
          class:on={furigana}
          onclick={() => (furigana = !furigana)}
          aria-pressed={furigana}
          aria-label="Furigana"
          data-tip="Furigana — the reading over words with kanji"
        >
          <!-- A kana, rather than an abstraction of one. The drawn version was two light
               marks over two heavier lines — the right idea, but it read as a generic
               "annotation" mark and could have meant anything. This is the alphabet the
               feature puts over the kanji, which is as literal as an icon gets. -->
          <span class="kana" aria-hidden="true">ア</span>
        </button>

        <button
          class="toggle tip-right"
          class:on={readClipboard}
          onclick={() => (readClipboard = !readClipboard)}
          aria-pressed={readClipboard}
          aria-label="Read clipboard"
          data-tip="Read the clipboard — analyzes Japanese as it is copied"
        >
          <!-- The board, its clip sitting on top rather than cut into it, and two lines
               of what was copied. Rects rather than an arced outline: the same shape with
               nothing to get wrong. -->
          <svg viewBox="0 0 24 24" width="17" height="17" aria-hidden="true">
            <!-- Everything half a unit lower than it was drawn: the ink ran 1.9 to 21.1,
                 centring it on 11.5 in a box whose centre is 12. -->
            <rect
              x="5.4" y="6.1" width="13.2" height="14.6" rx="2.2"
              fill="none" stroke="currentColor" stroke-width="1.8"
            />
            <rect
              x="8.8" y="3.3" width="6.4" height="2.9" rx="1.2"
              fill="none" stroke="currentColor" stroke-width="1.8"
            />
            <path
              d="M8.6 11.9L15.4 11.9M8.6 15.7L12.8 15.7"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
            />
          </svg>
        </button>

        <!-- Showing the overlay and reading through it stay two jobs on two controls:
             this one works the window, and the binding is what reads through it.

             One gesture per job: a click shows or hides the overlay, a double click
             switches the whole feature off and back on.

             Two states, two treatments, and they cannot collide: the accent means the
             frame is up, the attention colour means the feature is switched off, and
             switching off takes the frame down — so `on` and `off` are never both true.
             `aria-pressed` reports the first of them and is dropped entirely while the
             feature is off, because a button whose click does nothing is not a pressed
             or unpressed toggle, it is neither. -->
        <button
          class="toggle overlay tip-right"
          class:on={overlayEnabled && overlayShowing}
          class:off={!overlayEnabled}
          onclick={overlayClick}
          ondblclick={overlayDoubleClick}
          disabled={capturing}
          aria-pressed={overlayEnabled ? overlayShowing : undefined}
          aria-label={overlayEnabled
            ? overlayShowing
              ? "Hide the capture overlay"
              : "Show the capture overlay"
            : "Turn the capture overlay back on"}
          data-tip={overlayEnabled
            ? overlayShowing
              ? "Hide the capture overlay — double-click to switch it off"
              : "Show the capture overlay — double-click to switch it off"
            : "The capture overlay is off, and both its keys with it — double-click to switch it on"}
        >
          <!-- The frame itself, as four corner brackets: what the overlay looks like on
               screen, which is as literal as the ア and the clipboard beside it. The bar
               through it appears only when it is off, because a state carried by colour
               alone is a state some people cannot see. -->
          <svg viewBox="0 0 24 24" width="17" height="17" aria-hidden="true">
            <path
              d="M4 9V5.6A1.6 1.6 0 0 1 5.6 4H9M15 4h3.4A1.6 1.6 0 0 1 20 5.6V9M20 15v3.4a1.6 1.6 0 0 1-1.6 1.6H15M9 20H5.6A1.6 1.6 0 0 1 4 18.4V15"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
            />
            {#if !overlayEnabled}
              <path
                d="M5 19L19 5"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
              />
            {/if}
          </svg>
        </button>

        <!-- The frame's own controls, next to the button that puts the frame up.
             They used to live on the overlay itself, where they competed with the region
             for pixels and shrank with it — the frame is something you look *through*, and
             a control surface is the one thing it cannot also be. Here they cost the reader
             nothing, and this window was already where `frame-lock` and `frame-clickthrough`
             were handled, so it is now both the writer and the reader of them.

             Shown only while the overlay is enabled: controls for something switched off
             are noise, and the same rule already hides this whole row outside the reader. -->
        {#if overlayEnabled}
          <button
            class="toggle small tip-right"
            class:on={frameLocked}
            onclick={() => (frameLocked = !frameLocked)}
            aria-pressed={frameLocked}
            aria-label="Lock the capture frame"
            data-tip={frameLocked
              ? "Locked — the show/hide key leaves the frame where it is"
              : "Unlocked — the show/hide key brings the frame to the pointer"}
          >
            <!-- The shackle carries the state, not the colour: open leans off the body,
                 closed sits square on it. -->
            <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
              <rect x="3" y="7" width="10" height="7" rx="1.4" fill="currentColor" />
              <path
                d={frameLocked ? "M5.5 7V5a2.5 2.5 0 0 1 5 0v2" : "M5.5 7V5a2.5 2.5 0 0 1 5 0"}
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
              />
            </svg>
          </button>

          <button
            class="toggle small tip-right"
            class:on={frameClickthrough}
            onclick={toggleFrameClickthrough}
            aria-pressed={frameClickthrough}
            aria-label="Let clicks pass through the capture frame"
            data-tip={frameClickthrough
              ? "Clicks pass through the frame — its edges still resize it"
              : "Clicks land on the frame"}
          >
            <!-- A pointer through a pane. Filled when on, outlined when off, so the state
                 is not carried by colour alone — the same rule the shackle follows. -->
            <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
              <rect
                x="2" y="2" width="12" height="12" rx="1.6"
                fill={frameClickthrough ? "none" : "currentColor"}
                opacity={frameClickthrough ? 1 : 0.35}
                stroke="currentColor"
                stroke-width="1.3"
              />
              <path d="M6.2 5.4 11 8.2l-2 .5-.6 2z" fill="currentColor" />
            </svg>
          </button>
        {/if}
      </div>
    {/if}

  </header>

  {#if settingsSaveError}
    <p class="error">{settingsSaveError}</p>
  {/if}

  {#if needsSetup}
    <FirstRun
      onready={finishFirstRun}
      onexplore={exploreAfterFirstRun}
      onadopt={adoptFirstRunIchiran}
      onlindera={useLinderaForNow}
    />
  {:else if view === "glossary"}
    <Glossary {allTerms} bind:sort={glossarySort} />
  {:else if view === "help"}
    <!-- The three bindings go in rather than being written into the prose. A help page
         naming a hotkey somebody rebound is worse than one that says nothing at all. -->
    <Help
      {captureBind}
      {overlayBind}
      {switchBind}
      {tabNavigation}
      backTo={VIEW_LABELS[cameFrom]}
      onexit={() => show(cameFrom)}
      onopensettings={openSettingsAt}
    />
  {:else if view === "settings"}
    <!-- **In the chain, not over it.** This was a drawer at `z-index: 5` that covered the reader,
         which is why it needed a slide, a click-away and an `open` flag; as one of the views it is
         mounted and unmounted like the rest and needs none of them. `section` is bound because the
         title bar's menu picks one on the way in, and because it is remembered across a restart. -->
  <SettingsPanel
    backTo={VIEW_LABELS[cameFrom]}
    onexit={() => show(cameFrom)}
    onpalette={applyPalette}
    {dictionaryUpkeep}
    {upkeepState}
    {upkeepError}
    updateDictionaryIndexes={buildGlossIndex}
    retryDictionaryUpkeep={refreshDictionaryUpkeep}
    bind:vlmEndpoint
    bind:vlmModel
    bind:keepLineBreaks
    bind:markReadingGuesses
    bind:showWordParts
    bind:markSeenOnFocus
    bind:tabNavigation
    bind:fontSize
    bind:readerScale
    bind:panelScale
    bind:quiet
    bind:ichiranServer
    bind:preloadIchiran
    bind:checkUpdatesOnStartup
    oninstallupdate={() => settingsController.beforeExit(() => invoke<UpdateStatus>("update_install"))}
    bind:logging
    bind:collectSentences
    {logFile}
    {corpusFile}
    {warmupDiagnostics}
    {runtimeStatus}
    {runtimeStatusFailed}
    ondiagnosticreport={() => invoke<string>("diagnostic_report")}
    oncopyreport={(report) => invoke("copy_text", { text: report })}
    onunload={(role) =>
      invoke<boolean>(role === "recognition" ? "unload_ocr" : "unload_translator").finally(
        refreshRuntimeStatus,
      )}
    onreveallog={revealLog}
    onrevealcorpus={revealCorpus}
    onrestart={restartApp}
    bind:holdMs
    bind:lockTapMs
    bind:frameTint
    bind:hideDuringCapture
    bind:animations
    bind:animationMs
    bind:hideSetup
    bind:analyzer
    {analyzerEngine}
    onadopt={adoptIchiran}
    bind:ocr
    bind:preprocessTighten
    bind:preprocessContrast
    bind:preprocessUpscale
    bind:ankiDeck
    bind:ankiOurDecksOnly
    bind:theme
    bind:themePresets
    decks={ankiDecks}
    deckInfo={ankiDeckInfo}
    ondeckaction={deckAction}
    {ocrEngine}
    {captureBind}
    {overlayBind}
    onCaptureBind={rebind}
    onOverlayBind={rebindOverlay}
    onSwitchBind={rebindSwitch}
    {switchBind}
    bind:clickDelayMs
    bind:section={settingsSection}
    bind:showSourceAnalysis
    bind:showSourceReference
    bind:frequencySource
    bind:frequencyDirection
    bind:showFrequencySource
    bind:decorCloud
    bind:decorCloudTint
    bind:decorCloudOn
    bind:decorSakura
    bind:decorSakuraTint
    bind:decorSakuraOn
    bind:decorScale
    bind:decorCloudPad
    bind:decorCloudSpread
    bind:decorSakuraSpread
    bind:decorBlur
    bind:decorCloudBlur
    bind:decorSakuraBlur
  />
  {:else if view === "about"}
    <About onupdates={() => openSettingsAt("updates")} />
  {:else if view === "reference"}
    <Reference
      {searchWords}
      {prefetchReference}
      {searchGrammar}
      {lookupKanji}
      {buildGlossIndex}
      {explainTags}
      showSource={showSourceReference}
      {showFrequencySource}
      {searchTerms}
      {searchParticles}
      {allTerms}
      {patternsForTerm}
      pronounce={playWordAudio}
      {audioBusy}
      {audioNotice}
    />
  {:else}
  {#if study && sourceState.kind === "compact"}
  <section class="source-bar" aria-label="Analyzed source">
    <span class="source-name">Source</span>
    <span class="source-divider" aria-hidden="true"></span>
    <span class="source-text" lang="ja">{sourceState.canonical}</span>
    <!-- **A glyph, not the word.** This row exists to give the passage every spare pixel, and a
         bordered button spelling out "Edit" competed with the text it sits beside. Borderless and
         faint until it is wanted, which is the treatment the header's icon toggles already use —
         see the families table in `local documentation`.

         `aria-label` as well as `data-tip`, per `local documentation`: the tip is a
         pseudo-element and carries no accessible name, so without the label this button would
         have none at all once the word came off it. -->
    <button
      class="source-edit tip-right"
      type="button"
      onclick={editSource}
      aria-label="Edit"
      data-tip="Edit the source text"
    >
      <!-- A pencil, drawn here rather than borrowed, so no icon set's licence travels with it.
           The body is a closed outline along the diagonal; the short stroke across it is the
           ferrule, which is what stops the shape reading as a plain arrow at this size. -->
      <svg
        viewBox="0 0 24 24"
        aria-hidden="true"
        fill="none"
        stroke="currentColor"
        stroke-width="1.8"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path d="M4.5 19.5 L4.9 17 L14 8 L16 10 L7 19.2 Z" />
        <path d="M11.8 10.1 L13.9 12.2" />
      </svg>
    </button>
    {#if passageSizing.kind === "manual" && passageClipped}
      <button class="control source-action" type="button" onclick={fitPassage}>Fit passage</button>
    {/if}
  </section>
  {:else}
  <section class="input">
    <textarea
      bind:this={sourceField}
      bind:value={text}
      onkeydown={onKeydown}
      rows="2"
      spellcheck="false"
      lang="ja"
      placeholder="Paste Japanese text"
    ></textarea>
  </section>
  {/if}

  <!-- This row is stable across draft and compact source states. The primary reading actions
       stay together; capture configuration trails as a separate cluster so wrapping never puts
       Translation on the far side of OCR. -->
  <section class="actions" aria-label="Analysis actions">
    <div class="action-primary">
    <!-- **The word alone, and only on this one.** This is the primary action — the thing you came
         to the page to do — and it says so in the plainest way there is. A glyph beside the word
         was tried and added nothing the word did not already say; two ways of naming one button
         is the clutter the row is avoiding.

         The word also carries the wait. Analysis is the longest one in the app, because Ichiran
         is a sidecar that loads a Lisp image, and `{busy ? "Analyzing…" : "Analyze"}` has always
         been this button's progress indicator. While it was a glyph that word had to be lent to
         `.shortcut` across the row, which is where it went back from.

         No `aria-label`: the visible text is the accessible name, and an `aria-label` beside it
         does not add a name, it *replaces* one — so the two would have to be kept in step for
         no gain. -->
    <button
      class="control primary tip-left"
      onclick={analyze}
      disabled={busy}
      data-tip="Analyze the text — split it into words, look them up, read the grammar"
    >
      <!-- `aria-hidden`, because the word beside it already says "Analyzing…" and a screen
           reader announcing a decorative shape as well would say it twice. -->
      {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
      {busy ? "Analyzing…" : "Analyze"}
    </button>
    <button
      class="control translation-action"
      class:active={translationOpen}
      type="button"
      onclick={toggleTranslation}
      disabled={!study}
      aria-pressed={translationOpen}
    >Translation</button>
    <button
      class="control danger"
      onclick={clearReader}
      disabled={busy || (!study && text.trim() === "")}
      data-tip="Clear the text and the analysis"
    >Clear</button>
    </div>
    <div class="action-trailing">
      <span class="shortcut" class:working={capturing} class:stacked={hint.faces.length > 1}>
        {#if hint.spinner}<span class="spinner" aria-hidden="true"></span>{/if}
        {#if hint.faces.length > 1}
          <!-- Both faces stay in the accessibility tree. A screen reader reads them in order —
               "Middle click reads PaddleOCR-VL" — which is the whole sentence, so there is
               nothing for an `aria-label` or a visually-hidden twin to add. -->
          {#each hint.faces as face, index}
            <span class="face" class:off={index !== hintFace}>{face}</span>
          {/each}
        {:else}
          {hint.text}
        {/if}
      </span>
    </div>
  </section>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  <!-- Only intrudes when something is missing; a working install sees a collapsed
       line, not a wizard — or nothing at all, once it has been dismissed. -->
  <Setup
    onready={() => dictionariesChanged()}
    hidden={hideSetup}
    onhide={() => (hideSetup = true)}
    onadopt={adoptIchiran}
  />

  {#if storeError}
    <!-- The sentence comes from Rust, whole. It used to be prefixed here with "Word status
         will not be saved", which was true of the one case that existed then and is wrong now
         that a store can recover: the commonest message says a file was set aside and a new
         one started, which is not a refusal to save. Only the side that knows what happened
         can word it. -->
    <p class="warning">{storeError}</p>
  {/if}

  <!-- Which action just failed, when it failed for the reason above. Deliberately not silent:
       a click that quietly does nothing looks like a click that worked, and that is the
       misunderstanding the standing warning exists to prevent. Deliberately not a second
       explanation either — it says what did not happen, and the line above says why. -->
  {#if saveNotice}
    <p class="save-notice">{saveNotice}</p>
  {/if}

  {#if study}
    <div
      class="analysis-workspace"
      bind:this={analysisWorkspace}
      bind:clientWidth={analysisWidth}
      bind:clientHeight={analysisHeight}
    >
      <Split
        bind:ratio={outerRatio}
        min={0}
        max={0.8}
        firstMin="var(--reading-floor)"
        onuserresize={useManualPassage}
      >
        {#snippet first()}
          <div class="column passage-column">
            {@render readerPane()}
          </div>
        {/snippet}

        {#snippet second()}
          <div class="study">
            {#if translationOpen}
              <TranslationCard
                sentences={sentenceTranslations}
                sentencesBusy={translatingSentences}
                sentencesError={sentenceTranslationError}
                comparison={carries}
                comparisonBusy={carriesBusy}
                comparisonError={carriesError}
                comparing={studied.size > 0}
                activeToken={comparisonToken}
                contextView={translationView === "context"}
                contextAvailable={contextCheckAvailable}
                {contextTranslation}
                contextBusy={translatingContext}
                contextError={contextTranslationError}
                maxHeight={translationCardMaxHeight}
                onclearcomparison={clearStudied}
                onretrycomparison={askWhatTheyCarry}
                onretrysentences={translateSentences}
                onretrycontext={translatePassage}
                oncontextview={showContext}
              />
            {/if}
            <section
              class="selected-word"
              aria-label="Selected word"
              data-testid="selected-word-card"
            >
              <SelectedWordHeader
                token={activeToken}
                {word}
                {status}
                onstatus={mark}
                onanki={addToAnki}
                onplay={playAudio}
                playing={audioBusy === "analysis"}
                audioMessage={audioNotice?.key === "analysis" ? audioNotice.message : ""}
                {ankiBusy}
                {ankiMessage}
                {ankiFailed}
                {ankiReady}
              />
              {#if repairNote}
                <p class="repair-note" data-testid="repair-note">{repairNote}</p>
              {/if}

              <!-- **Parts, under the heading and above the panes.**

                   The passage selects whole words — that is what reading is, and grouping exists
                   to make the whole word the easy lookup. What grouping took away was the ability
                   to reach かっこ at all, because the passage only ever selected top-level tokens.
                   This puts it back where the word is already being described, without touching
                   how the passage behaves.

                   Ordinary buttons: focusable, operable with Enter or Space, reachable by Tab in
                   reading order. No gesture, no long-press, nothing a keyboard cannot do. -->
              {#if showWordParts && (activeParts.length > 0 || inspectingPart)}
                <nav class="word-parts" aria-label="Parts of this word">
                  {#if inspectingPart}
                    <span class="parts-label">Part of</span>
                    {#each partTrail as step (step.path.join("."))}
                      <button
                        type="button"
                        class="part"
                        data-testid="whole-word-button"
                        onclick={() => selectPart(step.path)}
                        lang="ja">{step.label}</button
                      >
                    {/each}
                  {/if}
                  {#if activeParts.length > 0}
                    <span class="parts-label">{inspectingPart ? "Parts" : "Parts"}</span>
                    {#each activeParts as { part, at } (at)}
                      <button
                        type="button"
                        class="part"
                        data-testid="word-part-button"
                        onclick={() => selectPart([...selectedPath, at])}
                        lang="ja">{part.surface}</button
                      >
                    {/each}
                  {/if}
                </nav>
              {/if}

              <div class="study-detail">
                <!-- At the end of the headings row, not in the identity strip above. It is a
                     reply to what the panes are showing, and it belongs beside them. -->
                {#if anyDetailOpen}
                  <button class="reset-details" type="button" onclick={resetDetails}>Reset details</button>
                {/if}
              {#if constrainedStudy}
                <div
                  class="panel-switch"
                  role="tablist"
                  tabindex="-1"
                  aria-label="Analysis details"
                  bind:this={panelSwitch}
                  onkeydown={onPanelSwitchKey}
                >
                  <button
                    id="vocabulary-tab"
                    type="button"
                    role="tab"
                    aria-controls="vocabulary-panel"
                    aria-selected={narrowPanel === "vocabulary"}
                    tabindex={narrowPanel === "vocabulary" ? 0 : -1}
                    class:current={narrowPanel === "vocabulary"}
                    onclick={() => chooseNarrowPanel("vocabulary")}
                  >Vocabulary</button>
                  <button
                    id="grammar-tab"
                    type="button"
                    role="tab"
                    aria-controls="grammar-panel"
                    aria-selected={narrowPanel === "grammar"}
                    tabindex={narrowPanel === "grammar" ? 0 : -1}
                    class:current={narrowPanel === "grammar"}
                    onclick={() => chooseNarrowPanel("grammar")}
                  >Grammar</button>
                </div>
                <div class="column">
                  {#if narrowPanel === "vocabulary"}
                    <div
                      id="vocabulary-panel"
                      class="column-body"
                      role="tabpanel"
                      aria-labelledby="vocabulary-tab"
                      bind:this={vocabularyScroll}
                    >{@render vocabulary()}</div>
                  {:else}
                    <div
                      id="grammar-panel"
                      class="column-body"
                      role="tabpanel"
                      aria-labelledby="grammar-tab"
                      bind:this={grammarScroll}
                    >{@render grammar()}</div>
                  {/if}
                </div>
              {:else}
                <!-- `snapWithin` is pixels, and 14 is the one number here worth tuning by feel: wide
                     enough to catch a deliberate "put it back in the middle", narrow enough not to
                     fight a position chosen just off centre. It lives at the call site so the
                     passage split above is visibly unaffected. -->
                <Split direction="row" bind:ratio={panelsRatio} min={0.25} max={0.75} fill={false} snapWithin={14}>
                  {#snippet first()}
                    <div class="column">
                      <!-- The wrapper is a mask, not a layout box — see `.pane-head`. -->
                      <div class="pane-head"><h2 class="pane-name">Vocabulary</h2></div>
                      <div class="column-body" bind:this={vocabularyScroll}>{@render vocabulary()}</div>
                    </div>
                  {/snippet}
                  {#snippet second()}
                    <div class="column">
                      <div class="pane-head"><h2 class="pane-name">Grammar</h2></div>
                      <div class="column-body" bind:this={grammarScroll}>{@render grammar()}</div>
                    </div>
                  {/snippet}
                </Split>
              {/if}
              </div>
            </section>
          </div>
        {/snippet}
      </Split>
    </div>

    {#snippet readerPane()}
      <div class="column-body" bind:this={readingMeasure}>
        {#if engineNotice?.tone === "prominent"}
          <div class="warning engine-notice" role="status" data-testid="engine-notice">
            <span>{engineNotice.text}</span>
            <details>
              <summary>Details</summary>
              <p>{engineNotice.detail}</p>
            </details>
          </div>
        {:else if engineNotice}
          <p
            class="engine-quiet"
            role="status"
            data-testid="engine-label"
            title={engineNotice.detail}
          >
            {engineNotice.text}
          </p>
        {/if}
        <Reader
          text={study?.analysis.text ?? null}
          {tokens}
          {patterns}
          {statuses}
          {ruby}
          {selected}
          {furigana}
          {keepLineBreaks}
          {passageAlign}
          {markReadingGuesses}
          {studied}
          {tabNavigation}
          focusRequest={readerFocusRequest}
          onselect={(index) => select(index)}
          onknow={markKnown}
          onstudy={toggleStudied}
        />
      </div>
    {/snippet}

    {#snippet vocabulary()}
      <Vocabulary
        {word}
        ranking={vocabularyRanking}
        loading={lookingUp}
        hasDictionaries={dictionaries.length > 0}
        {explainTags}
        showSource={showSourceAnalysis}
        meaningsExpanded={detailOpen.meanings}
        examplesExpanded={detailOpen.examples}
        ontoggleMeanings={(open) => (detailOpen.meanings = open)}
        ontoggleExamples={(open) => (detailOpen.examples = open)}
      />
    {/snippet}

    {#snippet grammar()}
      <Grammar
        {token}
        {breakdown}
        patterns={wordPatterns}
        reading={particleSenses}
        expression={expressionOffer}
        {expressionFailed}
        {explainPattern}
        {lookTerm}
        {allTerms}
        alternativesExpanded={detailOpen.grammar}
        ontoggleAlternatives={(open) => (detailOpen.grammar = open)}
      />
    {/snippet}
  {:else if !busy}
    <!-- The empty state claims the whole remaining height rather than leaving a black
         void under a stranded line of text. The old copy here also said Ichiran was
         needed; it is not, and has not been since the built-in analyzer shipped. -->
    <div class="empty">
      <p class="hint">
        Paste Japanese above, or capture it from the screen or clipboard.
      </p>
    </div>
  {/if}
  {/if}

  <footer bind:clientHeight={footerHeight}>
    {#if counts.seen + counts.known + counts.ignored > 0}
      <!-- The learner's own progress reads a step brighter than the attribution
           beside it: it is the one thing in the footer that is about them. -->
      <span class="progress">
        {counts.known.toLocaleString()} known · {counts.seen.toLocaleString()} seen
      </span>
    {/if}
    {#if dictionaries.length > 0}
      {#each dictionaries as dictionary}
        <span class="credit" title={dictionary.attribution ?? ""}>
          {dictionary.title}
          {#if dictionary.entry_count}({dictionary.entry_count.toLocaleString()} entries){/if}
        </span>
      {/each}
    {:else}
      <span class="credit">No dictionaries — run scripts/fetch-dictionaries.ps1</span>
    {/if}
  </footer>
    <!-- Last, and absolutely positioned, so it sits outside the column flow entirely. -->
    <Kasumi spread={decorCloudSpread} />
  </main>
  </div>
</div>

<style>
  /* The title bar, then everything else. A column now: the bar spans the full width above
     both the rail and the reading area, which is where a title bar goes — and it means the
     strip over the rail is draggable too. */
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    /* **How short the reading pane is allowed to get.** One line of Japanese with its
     * furigana and its status rule, at whatever reading scale is set.
     *
     * It used to carry a second term, `--translation-band`, because the translation sat under
     * the passage in this same pane and outside its scroll — so a floor of one line alone still
     * cut the line, by exactly the height of the band. The translation is a mode of the study
     * half now, this pane holds the reader and nothing else, and the arithmetic goes with it.
     *
     * Declared here rather than inside the component because `<Split>` is what has to enforce
     * it, and a custom property is the only thing that reaches it. */
    --reading-floor: var(--reader-line);
    /* The window's own chrome. The title bar and the rail are the same surface, so they read
       as one continuous frame that `.workspace::before` completes behind the reading area's
       rounded corner. */
    background: var(--surface);
  }

  /* The rail, then the reader. `relative` is left over from the settings drawer, which was
     absolute against this; it stays because `.workspace::before` is too
     against — it slides over the reader, and now cannot slide over the caption buttons, which
     it could if it were still positioned against the whole window. */
  .workspace {
    display: flex;
    flex: 1;
    /* Or the rail and the reader push the bar off the top of a short window. */
    min-height: 0;
    position: relative;
  }

  /* The reading area, inset into that chrome instead of butting into the title bar and the
     rail at a hard right angle.

     A pseudo-element rather than a wrapper: `main` is centred inside the space with a
     max-width, so it cannot carry this background itself, and absolute positioning keeps
     this out of the flex flow so the rail beside it is undisturbed. */
  .workspace::before {
    content: "";
    position: absolute;
    inset: 0 0 0 var(--rail);
    background: var(--bg);
    border-top-left-radius: var(--radius-lg);
    /* Absolutely positioned, so without this it would paint over `main` rather than
       behind it. */
    z-index: 0;
  }

  /* **The height must not drift from `TITLEBAR_HEIGHT` in `titlebar.rs`.** That number is what
     the hit-test overlay is sized to, so a taller bar leaves a strip at the bottom of the
     maximise button that does not raise the flyout, and a shorter one puts the overlay over
     the page. It is fetched rather than copied — `Caption.svelte` asks `titlebar_metrics` —
     and this fallback only stands for the instant before the answer lands.

     `flex: 0 0 auto` because a bar that grows is a bar that steals reading area. */
  .titlebar {
    flex: 0 0 var(--titlebar-h, 32px);
    display: flex;
    align-items: center;
    /* Left-padded only: the caption buttons must reach the window's right edge, and the
       close button reaching the corner is what makes the throw everyone makes at it land. */
    padding-left: var(--gap);
    /* The wordmark, then the space that does the dragging, then the controls. */
    gap: var(--gap-sm);
    /* Above the reading area's rounded corner, the rail's own paint, and everything below.
     *
     * **`3`, not `2`, because the settings menu drops out of this bar.** `position` plus a
     * `z-index` makes this a stacking context, so nothing inside can escape it however high it
     * goes — the menu's own `z-index: 1` sorts it within this bar and nowhere else. At `2` the bar
     * tied with `.rail`, also `2`, and lost on tree order: the menu would have opened *behind* the
     * rail. Lifting the bar is the fix; lifting the menu is the trap. See `traps.md`. */
    position: relative;
    z-index: 3;
    /* Nothing in here is text you select — and a drag that begins with a text selection is a
       drag that does not move the window. */
    user-select: none;
  }

  /* The name, at last on screen. It was hidden while Windows drew a bar saying the same thing
     a centimetre above it; that bar is ours now, so this is the only place it is said. */
  .wordmark {
    /* Pushes everything after it to the right — the drag target is this gap, which is most
       of the bar. */
    margin-right: auto;
    font-size: 0.82rem;
    letter-spacing: 0.02em;
    color: var(--text-dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    /* The bar is short, and a 2rem toggle beside a baseline-aligned label needs the label to
       centre with it rather than sit on its own baseline. */
    line-height: 1;
  }


  main {
    flex: 1;
    min-width: 0;
    /* No measure of its own: the page fills the window. It had a Column width setting, which
       every reader pushed to its top; the pages that read better narrow (Glossary, Settings)
       set their own. */
    padding: var(--gap-lg) var(--gap-lg) var(--gap);
    display: flex;
    flex-direction: column;
    /* These are the page's sections — the box you type in, the controls, the passage,
       the study area. `--gap-sm` is a within-a-group step and read as cramped between
       things that are not in a group at all. */
    gap: var(--gap);
    position: relative;
    z-index: 1;
  }

  header {
    display: flex;
    /* Centred, not baseline: the heading is out of the flow and the toggles are icons
       with no text baseline to align to. */
    align-items: center;
    /* `flex-end`, not `space-between`: the heading is out of the flow now, so with one
       child left space-between would slide the toggles to the left. */
    justify-content: flex-end;
    gap: var(--gap);
    flex-wrap: wrap;
  }
  /* `.name` is `.offscreen` in theme.css now — the reference page's heading wants the
     same treatment, and a byte-identical scoped copy is how two of them come to differ. */
  .toggles {
    display: flex;
    align-items: center;
    /* Tight: two icon buttons read as one pair of related controls, where a full gap
       would space them as two unrelated ones. */
    gap: var(--gap-xs);
  }
  /* The same shape and the same "this is on" treatment the rail's buttons use, so the
     app has one visual language for an icon that toggles rather than two. */
  .toggle {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    /* The browser's own `button` padding is `1px 6px` and does not scale — see the same reset on
       `.view` in `Sidebar.svelte`, which is where this was diagnosed. `box-sizing` is `border-box`,
       so at a 12px interface these 24px buttons offered their icon 10px of the 17px it wanted; it
       overflowed a track that starts 7px in and sat hard against the right edge. A fixed square
       with a centred glyph has no use for padding. */
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    color: var(--text-faint);
    cursor: pointer;
    transition:
      color var(--anim) var(--ease),
      background var(--anim) var(--ease),
      border-color var(--anim) var(--ease);
  }
  /* **In `rem`, because the button they sit in is.** These were `width="17"` and `width="13"`
     attributes: device pixels, fixed, while the button around them followed the interface size. At
     the smallest setting the icon filled its button; at the largest it was lost in the middle of
     one. The ratio is the thing that must hold, so the size is expressed in the same unit as the
     box. The small pair are the frame controls, drawn on a 16 viewBox and deliberately denser. */
  .toggle svg {
    width: 1.15rem;
    height: 1.15rem;
  }
  .toggle.small svg {
    width: 0.85rem;
    height: 0.85rem;
  }
  .toggle:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .toggle.on {
    color: var(--accent);
    background: var(--accent-wash);
    border-color: var(--accent-edge);
  }
  /* Switched off, in the attention colour and in the same shape `.on` uses for the accent,
     so the two states read as one control rather than as two designs.

     Red is what `theme.css` reserves for attention, and this is that meaning rather than a
     third one: the overlay being off also means two global keys silently do nothing, and it
     survives a restart. It sits in the header, well clear of the error line under the input
     box, which is the separation that rule asks for. The bar through the icon carries the
     same state for anyone who cannot see the colour. */
  .overlay.off {
    color: var(--red);
    background: var(--red-wash);
    border-color: var(--red-edge);
  }
  .overlay.off:hover {
    color: var(--red);
    background: var(--red-wash);
  }
  /* Sized to sit in the same 2rem square the drawn icons do, and set in the interface's
     Japanese face rather than whatever the body font falls back to for kana. */
  .kana {
    font-family: var(--font-jp);
    font-size: 0.95rem;
    line-height: 1;
  }

  /* The last control on its row, so a centred tip would hang off the edge. */
  .toggles .toggle:last-child::after {
    left: auto;
    right: 0;
    translate: 0 0;
  }

  .input {
    display: flex;
  }
  /* The successful source becomes one quiet row. The canonical text gets every spare pixel;
     actions remain named and reachable, but no longer compete with the passage itself. */
  .source-bar {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    min-width: 0;
    padding: 0.28rem 0.35rem 0.28rem var(--gap-sm);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .source-name {
    flex: 0 0 auto;
    font-size: 0.65rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-faint);
  }
  /* A real rule instead of a text pipe: it stays centred at every font scale, follows the
     theme's border colour, and remains presentation-only to assistive technology. */
  .source-divider {
    flex: 0 0 auto;
    height: 1em;
    border-inline-start: 1px solid var(--border-strong);
  }
  .source-text {
    flex: 1 1 auto;
    min-width: 4rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-jp);
    color: var(--text-dim);
  }
  .source-action {
    flex: 0 0 auto;
    padding: 0.2rem 0.55rem;
    font-size: 0.72rem;
  }
  /* Borderless on purpose. The row's whole job is to hand the passage the pixels, so the one
     action that is always in it stays quiet until reached for. Same shape and the same
     faint-then-lit treatment as the header's `.toggle`, so a transparent icon button looks the
     same wherever it appears rather than becoming a second dialect.

     Sized to the bordered `.source-action` beside it, so a visible "Fit passage" does not sit a
     pixel taller than this and re-open the row.

     `--radius-sm` rather than `--radius`: that is the radius the global focus ring sets *on the
     element*, so matching it is what stops tabbing here reshaping the button — see
     `local documentation`. */
  .source-edit {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 1.5rem;
    height: 1.5rem;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-faint);
    cursor: pointer;
    transition:
      color var(--anim) var(--ease),
      background var(--anim) var(--ease);
  }
  .source-edit:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .source-edit svg {
    width: 15px;
    height: 15px;
  }
  textarea {
    flex: 1;
    resize: vertical;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.7rem 0.8rem;
    font-family: var(--font-jp);
    font-size: 1.15rem;
    line-height: 1.6;
  }
  textarea::placeholder {
    color: var(--text-faint);
  }
  .actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--gap-sm);
    flex-wrap: wrap;
  }
  .action-primary,
  .action-trailing {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    min-width: 0;
  }
  .action-trailing { margin-left: auto; }
  /* Size only — surface, edge, colour and motion come from `.control`. The radius stays
     local and larger on purpose: these are the biggest buttons in the app, and the tighter
     corner read as a different control from the same family. */
  .actions button {
    border-radius: var(--radius);
    padding: 0.34rem 0.85rem;
    font-size: 0.85rem;
  }
  /* A word, and a spinner beside it while it works. `inline-flex` centres the two on one row
     and `gap` is the space between them, since flex collapses the whitespace the markup used to
     rely on. The explicit height is what keeps it level with the square Clear button: sized by
     its padding alone this comes out about 3px shorter, and the row steps. */
  .actions .control.primary {
    display: inline-flex;
    align-items: center;
    gap: var(--gap-xs);
    height: 2.1rem;
  }
  .translation-action.active {
    color: var(--accent);
    border-color: var(--accent-edge);
    background: var(--accent-wash);
  }
  .shortcut {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--text-faint);
    /* `.actions` wraps, and "Loading PaddleOCR-VL…" is longer than anything this element has
       ever held — long enough to break the trailing cluster onto its own line mid-capture,
       which moves the whole row while the reader is watching it. */
    white-space: nowrap;
  }
  /* One cell, two faces, the same shape `WordHeader` uses. The stack makes
     the element as wide as its wider face, so the row does not step as the text turns over.
     `var(--anim)` and not a literal: this is a finite transition a timer triggers, so at zero
     the two simply exchange places, which is what turning animation off should mean. */
  .shortcut.stacked {
    display: inline-grid;
    align-items: center;
    /* **Anchored to the right edge, not the left.** The cell is as wide as the wider face, so
       left-aligned the shorter one starts where the longer one does and stops short — leaving a
       gap that opens and closes as the two turn over. This row is already flush right against
       the window, and text that grows leftwards from a fixed edge reads as one thing changing
       rather than two things of different lengths. */
    justify-items: end;
  }
  .shortcut.stacked .face {
    grid-area: 1 / 1;
    /* **A multiple of the token, never a literal.** `tokens.md` forbids a hardcoded duration
       because it opts the element out of both the animation setting and the reader's operating
       system; scaling the token keeps both — `calc(0ms * 1.15)` is still `0ms`, so switching
       animation off still swaps the faces instantly.
       Slower than the app's other transitions on purpose: those answer a click, where a delay
       reads as lag. This one is unprompted, and something moving in the corner of the eye wants
       to be gentler than something the reader just asked for. */
    transition: opacity calc(var(--anim) * 1.15) var(--ease);
  }
  .shortcut.stacked .face.off {
    opacity: 0;
  }
  /* A read is the one thing here that takes long enough to wonder about. */
  .shortcut.working {
    color: var(--accent);
  }
  /* The whole passage/selected-word region owns the remaining Analysis-page height. Its inline
     size is the responsive authority because the rail, the padding and the Interface size all
     sit between it and the window. */
  .analysis-workspace {
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    container: analysis / inline-size;
  }
  .passage-column {
    min-width: 0;
  }
  /* The study area under the passage owns the pane's exact height, but its cards do not have to
     spend it. Short answers stop at their content; long answers shrink against this boundary and
     hand the excess to their internal scrollports. */
  .study {
    display: flex;
    flex-direction: column;
    gap: var(--gap-sm);
    height: 100%;
    min-height: 0;
  }
  /* Identity and both kinds of explanation are one selected-word surface. `auto` is the natural
     content height, while shrink plus the parent's finite height provides the cap: short entries
     leave honest workspace below them and long entries keep the header fixed over independent
     detail scrollports. Do not turn the grow factor back on merely to occupy empty space. */
  .selected-word {
    flex: 0 1 auto;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .study-detail {
    flex: 0 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 0 var(--gap);
    /* For `.reset-details` alone. */
    position: relative;
  }

  /* **Out of the flow, so opening a disclosure cannot change anyone's height.**

     This sat in the identity strip's message row, which meant the strip gained a line the
     moment any detail was opened and lost it again on reset — the one part of the card that
     should never move, moving because of something happening below it.

     The headings row already had the room: two centred tongues with empty space at both ends.
     Absolute rather than a flex item because the row is a `Split` in the wide layout and a
     tablist in the narrow one, and this belongs to neither — it answers the panes as a pair.

     `right: var(--gap)` rather than `0`: the containing block is the padding box, so zero would
     put it past this element's own inset and out of line with the panes. The vertical padding
     is `.pane-name`'s, which is what puts its text on the same line as the headings. */
  /* The parts row, between the word's identity and the panes describing it.

     Wraps rather than scrolls: a compound with several parts is short, and a row that scrolled
     would hide a part behind a gesture — which is the thing this slice exists to undo. The
     buttons carry `lang="ja"` in the markup so the Japanese face is chosen, which is why nothing
     here sets a font. */
  .word-parts {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--gap-xs) var(--gap-sm);
    padding: var(--gap-xs) var(--gap) var(--gap-sm);
  }

  .parts-label {
    color: var(--text-dim);
    font-size: 0.85em;
  }

  .part {
    padding: 0.15rem var(--gap-sm);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }

  .part:hover {
    border-color: var(--accent);
  }

  .reset-details {
    position: absolute;
    top: 0;
    right: var(--gap);
    z-index: 1;
    padding: var(--gap-sm) 0;
    border: 0;
    background: none;
    color: var(--text-faint);
    font: inherit;
    font-size: 0.72rem;
    cursor: pointer;
  }
  .reset-details:hover {
    color: var(--text);
  }

  /* At a narrow study measure, one readable section replaces two cramped ones. These are tabs by
     semantics and a restrained segmented control by appearance; the identity header above them
     stays shared, so switching never loses the selected word or its actions. */
  .panel-switch {
    flex: 0 0 auto;
    display: grid;
    grid-template-columns: 1fr 1fr;
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--gap-sm);
  }
  .panel-switch button {
    padding: var(--gap-sm);
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--text-faint);
    font: inherit;
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .panel-switch button:hover {
    color: var(--text);
    background: var(--surface-sunken);
  }
  .panel-switch button.current {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }

  /* A column is a fixed heading over a scrolling body, rather than one scrolling box
     with the heading stuck to its top. Same result for the heading — it never moves —
     but the scrollbar now belongs to the content alone and starts beneath the rule,
     level with what it scrolls, instead of running the full height of the column and
     past a heading that never moves anyway. */
  .column {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  /* There was a `.column-body.reading` floor here, guarding the passage against the
     comparison strip that used to sit beside it in this column — `min-height: 0` lets a flex
     item shrink to nothing when a sibling wants the room, and measured, a 161px strip crushed
     the passage to 12px. The strip is in the study half now and the reader has no sibling at
     all, so the guard has nothing left to guard.
     **It could not simply be left in place.** It was a constant, 5.5rem, and `--reading-floor`
     is a `calc()` that follows the reading-scale slider down to 0.6. With the band gone the
     floor is 3.8rem at the small end — under the constant — so a stale guard would have
     demanded more height than the pane it sits in was allowed to have. See `traps.md`. */
  .column-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    /* Room between the text and the bar, and the gutter held whether or not the bar is
       showing — otherwise the measure jumps the moment a panel grows past a screenful. */
    scrollbar-gutter: stable;
    padding-right: var(--gap-sm);
  }

  /* **The headings row, and it is `.panel-switch` under another name.**

     The narrow layout already draws this: uppercase label, accent underline, a hairline under
     the row. Four other shapes were tried here first — a full-width rule, a label-width rule,
     and sunken tongues flowing out of the strip above — and the argument against all of them is
     the same. They were new. This one is already in the app, twenty lines down, and the two
     layouts now differ only where they have to.

     The row carries the hairline; the label carries the accent. That stacking is what makes a
     tab strip read as one: 2px of accent directly over 1px of border, exactly as
     `.panel-switch button.current` sits on `.panel-switch`. */
  .pane-head {
    flex: 0 0 auto;
    display: flex;
    justify-content: center;
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--gap-sm);
  }

  /* **Both headings are marked, and that is the one place this departs from the tab strip.**

     There, the accent means *selected*, because only one panel is on screen. Here both are, so
     there is no selection to mark and marking one would be a lie about the other. Both live,
     both accented.

     The underline is the label's width rather than the column's — `.panel-switch` fills its grid
     cell, but two full-width accents side by side would meet across the splitter and read as one
     long line drawn under the whole card rather than as two headings.

     Type is copied from `.panel-switch button` so the two cannot drift apart on a later edit. */
  .pane-name {
    margin: 0;
    padding: var(--gap-sm) var(--gap);
    border-bottom: 2px solid var(--accent);
    text-align: center;
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--accent);
  }

  /* Banners. Red for a failure, purple-grey for a caveat — never both shouting. */
  .error,
  .warning {
    border-radius: var(--radius);
    padding: 0.45rem 0.7rem;
    font-size: 0.85rem;
    margin: 0;
    border: 1px solid;
  }
  .error {
    color: var(--red);
    background: var(--red-wash);
    border-color: var(--red-edge);
  }
  .warning {
    color: var(--text-dim);
    background: var(--surface);
    border-color: var(--border-strong);
  }
  /* Which analyzer read the passage, when it was not the one asked for. The prominent case wears
     `.warning`; these add only the spacing above the passage and the small disclosure. */
  .engine-notice {
    margin: 0 0 0.5rem;
  }
  .engine-notice details {
    margin-top: 0.25rem;
    font-size: 0.8rem;
  }
  .engine-notice summary {
    cursor: pointer;
  }
  .engine-notice details p {
    margin: 0.25rem 0 0;
    overflow-wrap: anywhere;
  }
  .engine-quiet {
    margin: 0 0 0.35rem;
    color: var(--text-faint);
    font-size: 0.8rem;
  }
  /* Provenance for the selected word, in the same voice as the quiet engine label: a fact about
     where the word came from, not something to attend to. */
  .repair-note {
    margin: 0;
    color: var(--text-faint);
    font-size: 0.8rem;
  }
  /* A footnote to the warning above it, not a third banner. No border and no ground: those
     are what make the two above read as things to attend to, and this one is a confirmation
     that something already explained has just happened again. */
  .save-notice {
    margin: 0;
    padding: 0 0.7rem;
    color: var(--text-faint);
    font-size: 0.8rem;
  }
  /* Takes the leftover height so the footer is pushed to the bottom of the window
     instead of floating halfway up it with black underneath. */
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
    text-align: center;
  }
  .hint {
    color: var(--text-dim);
    font-size: 0.9rem;
    margin: 0;
    max-width: 28rem;
  }

  footer {
    /* Belt and braces with `.empty`: whatever is above it, the footer sits at the
       bottom edge. */
    margin-top: auto;
    border-top: 1px solid var(--border);
    padding-top: var(--gap-xs);
    display: flex;
    gap: var(--gap);
    flex-wrap: wrap;
    align-items: baseline;
  }
  .credit {
    font-size: 0.72rem;
    color: var(--text-faint);
    cursor: help;
  }
  .progress {
    font-size: 0.72rem;
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
  }
</style>
