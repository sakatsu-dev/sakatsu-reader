<script lang="ts">
  import Updates from "./Updates.svelte";
  import type { UpdateStatus } from "../bindings";
  import BindRecorder from "./BindRecorder.svelte";
  import IchiranSetup from "./IchiranSetup.svelte";
  import OcrSelect from "./OcrSelect.svelte";
  import VlmSetup from "./VlmSetup.svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { humanBytes, setup as setupStore } from "./setup-store.svelte";
  import { displayPath } from "./display-path.js";
  import { fetching, humanSize, share } from "./fetching.svelte";

  import { copyName, exportTarget, manageableDecks, offerableDecks } from "./decks.js";
  import {
    KIND_HUES,
    PRESETS,
    deriveTokens,
    freePresetName,
    matchPalette,
    palettesFor,
    resolveMode,
    themeWarnings,
  } from "./theme.js";
  import type {
    AnalyzerChoice,
    Binding,
    DeckInfo,
    DecorTint,
    LogFile,
    CorpusFile,
    NamedTheme,
    OcrChoice,
    ThemeDecor,
    ThemeSettings,
    WarmupDiagnostics,
    RuntimeStatus,
  } from "../model";
  import { ANALYZER_OPTIONS, SETTINGS_ABOUT, SETTINGS_SECTIONS, engineLabel } from "../model";
  import { SECTION_ICONS } from "./settings-icons.js";
  import { engineRow, isLoopback, resultPhrase, statusLabel } from "./runtime-status.js";
  import { hasSavedPartial, savedPhrase } from "./download-state.js";
  // The search index, and where each row lives. Out of this file so a test can check that
  // what it claims and what the markup renders are the same thing — see `settings-index.js`.
  import { FIND, anyMatch, matches, search } from "./settings-index.js";
  import type { DictionaryUpkeep } from "../model";
  import { errorMessage } from "./save-notice.js";
  import PalettePicker from "./PalettePicker.svelte";
  import { WORD_KIND_LABELS } from "./word-kinds.js";

  // Settings mounts only after the setup snapshot is loaded. Capture its current location once:
  // tracking and assigning the same state in an effect can create a render loop in Svelte 5.
  let moveResourcesTo = $state(setupStore.snapshot?.storage.target ?? "");
  let removeOldResources = $state(false);
  let moveResourcesResult = $state("");
  let movingResources = $state(false);

  async function moveResources() {
    if (!moveResourcesTo.trim() || movingResources) return;
    movingResources = true;
    moveResourcesResult = "";
    try {
      moveResourcesResult = await setupStore.moveResources(
        moveResourcesTo.trim(),
        removeOldResources,
      );
    } catch (thrown) {
      moveResourcesResult = String(thrown);
    } finally {
      movingResources = false;
    }
  }

  async function browseResourceLocation() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choose where Sakatsu stores downloaded resources",
    });
    if (typeof selected === "string") moveResourcesTo = selected;
  }

  /// A double-tap window the store will actually keep.
  ///
  /// `Settings::lock_tap` in `core/src/store/mod.rs` is the authority: zero means off, and
  /// anything else is clamped to 150–1000. A slider that offers values in between shows one
  /// number and the app behaves as another, so the gap is closed here rather than displayed.
  ///
  /// Halfway is the parting line — under 75 reads as reaching for off, over it as reaching for
  /// the shortest window that works.
  function snapLockTap(ms: number): number {
    if (!Number.isFinite(ms)) return 0;
    return ms < 75 ? 0 : Math.max(150, Math.min(1000, ms));
  }

  /// Bytes as something a person can judge at a glance.
  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} kB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  // The settings page: a nav down the left, one section at a time beside it.
  //
  // **It was a drawer that slid over the reader with all its sections stacked in it, folded
  // shut.** Which meant the panel had to carry, and keep working, a slide animation, a
  // drag-to-resize grip, a click-away that had to tell "outside" from "the gear that opened me",
  // and a per-section fold remembered in the store — four mechanisms whose whole job was making a
  // long scroll bearable. A page needs none of them: the nav is how you get past a section, and
  // the section you are on is the only one rendered.
  //
  // The ~45 bindings did not change in that move. They are what this component is; the shell was
  // the accident.

  let {
    backTo,
    onexit,
    dictionaryUpkeep = null,
    upkeepState = "loading",
    upkeepError = "",
    updateDictionaryIndexes,
    retryDictionaryUpkeep,
    vlmEndpoint = $bindable(),
    vlmModel = $bindable(),
    keepLineBreaks = $bindable(),
    markReadingGuesses = $bindable(),
    showWordParts = $bindable(),
    markSeenOnFocus = $bindable(),
    focusBeforeOpen = $bindable(),
    tabNavigation = $bindable(),
    fontSize = $bindable(),
    readerScale = $bindable(),
    panelScale = $bindable(),
    quiet = $bindable(),
    ichiranServer = $bindable(),
    checkUpdatesOnStartup = $bindable(true),
    oninstallupdate,
    logging = $bindable(),
    collectSentences = $bindable(),
    holdMs = $bindable(),
    lockTapMs = $bindable(),
    clickDelayMs = $bindable(),
    section = $bindable("reading"),
    showSourceAnalysis = $bindable(),
    showSourceReference = $bindable(),
    frequencySource = $bindable(),
    frequencyDirection = $bindable(),
    showFrequencySource = $bindable(),
    decorCloud = $bindable(),
    decorCloudTint = $bindable(),
    decorCloudOn = $bindable(),
    decorSakura = $bindable(),
    decorSakuraTint = $bindable(),
    decorSakuraOn = $bindable(),
    decorScale = $bindable(),
    decorCloudPad = $bindable(),
    decorCloudSpread = $bindable(),
    decorSakuraSpread = $bindable(),
    decorBlur = $bindable(),
    decorCloudBlur = $bindable(),
    decorSakuraBlur = $bindable(),
    frameTint = $bindable(),
    hideDuringCapture = $bindable(),
    animations = $bindable(),
    animationMs = $bindable(),
    hideSetup: _hideSetup = $bindable(),
    analyzer = $bindable(),
    analyzerEngine = "",
    ocr = $bindable(),
    preprocessTighten = $bindable(),
    preprocessContrast = $bindable(),
    preprocessUpscale = $bindable(),
    ankiDeck = $bindable(),
    ankiOurDecksOnly = $bindable(),
    theme = $bindable(),
    themePresets = $bindable(),
    decks,
    deckInfo,
    ondeckaction,
    ocrEngine,
    logFile,
    corpusFile,
    warmupDiagnostics,
    runtimeStatus,
    runtimeStatusFailed,
    ondiagnosticreport,
    oncopyreport,
    onunload,
    onreveallog,
    onrevealcorpus,
    captureBind,
    overlayBind,
    switchBind,
    onCaptureBind,
    onOverlayBind,
    onSwitchBind,
    onrestart,
    onpalette,
    onadopt,
  }: {
    keepLineBreaks: boolean;
    markReadingGuesses: boolean;
    showWordParts: boolean;
    markSeenOnFocus: boolean;
    /** Two clicks to open a result card — focus, then open — rather than a tab revealed on hover. */
    focusBeforeOpen: boolean;
    /** Restore conventional Windows Tab traversal and click focus throughout the app. */
    tabNavigation: boolean;
    fontSize: number;
    readerScale: number;
    panelScale: number;
    quiet: boolean;
    ichiranServer: boolean;
    checkUpdatesOnStartup?: boolean;
    oninstallupdate: () => Promise<UpdateStatus>;
    logging: boolean;
    collectSentences: boolean;
    holdMs: number;
    lockTapMs: number;
    clickDelayMs: number;
    /**
     * Which section is on screen, by id.
     *
     * Bound, because it outlives the page: the title bar's menu sets it to jump straight to a
     * section, and it is remembered across a restart. An id nothing here recognises shows the
     * nav with nothing selected rather than an empty page — see `current` below.
     */
    section?: string;
    /** Name the dictionary behind each definition, on the analysis page. */
    showSourceAnalysis: boolean;
    /** The same on the reference page — separate, because a search repeats it. */
    showSourceReference: boolean;
    frequencySource: string;
    frequencyDirection: "rank" | "count";
    showFrequencySource: boolean;
    decorCloud: number;
    decorCloudTint: DecorTint;
    /** Whether the clouds are drawn; off keeps everything else about them. */
    decorCloudOn: boolean;
    decorSakura: number;
    decorSakuraTint: DecorTint;
    decorSakuraOn: boolean;
    decorScale: number;
    decorCloudPad: number;
    decorCloudSpread: number;
    decorSakuraSpread: number;
    decorBlur: boolean;
    decorCloudBlur: number;
    decorSakuraBlur: number;
    frameTint: number;
    hideDuringCapture: boolean;
    animations: boolean;
    animationMs: number;
    hideSetup: boolean;
    analyzer: AnalyzerChoice;
    /// The engine actually running, which Automatic resolves to one of the others. Named
    /// because "Automatic" alone does not say whether Ichiran answered.
    analyzerEngine?: string;
    ocr: OcrChoice;
    preprocessTighten: boolean;
    preprocessContrast: boolean;
    preprocessUpscale: boolean;
    ankiDeck: string;
    /** Narrow the list to decks that already hold 屠つ cards. */
    ankiOurDecksOnly: boolean;
    /**
     * The palette, as inputs. Only `mode` has a control for now; the two hues stay at the values
     * the app was built with until there is an interface for them.
     */
    theme: ThemeSettings;
    /** The learner's own palettes. The shipped ones are code and are not in here. */
    themePresets: NamedTheme[];
    decks: string[];
    /** What is in each deck — only populated while the settings are open. */
    /// What is in each deck. `null` until it has been read once, which is not the same as an
    /// empty list — see `offerableDecks`.
    deckInfo: DeckInfo[] | null;
    /** Run a deck command and re-read. Resolves to an error to show, or null. */
    ondeckaction: (command: string, args: Record<string, string>) => Promise<string | null>;
    ocrEngine: string;
    logFile: LogFile | null;
    corpusFile: CorpusFile | null;
    warmupDiagnostics: WarmupDiagnostics | null;
    /// What is running and what each operation last used. `null` while it is being read.
    runtimeStatus: RuntimeStatus | null;
    /// Whether reading it failed, which is not the same as not having read it yet.
    runtimeStatusFailed: boolean;
    /// Build the bug-report text. The backend assembles it from a fixed list of facts.
    ondiagnosticreport: () => Promise<string>;
    /// Copy text without the clipboard watcher reading it back as a passage.
    oncopyreport: (text: string) => Promise<unknown>;
    /// Release a loaded model from memory. Resolves to whether anything was loaded.
    onunload: (role: "recognition" | "translation") => Promise<boolean>;
    onreveallog: () => void;
    onrevealcorpus: () => void;
    captureBind: Binding;
    overlayBind: Binding;
    switchBind: Binding | null;
    onCaptureBind: (bind: Binding | null) => Promise<string>;
    onOverlayBind: (bind: Binding | null) => Promise<string>;
    onSwitchBind: (bind: Binding | null) => Promise<string>;
    /// What the page the reader came from is called, so the way back can name it.
    backTo: string;
    /// Leave settings and go back to that page.
    onexit: () => void;
    /// Where the vision model is served, written by its setup and editable by hand.
    vlmEndpoint: string;
    vlmModel: string;
    onrestart: () => void;
    /// Apply a palette by name — see `applyPreset`.
    onpalette: (name: string) => void;
    /// Choose the analyzer again, for when Ichiran has just been set up.
    onadopt: () => void;
    /// Which secondary indexes each installed dictionary has.
    dictionaryUpkeep?: DictionaryUpkeep[] | null;
    /// Whether that status is still being read, was read, or could not be.
    upkeepState?: "loading" | "ready" | "failed";
    /// Why it could not be read, in the shared formatter's words.
    upkeepError?: string;
    /// Read it again, for when it could not be.
    retryDictionaryUpkeep?: () => Promise<void>;
    /// Rebuild the missing ones, returning what was built. Rejects on failure *and* on refusal,
    /// which is why both are reported with the text the backend gave.
    updateDictionaryIndexes?: () => Promise<string>;
  } = $props();

  /// Dictionaries still missing a secondary index, and what rebuilding would gain.
  ///
  /// **Offered on its own terms, not on English search's.** The only existing prompt appears when
  /// `needs_gloss_index` is true, and a dictionary with glosses reports ready however many other
  /// buckets it lacks — so a reader whose English search works has no way to learn that words
  /// spelled as cross-references show an arrow where a definition belongs, and no way to fix it.
  /// This names the dictionary, what is missing, and what it costs.
  const upkeepPending = $derived(
    (dictionaryUpkeep ?? []).filter((entry) => entry.needs_indexes),
  );
  /// Something to list, as opposed to *no dictionary installed* or *no status yet*.
  const upkeepKnown = $derived(
    upkeepState === "ready" && dictionaryUpkeep !== null && dictionaryUpkeep.length > 0,
  );
  /// The Interface size under the pointer while the slider is dragged, applied on release.
  let fontSizeDraft = $state<number | null>(null);
  /// Names for every engine the status mentions — analyzers and translator as well as OCR.
  const statusName = statusLabel(engineLabel);
  /// The bug report: built on request, previewed, then copied. Each preview is numbered so a slow
  /// one that answers after a newer one cannot replace it.
  let report = $state("");
  let reportState = $state<"idle" | "loading" | "ready" | "failed">("idle");
  let reportError = $state("");
  let reportCopied = $state(false);
  let reportGeneration = 0;

  async function previewReport() {
    if (reportState === "loading") return;
    const generation = ++reportGeneration;
    reportState = "loading";
    reportCopied = false;
    try {
      const built = await ondiagnosticreport();
      if (generation !== reportGeneration) return;
      report = built;
      reportState = "ready";
    } catch (thrown) {
      if (generation !== reportGeneration) return;
      reportError = errorMessage(thrown);
      reportState = "failed";
    }
  }

  async function copyReport() {
    reportCopied = false;
    try {
      await oncopyreport(report);
      reportCopied = true;
    } catch (thrown) {
      reportError = errorMessage(thrown);
      reportState = "failed";
    }
  }

  /// What the last unload did, or why it could not.
  let unloadMessage = $state("");
  let unloadFailed = $state(false);

  async function unload(role: "recognition" | "translation") {
    try {
      const released = await onunload(role);
      unloadFailed = false;
      const name = role === "recognition" ? "Recognition" : "Translation";
      unloadMessage = released
        ? `${name} unloaded. It loads again the next time it is used.`
        : `${name} had nothing loaded.`;
    } catch (thrown) {
      unloadFailed = true;
      unloadMessage = errorMessage(thrown);
    }
  }

  let updatingIndexes = $state(false);
  let indexResult = $state("");
  let indexFailed = $state(false);

  async function updateIndexes() {
    if (updatingIndexes || !updateDictionaryIndexes) return;
    updatingIndexes = true;
    indexResult = "";
    indexFailed = false;
    try {
      // The command reports a refusal — another resource operation owns the dictionaries — as a
      // rejected promise, the same as a failure. Both are shown with the text the backend gave
      // and both leave the button live, because retrying is the answer to one and often to the
      // other.
      indexResult = (await updateDictionaryIndexes()) || "Indexes are already up to date.";
    } catch (thrown) {
      indexFailed = true;
      indexResult = errorMessage(thrown);
    } finally {
      updatingIndexes = false;
    }
  }

  // Whether a chord is being recorded, one flag per recorder. It used to also decide whether
  // Escape closed the drawer, since Escape is a bindable key. The page has nothing to close and
  // what is left is `recording`, which the three recorders read to stay out of each other's way.
  //
  // One flag each rather than a shared count: a count has to be incremented and decremented in
  // matching pairs, and the effect that reports these fires once on mount, which would put it out
  // by one before anything was pressed.
  // --- The palette -------------------------------------------------------------------
  //
  // A preset is a mode and two hues, so "which preset is this" is answerable by comparing rather
  // than by remembering — no stored pointer to go stale, and moving a slider drops to Custom on
  // its own without anything having to notice.

  /** Shown when the hues sit on no saved palette. Not a palette, and not selectable. */
  const CUSTOM = "custom";

  /** The three modes, in the order they are offered. System last: it hands the choice to Windows. */
  const MODES = [
    { id: "light", label: "Light" },
    { id: "dark", label: "Dark" },
    { id: "system", label: "System" },
  ] as const;

  /** The shipped palettes and the learner's, in one list to compare against. */
  const allPalettes = $derived(palettesFor(PRESETS, themePresets));

  // Shared with the quick appearance panel — see `matchPalette` for why the comparison lives in
  // `theme.js` rather than here, and which four values it reads.
  const matchedPreset = $derived(matchPalette(allPalettes, theme) ?? CUSTOM);

  const matchedPalette = $derived(
    allPalettes.find((palette: (typeof allPalettes)[number]) => palette.name === matchedPreset),
  );

  /**
   * What to say under the picker, or nothing.
   *
   * Three cases, and the middle one used to be lost: a shipped palette explains itself, a palette
   * you *made* has nothing to explain, and dials sitting on no saved palette at all need telling
   * how to keep them. Reading `?.about ?? fallback` in one step could not separate the last two —
   * a saved palette with no blurb would have been handed the unsaved one's sentence — which is why
   * the match is held here and asked two questions instead of one.
   */
  const presetAbout = $derived(
    matchedPalette
      ? (matchedPalette.about ?? "")
      : "Your own, from the two dials below. Save it to keep it.",
  );

  /** Whether what is on screen is one of the learner's, so it can be renamed or removed. */
  const mineSelected = $derived(matchedPalette?.mine ?? false);

  /**
   * The tokens offered for setting by hand.
   *
   * Colours only, and the ones somebody might reasonably want to move. The washes and edges are
   * left out on purpose: each is its parent at a fixed alpha, so setting one by hand is how a
   * palette starts disagreeing with itself.
   */
  /**
   * The colours a word can be drawn in, by what they *mean* rather than what they are called.
   *
   * The tokens are colour names — `blue`, `yellow`, `good`, `red` — because that is what they
   * were when the palette had four fixed status hues. The moment somebody sets New to orange,
   * a control labelled "blue" is absurd, so the label is the word state and the token is an
   * implementation detail this list joins to it.
   *
   * `selection` is the one that is not a status: it is where you are, not what you know. It is
   * here because this is the section about reading, and because detaching it from the accent is
   * exactly the same gesture as setting any of the four.
   */
  const WORD_COLOURS = [
    { token: "blue", label: "New" },
    { token: "yellow", label: "Seen" },
    { token: "good", label: "Known" },
    { token: "red", label: "Ignored" },
    { token: "selection", label: "Selected" },
  ];

  /**
   * The kinds that have a colour, labelled the way a sense list labels them.
   *
   * Built from `WORD_KIND_LABELS` so it cannot fall behind the list that decides what a sense *is*
   * — `Function` was added to that one and would have been missing here — and then **filtered by
   * what actually has a hue**, which is the half that was wrong first time round: that list ends
   * with `Other`, the filter row's catch-all for a sense no kind claims. `Other` has no
   * `--kind-other` because it is the absence of a classification rather than one of them, so it
   * arrived as a seventh swatch with no colour in it and a colour well that wrote a token nothing
   * reads. `KIND_HUES` is the authority on which kinds are coloured; this asks it.
   */
  const KIND_COLOURS = WORD_KIND_LABELS.filter(
    (label: string) => label.toLowerCase() in KIND_HUES,
  ).map((label: string) => ({ token: `kind-${label.toLowerCase()}`, label }));



  const OVERRIDABLE = [
    "bg",
    "surface",
    "surface-raised",
    "surface-hover",
    "surface-sunken",
    "border",
    "border-strong",
    "text",
    "text-dim",
    "text-faint",
    "accent",
    "accent-strong",
  ];

  /**
   * What the palette derives to right now, so a colour well opens on the colour it is replacing.
   *
   * `system` is resolved against the mode already painted onto the root rather than asked of the
   * desktop again — this only feeds the swatches, and a promise cannot be awaited in a `$derived`.
   */
  const paletteNow = $derived(
    deriveTokens(
      {
        mode: theme.mode,
        surfaceHue: theme.surface_hue,
        kindSpin: theme.kind_spin,
        accentHue: theme.accent_hue,
        tint: theme.tint,
        contrast: theme.contrast,
        depth: theme.depth,
        overrides: theme.overrides,
      },
      resolveMode(
        theme.mode,
        (document.documentElement.dataset.theme === "light" ? "light" : "dark"),
      ),
    ),
  );

  /** What is below a floor, reported rather than corrected. */
  const warnings = $derived(themeWarnings(paletteNow));

  function setToken(token: string, value: string) {
    theme = { ...theme, overrides: { ...theme.overrides, [token]: value } };
  }

  function clearToken(token: string) {
    const { [token]: _gone, ...rest } = theme.overrides;
    theme = { ...theme, overrides: rest };
  }

  /** Which palette action wants a name typed, if any. */
  let paletteMode = $state<null | "save" | "rename">(null);
  let paletteName = $state("");
  let paletteField = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (paletteMode) paletteField?.focus();
  });

  function startPalette(mode: "save" | "rename") {
    paletteMode = mode;
    // Renaming starts from the current name, because it is usually an edit of it rather than a
    // fresh thought.
    paletteName = mode === "rename" ? matchedPreset : "";
  }

  /// The decoration and motion on screen right now, as a palette would keep them.
  ///
  /// Read at save time rather than bound, because a palette is a snapshot: changing a slider
  /// afterwards should change the interface and not quietly rewrite a saved palette.
  function currentDecor(): ThemeDecor {
    return {
      cloud: decorCloud,
      cloud_tint: decorCloudTint,
      sakura: decorSakura,
      sakura_tint: decorSakuraTint,
      scale: decorScale,
      cloud_pad: decorCloudPad,
      cloud_spread: decorCloudSpread,
      sakura_spread: decorSakuraSpread,
      blur: decorBlur,
      cloud_blur: decorCloudBlur,
      sakura_blur: decorSakuraBlur,
      animations,
      animation_ms: animationMs,
    };
  }

  /**
   * Whether the typed name already belongs to something, and what can be done about it.
   *
   * Two different answers, because the two kinds of palette are not equally yours:
   *
   * - **one of yours** — saving replaces it, which is what "overwrite" means, and the warning
   *   exists so that is a decision rather than a discovery;
   * - **one that ships** — those stay untouchable, so the name is taken and cannot be had. The
   *   save still happens under a free name rather than being refused, because refusing a name is
   *   a wall and offering a copy is a door.
   */
  const paletteClash = $derived.by(() => {
    const wanted = paletteName.trim().toLowerCase();
    if (!wanted || paletteMode === null) return null;
    // Renaming to the name it already has is not a clash with itself.
    if (paletteMode === "rename" && wanted === matchedPreset.trim().toLowerCase()) return null;
    if (PRESETS.some((preset) => preset.name.toLowerCase() === wanted)) return "shipped";
    if (themePresets.some((saved: NamedTheme) => saved.name.toLowerCase() === wanted)) {
      return "mine";
    }
    return null;
  });

  function commitPalette() {
    const wanted = paletteName.trim();

    // Replacing one of the learner's own, which is the case `freePresetName` used to turn into
    // "Dusk 2" — a second entry nobody asked for, and no way to say "no, that one".
    if (paletteClash === "mine") {
      const at = themePresets.findIndex(
        (saved: NamedTheme) => saved.name.toLowerCase() === wanted.toLowerCase(),
      );
      const kept: NamedTheme =
        paletteMode === "save"
          ? { name: themePresets[at].name, theme: { ...theme }, decor: currentDecor() }
          : { ...themePresets[at], name: themePresets[at].name };
      themePresets = themePresets.map((saved: NamedTheme, i: number) => (i === at ? kept : saved));
      if (paletteMode === "rename") {
        // A rename onto an existing name is a merge: the palette being renamed takes that
        // entry's place, and the entry it replaced is gone.
        themePresets = themePresets.filter(
          (saved: NamedTheme, i: number) => i === at || saved.name !== matchedPreset,
        );
      }
      paletteMode = null;
      paletteName = "";
      return;
    }

    const taken = allPalettes
      .map((palette: (typeof allPalettes)[number]) => palette.name)
      .filter((name: string) => paletteMode !== "rename" || name !== matchedPreset);
    const name = freePresetName(taken, paletteName);

    if (paletteMode === "save") {
      themePresets = [...themePresets, { name, theme: { ...theme }, decor: currentDecor() }];
    } else if (paletteMode === "rename") {
      themePresets = themePresets.map((saved: NamedTheme) =>
        saved.name === matchedPreset ? { ...saved, name } : saved,
      );
    }
    paletteMode = null;
    paletteName = "";
  }

  /**
   * Forget a saved palette.
   *
   * **Nothing on screen changes.** Applying a palette copies its values into `theme`, so the live
   * look never pointed at this entry — removing it takes the row out of the list and leaves the
   * colours exactly where they are. The list simply falls to *Custom*, which is the truth.
   */
  function forgetPalette() {
    themePresets = themePresets.filter((saved) => saved.name !== matchedPreset);
  }

  /**
   * Take a preset's values as the live theme.
   *
   * **A copy, never a reference.** Nothing afterwards points back at the preset it came from, so
   * moving a slider changes only what is on screen — and a preset can never be edited by
   * accident, which is what would make the shipped list untrustworthy.
   */
  /**
   * Take a preset's values as the live look.
   *
   * **The doing is `App.svelte`'s**, because it owns the theme *and* the thirteen decoration values
   * a palette can carry, and this used to be one of two copies — the title bar's menu had a shorter
   * one that applied the colours and dropped the decoration. Same palette, two results. What is
   * left here is the one thing that is this page's own: closing the name field, which the menu has
   * no equivalent of.
   */
  function applyPreset(name: string) {
    onpalette(name);
    paletteMode = null;
  }

  /// Whether either watermark is on screen, which is when the settings they share mean anything.
  const showsDecor = $derived(
    (decorCloudOn && decorCloud > 0) || (decorSakuraOn && decorSakura > 0),
  );

  let recordingCapture = $state(false);
  const ichiranOffer = $derived(fetching.entries.find((entry) => entry.id === "ichiran"));
  /** The same, for the vision reader: its size belongs to the catalog, not to a button label. */
  const vlmOffer = $derived(fetching.entries.find((entry) => entry.id === "paddle-vl"));
  const optionalOffers = $derived(
    fetching.entries.filter((entry) =>
      ["translation", "pronunciation", "frequency-jpdb", "frequency-bccwj"].includes(entry.id),
    ),
  );

  // --- Anki decks ---------------------------------------------------------------------
  //
  // `deckInfo` arrives only while this panel is open, so every derivation here has to read
  // sensibly from an empty list: before it lands, nothing is filtered and nothing is managed.

  /** The list actually offered — see `offerableDecks`, which owns the rule and is tested. */
  const shownDecks = $derived(offerableDecks(decks, deckInfo, ankiOurDecksOnly));
  /// Where a card would go right now, or null when there is nowhere safe to send one.
  const target = $derived(exportTarget(ankiDeck, shownDecks));

  /**
   * Which deck the manage buttons act on — **its own choice, not the export target.**
   *
   * These were one control at first, and that was wrong: it meant renaming a deck required first
   * changing where your cards go, and leaving it changed. It also filtered the list of decks you
   * could manage by a rule written for choosing where to export.
   */
  let deckTarget = $state("");
  /** The decks these controls may act on — see `manageableDecks`, which owns the rule. */
  const manageDecks = $derived(manageableDecks(decks, deckInfo));
  // Keep the target real. Also how it recovers after a rename or a delete takes the name away,
  // and after the counts arrive and narrow the list under it.
  $effect(() => {
    if (manageDecks.length > 0 && !manageDecks.includes(deckTarget)) {
      deckTarget = manageDecks.includes(ankiDeck) ? ankiDeck : manageDecks[0];
    }
  });

  const selected = $derived((deckInfo ?? []).find((deck) => deck.name === deckTarget) ?? null);
  /**
   * Whether the target deck may be renamed or deleted here.
   *
   * Rust decides this too, and its answer is the one that counts — this is for drawing a
   * disabled button and saying why, not for keeping anyone safe.
   */
  const managed = $derived(
    selected !== null && selected.made_here && selected.foreign === 0 && deckTarget !== "Default",
  );

  /**
   * Which action wants a name typed, if any.
   *
   * Only creating and renaming do. Duplicating names the copy itself, and deleting has nothing to
   * name — so a text box on screen at all times was three-quarters of the time an invitation to
   * fill in a field that the button you wanted would ignore.
   */
  let deckMode = $state<null | "create" | "rename">(null);
  let deckName = $state("");
  let deckBusy = $state(false);
  let deckError = $state<string | null>(null);
  /** Delete asks twice. The second press is the one that acts. */
  let confirmingDelete = $state(false);
  let nameField = $state<HTMLInputElement | null>(null);

  const deleteLabel = $derived(
    `Delete ${selected?.ours ?? 0} ${selected?.ours === 1 ? "card" : "cards"}?`,
  );

  function startNaming(mode: "create" | "rename") {
    deckMode = mode;
    // Renaming starts from the current name, because a rename is usually an edit of it rather
    // than a fresh thought, and an empty box would make you retype what you are changing.
    deckName = mode === "rename" ? deckTarget : "";
    confirmingDelete = false;
    deckError = null;
  }

  function stopNaming() {
    deckMode = null;
    deckName = "";
  }

  function commitName() {
    if (!deckName.trim()) return;
    if (deckMode === "create") runDeck("anki_create_deck", { name: deckName });
    else if (deckMode === "rename")
      runDeck("anki_rename_deck", { from: deckTarget, to: deckName });
  }

  async function runDeck(command: string, args: Record<string, string>) {
    deckBusy = true;
    deckError = await ondeckaction(command, args);
    deckBusy = false;
    confirmingDelete = false;
    // A refusal keeps what was typed, so it can be corrected rather than typed again.
    if (!deckError) stopNaming();
  }

  // The field is only on screen while it is the thing to use, so it takes focus when it appears.
  $effect(() => {
    if (deckMode) nameField?.focus();
  });

  // Choosing a different deck abandons a half-pressed delete and any half-typed name, so neither
  // can be inherited by a deck the learner never armed them for.
  $effect(() => {
    void deckTarget;
    confirmingDelete = false;
    deckMode = null;
  });

  /// The section asked for, or `undefined` when this build does not have it.
  ///
  /// A settings file written by a newer version can name a section that does not exist here, and
  /// an unknown id would otherwise render a page with a nav and nothing beside it. The nav shows
  /// nothing selected, which is honest, and any click fixes it.
  ///
  /// **Undefined is a real value here, not an error to wish away.** The header reads its title and
  /// description from this, and does so only inside `{#if current}` — the unknown-section test in
  /// `settings-layout.spec.ts` is what keeps a read from slipping outside that guard.
  const current = $derived(SETTINGS_SECTIONS.find((entry) => entry.id === section));

  let find = $state("");
  const needle = $derived(find.trim().toLowerCase());
  /// Whether one row survives the search. An empty box matches everything.
  const hit = (row: { label: string; keywords: string }) => matches(row, needle);
  /// Whether a subheading has anything left under it — a divider with nothing beneath it
  /// reads as a section that failed to load rather than one that did not match.
  const some = (...rows: { label: string; keywords: string }[]) => anyMatch(needle, rows);

  /// What a search matches, across every section rather than the one on screen.
  const results = $derived(search(needle, SETTINGS_SECTIONS));

  /// Open the section a result is in, leaving the query alone.
  ///
  /// The query stays because the rows in that section are still filtered by it — landing on
  /// Capture with every row showing would make you hunt for the one you searched for, which is the
  /// work the search was doing.
  function jump(to: string) {
    section = to;
  }

  let recordingOverlay = $state(false);
  let recordingSwitch = $state(false);
  const recording = $derived(recordingCapture || recordingOverlay || recordingSwitch);

  /**
   * Escape leaves, the same way the corner button does.
   *
   * **Here rather than in `App.svelte`, and `recording` is the reason.** Escape is a bindable key:
   * the three chord recorders on this page are waiting for whatever you press, and a window-level
   * handler that did not know about them would close the page instead of recording the chord —
   * which is exactly why Escape was dropped when settings stopped being a drawer. The flag that
   * settles it lives here, so the handler does too, and nothing new has to be plumbed out.
   *
   * `defaultPrevented` because the same window sees the section menu's Escape first; whatever is
   * open on top of the page closes before the page itself does. `Caption.svelte` takes the same
   * shape and its comment gives the rest of the reasoning: a `keydown` on a plain wrapper wants an
   * ARIA role it has no honest one to give, and "Escape dismisses what is open" is a window-level
   * convention anyway. It costs nothing while shut — the guard is the first line.
   */
  function onWindowKey(event: KeyboardEvent) {
    if (event.key !== "Escape" || recording || event.defaultPrevented) return;
    event.preventDefault();
    onexit();
  }
</script>

<svelte:window onkeydown={onWindowKey} />

<!-- A section's icon in the nav. Decoration: the label beside it is the button's name, so the
     drawing is hidden from assistive technology and adds nothing to what the button is called.
     The paths and why they are centred where they are: `settings-icons.js`. -->
{#snippet sectionIcon(id: string)}
  <svg
    class="section-icon"
    viewBox="0 0 24 24"
    aria-hidden="true"
    fill="none"
    stroke="currentColor"
    stroke-width="1.9"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    {#each SECTION_ICONS[id] ?? [] as d}<path {d} />{/each}
  </svg>
{/snippet}

<section class="page settings">
  <h2>Settings</h2>

  <!-- `tip-right` because the bubble would otherwise centre itself on a control at the page's
       right edge and hang off it; `traps.md` records that any `data-tip` there needs this. The
       rest of this button's reasoning is with its rule in `theme.css`. -->
  <button
    type="button"
    class="exit tip-right"
    onclick={onexit}
    aria-label={`Leave settings and go back to ${backTo}`}
    data-tip={`Back to ${backTo}`}
  >
    <span aria-hidden="true">×</span>
  </button>

  <!-- The search, and then either its results or the section you are in.
       It sits above both columns rather than inside the pane, because it searches all seven
       sections and belongs to the page rather than to whichever one is open. -->
  <label class="find">
    <input
      type="search"
      placeholder="Search settings"
      bind:value={find}
      spellcheck="false"
      aria-label="Search settings"
    />
  </label>

  {#if needle !== ""}
    <!-- **Results before the section, and this is the whole reason the search was reworked.** As
         a drawer, search filtered rows in place because every section was on screen. One
         section at a time means a match under Capture is invisible while Appearance is open, so
         the matches are listed with the section that holds them and clicking one goes there. -->
    <ul class="results">
      {#each results as found}
        <li>
          <button type="button" class="result" onclick={() => jump(found.section)}>
            <span class="in">{found.sectionLabel}</span>
            <span class="what">{found.label}</span>
          </button>
        </li>
      {/each}
      {#if results.length === 0}
        <li class="none">Nothing here matches <em>{find.trim()}</em>.</li>
      {/if}
    </ul>
  {/if}

  <div class="body">
    <!-- The nav the title bar's menu duplicates on purpose: the menu gets you here, this moves
         you about once you are. Both read `SETTINGS_SECTIONS`, so neither can offer a section the
         other does not. -->
    <nav class="sections" aria-label="Settings sections">
      {#each SETTINGS_SECTIONS as entry}
        <button
          type="button"
          class="tab"
          class:current={section === entry.id}
          onclick={() => (section = entry.id)}
          aria-current={section === entry.id ? "page" : undefined}
          aria-label={entry.label}
          data-tip={entry.label}
        >
          {@render sectionIcon(entry.id)}
          <span class="tab-label">{entry.label}</span>
        </button>
      {/each}
    </nav>

    <!-- `page-body` is the shared scroller — the same one Reference and Glossary use, so the
         settings scroll with the stable gutter and the bottom breathing room every page has. -->
    <div class="pane page-body" class:searching={needle !== ""}>
    <!-- The section's title and what it is for, once, above whichever section is open — and the
         only place either is read, so the guard below is the only one needed. The line is
         `SETTINGS_ABOUT`, which Help's list of sections prints too. -->
    {#if current}
      <header class="pane-head">
        <h3>{current.label}</h3>
        <p>{SETTINGS_ABOUT[current.id]}</p>
      </header>
    {:else}
      <!-- A settings file from a newer build named a section this one does not have. -->
      <p class="note">That section is not in this version. Pick one from the list.</p>
    {/if}

    {#if section === "reading"}
    <div class="group">
      <div class="block" hidden={!some(FIND.reading.markSeen, FIND.reading.keyboard)}>
        <h4 class="subhead">The passage</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.reading.markSeen)}>
            <label class="row">
              <span>Mark focused words as Seen</span>
              <input type="checkbox" bind:checked={markSeenOnFocus} />
            </label>
            <p class="note">Landing on a New word marks it Seen. Known and Ignored words never move back.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.markSeen.label}</span></summary>
              <p>
                Whatever puts the selector on a New word marks that you have seen it — clicking, the
                arrow keys, or analysis landing on the first word of the passage. Known and Ignored words
                are never moved backwards.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.reading.keyboard)}>
            <label class="row">
              <span>Windows keyboard navigation</span>
              <input type="checkbox" bind:checked={tabNavigation} />
            </label>
            <p class="note">Tab moves through controls, and focus stays on what you click.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.keyboard.label}</span></summary>
              <p>
                Lets Tab move through controls and keeps focus on controls you click. In the passage,
                only the selected word joins the Tab order; the arrow keys move both focus and selection.
                Off preserves the reader-first mode, where Tab is ignored and clicks do not leave a
                persistent focus ring.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div
        class="block"
        hidden={!some(FIND.reading.lineBreaks, FIND.reading.guesses, FIND.reading.parts)}
      >
        <h4 class="subhead">Display</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.reading.lineBreaks)}>
            <label class="row">
              <span>Keep the capture's line breaks</span>
              <input type="checkbox" bind:checked={keepLineBreaks} />
            </label>
            <p class="note">Off joins the passage into one paragraph; on keeps the breaks it arrived with.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.lineBreaks.label}</span></summary>
              <p>
                Off, the passage is laid out as one paragraph that wraps to the pane. A manga panel or a
                game text box is broken by the width of its bubble, not by the sentence, so reproducing
                those breaks spends height on the artwork's shape and pushes a passage that would
                otherwise fit into a scroll. On, the reader shows the passage exactly as it arrived,
                which is what you want for text you pasted yourself. Affects only how it looks —
                analysis, translation and Anki read the same words either way.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.reading.guesses)}>
            <label class="row">
              <span>Mark uncertain readings</span>
              <input type="checkbox" bind:checked={markReadingGuesses} />
            </label>
            <p class="note">A dotted line under furigana the app is not sure of.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.guesses.label}</span></summary>
              <p>
                Some readings are the app's best guess rather than a certain match. On, those get a
                dotted line under the furigana, so you know which to check. Off by default, because
                it marks about one word in eight and reads as clutter on a page you are trying to
                read.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.reading.parts)}>
            <label class="row">
              <span>Show a word's parts</span>
              <input type="checkbox" bind:checked={showWordParts} />
            </label>
            <p class="note">Under a grouped word, buttons that look up each part on its own.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.parts.label}</span></summary>
              <p>
                Words like かっこいい are kept whole, so the whole word is the easy lookup. On, the
                selected word lists its parts as buttons, so you can look up かっこ or いい by
                itself. Off by default, and switching it off returns an open part to the whole word.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.reading.sizes)}>
        <h4 class="subhead">Text size</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.reading.sizes)}>
            <label class="row">
              <span>Interface <em>{(fontSizeDraft ?? fontSize).toFixed(0)}px</em></span>
              <!-- Applied on release, not while dragging: the slider is sized in `rem`, so every
                   step rescaled it and moved the handle out from under the pointer. Keyboard
                   steps still apply one at a time, since each key press is a `change`. -->
              <input
                type="range"
                min="12"
                max="24"
                step="1"
                value={fontSize}
                oninput={(event) => (fontSizeDraft = event.currentTarget.valueAsNumber)}
                onchange={(event) => {
                  fontSize = event.currentTarget.valueAsNumber;
                  fontSizeDraft = null;
                }}
              />
            </label>
            <label class="row">
              <span>Analyzed text <em>{Math.round(readerScale * 100)}%</em></span>
              <input type="range" min="0.6" max="2.5" step="0.01" bind:value={readerScale} />
            </label>
            <label class="row">
              <span>Definitions <em>{Math.round(panelScale * 100)}%</em></span>
              <input type="range" min="0.6" max="2" step="0.01" bind:value={panelScale} />
            </label>
            <p class="note">Interface moves everything together. The other two rebalance within it.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.sizes.label}</span></summary>
              <p>
                Interface moves everything together. The other two rebalance within it, so the
                passage can be large enough to read comfortably without the buttons and labels
                growing to match. Interface takes effect when you let go of the slider, so the
                slider stays put while you drag it.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.reading.colours)}>
        <h4 class="subhead">Word colours</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.reading.colours)}>
            {#each WORD_COLOURS as swatch}
              <label class="row token-row">
                <span>{swatch.label}</span>
                <span class="token-set">
                  <input
                    type="color"
                    value={theme.overrides[swatch.token] ?? paletteNow[swatch.token] ?? "#000000"}
                    oninput={(event) => setToken(swatch.token, event.currentTarget.value)}
                  />
                  {#if theme.overrides[swatch.token]}
                    <button
                      class="control"
                      onclick={() => clearToken(swatch.token)}
                      data-tip="Back to derived">Reset</button
                    >
                  {/if}
                </span>
              </label>
            {/each}
            <p class="note">The rule under a word and its chip are one colour; the wash and edge follow it.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.colours.label}</span></summary>
              <p>
                What a word is drawn in: the rule under it in the passage and the chip in the vocabulary
                panel are the same colour on purpose, so the control and the text cannot say different
                things. Picking one moves both, and the wash and edge follow it.
              </p>
              <p>
                <em>Selected</em> is the word you are on. It follows the accent until you set it here,
                which is what detaches the two — after that the accent can move without dragging the
                selection with it. Reset puts it back on the accent.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.reading.analyzer, FIND.reading.ichiranServer)}>
        <h4 class="subhead">How the text is read</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.reading.analyzer)}>
            <label class="row">
              <span>Analyzer</span>
              <select bind:value={analyzer}>
                {#each ANALYZER_OPTIONS as option}<option value={option.value}>{option.label}</option>{/each}
              </select>
            </label>
            <p class="note">
              What breaks a passage into words. Changes apply at once{analyzerEngine
                ? ` — running ${analyzerEngine} now`
                : ""}.
            </p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.analyzer.label}</span></summary>
              <p>
                What breaks a passage into words, which is the first thing that happens to anything you
                read. Auto prefers Ichiran, with Lindera assistance in the background and a visible
                fallback when Ichiran cannot answer. Changing
                this applies straight away{analyzerEngine ? ` — running ${analyzerEngine} now` : ""},
                and anything already on screen is analysed again.
              </p>
            </details>
            {#if analyzer !== "builtin" && ichiranOffer && !ichiranOffer.ready}
              <IchiranSetup size={ichiranOffer.size} {onadopt} />
            {/if}
          </div>
          <div class="item" hidden={!hit(FIND.reading.ichiranServer)}>
            <label class="row">
              <span>Keep Ichiran running between passages</span>
              <input type="checkbox" bind:checked={ichiranServer} />
            </label>
            <p class="note">About 1.4 seconds a passage becomes 0.47, for the memory it keeps while open.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.ichiranServer.label}</span></summary>
              <p>
                On by default. Without it Ichiran starts fresh for every passage, and most of that time is
                spent loading itself rather than reading Japanese — about <strong>1.4 seconds a passage
                against 0.47</strong> when it is left running. The cost is that it stays in memory, with its database connection
                open, for as long as the app is.
                <em>Worth it if you read for a while at a time; not if you look up the odd word.</em>
                It does nothing unless Ichiran is the analyzer — the built-in one is part of the app and has
                no process to keep.
              </p>
            </details>
          </div>
        </div>
      </div>

      <!-- Mounted whatever the search says, and hidden like every other block: the offers are a
           handful of rows, and an `{#if}` here would be the one block whose rows the
           block-keys test could not see. -->
      <div class="block" hidden={!some(FIND.reading.extras)}>
        <h4 class="subhead">Optional downloads</h4>
        <div class="cluster">
          <div class="item">
            <p class="note">Never part of required setup. Each stays on this device under its own licence.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.reading.extras.label}</span></summary>
              <p>
                These are never part of required setup. Add only the reading aids you want; each stays on
                this device and keeps its own licence.
              </p>
            </details>
          </div>
          {#each optionalOffers as offer}
            {@const job = fetching.jobs[offer.id]}
            {@const saved = setupStore.snapshot?.components.find((component) => component.id === offer.id)}
            <section class="item optional-resource">
              <div>
                <strong>{offer.label}</strong>
                <span>{offer.about}</span>
                <small>{humanSize(offer.bytes)} · {offer.licence}</small>
                {#if offer.id === "pronunciation"}
                  <!-- True with the pack installed too: a word it does not have is still checked. -->
                  <small>Words it lacks are checked with JapanesePod101 online when shown.</small>
                {/if}
                {#if fetching.failures[offer.id]}
                  <small class="missing">{fetching.failures[offer.id]}</small>
                {/if}
              </div>
              {#if offer.ready}
                <span class="installed">Installed</span>
              {:else if job}
                <div class="optional-progress">
                  <progress value={job.done} max={job.total ?? undefined}></progress>
                  <small>{share(job) === null ? job.what : `${share(job)}%`}</small>
                  <button class="control" onclick={() => fetching.cancel(offer.id)}>Pause</button>
                </div>
              {:else if saved && hasSavedPartial(saved)}
                <!-- A paused download keeps what it has: offer to finish it, or to throw it away. -->
                <div class="optional-saved">
                  <small>{savedPhrase(saved.downloaded_bytes, saved.total_bytes || offer.bytes, humanBytes) ?? "Download paused."}</small>
                  <button class="control" onclick={() => fetching.start(offer.id)}
                    >Resume <span class="offscreen">{offer.label}</span></button
                  >
                  <button class="control" onclick={() => setupStore.restartArtifact(offer.id)}
                    >Discard partial download <span class="offscreen">of {offer.label}</span></button
                  >
                </div>
              {:else}
                <button class="control" onclick={() => fetching.start(offer.id)}>
                  Get {humanSize(offer.bytes)}
                </button>
              {/if}
            </section>
          {/each}
          {#if optionalOffers.length === 0 && fetching.loaded}
            <div class="item">
              <p class="note">No optional download catalog is available in this build.</p>
            </div>
          {/if}
        </div>
      </div>
    </div>
    {/if}

    {#if section === "appearance"}
    <div class="group">
      <div class="block" hidden={!some(FIND.appearance.theme)}>
        <h4 class="subhead">Colour scheme</h4>
        <div class="cluster">
          <!-- Every colour in the app is derived from these three. The list picks all of them at
               once; the two sliders below move the hues without disturbing the mode. -->
          <div class="item" hidden={!hit(FIND.appearance.theme)}>
            <label class="row">
              <span>Palette</span>
              <PalettePicker {theme} {themePresets} onpick={applyPreset} />
            </label>
            <!-- Immediately after the row, and nothing may come between: what it says changes with
                 the palette, and `app-menu.spec.ts` finds it as the row's next sibling. -->
            <p class="note" hidden={presetAbout === ""}>
              {presetAbout}
            </p>
            {#if paletteMode}
              <label class="row">
                <span>{paletteMode === "save" ? "Call it" : "Rename to"}</span>
                <input
                  class="control"
                  type="text"
                  bind:this={paletteField}
                  bind:value={paletteName}
                  placeholder={paletteMode === "save" ? "Dusk" : matchedPreset}
                  onkeydown={(event) => {
                    if (event.key === "Enter") commitPalette();
                  }}
                />
              </label>
              {#if paletteClash === "mine"}
                <!-- Said before the button is pressed, and the button changes with it. A warning that
                     arrives after the fact is a report, not a choice. -->
                <p class="note warn">
                  You already have a palette called <em>{paletteName.trim()}</em>.
                  {paletteMode === "save"
                    ? "Saving replaces it — its colours and decoration are overwritten with what is on screen now."
                    : "Renaming onto it replaces it, and the two become one palette."}
                </p>
              {:else if paletteClash === "shipped"}
                <p class="note">
                  <em>{paletteName.trim()}</em> is one of the palettes that ship with the app, and those
                  cannot be replaced. This will be kept as
                  <em>{freePresetName(allPalettes.map((p: (typeof allPalettes)[number]) => p.name), paletteName)}</em>
                  instead.
                </p>
              {/if}
              <div class="row deck-actions">
                <button class="control" class:danger={paletteClash === "mine"} onclick={commitPalette}>
                  {paletteClash === "mine"
                    ? paletteMode === "save"
                      ? "Overwrite"
                      : "Replace"
                    : paletteMode === "save"
                      ? "Save"
                      : "Rename"}
                </button>
                <button class="control" onclick={() => (paletteMode = null)}>Cancel</button>
              </div>
            {:else}
              <div class="row deck-actions">
                <button
                  class="control tip-right"
                  onclick={() => startPalette("save")}
                  data-tip="Keep what is on screen as a palette of your own">Save as…</button
                >
                <!-- The shipped palettes cannot be renamed or removed. Editing one offers a copy
                     instead of refusing, which is what keeps "untouchable" from reading as a wall. -->
                <button
                  class="control tip-right"
                  onclick={() => startPalette("rename")}
                  disabled={!mineSelected}
                  data-tip={mineSelected
                    ? `Rename ${matchedPreset}`
                    : "Only palettes you saved can be renamed"}>Rename</button
                >
                <button
                  class="control danger tip-right"
                  onclick={forgetPalette}
                  disabled={!mineSelected}
                  data-tip={mineSelected
                    ? `Forget ${matchedPreset} — the colours on screen stay`
                    : "Only palettes you saved can be removed"}>Forget</button
                >
              </div>
            {/if}
          </div>

          <!-- **Three states, so three segments, and they are real radio buttons.** The `<select>`
               this replaced hid two of the three answers behind a click. Native radios rather than
               buttons wearing `role="radio"`: one Tab stop on the chosen mode and the arrow keys
               moving *and* choosing come with the element, where a button group would have to
               write both and could get either wrong. The inputs are `.offscreen` and the labels are
               what you see; the focus ring is drawn on the label, since the input that has focus
               is not drawn at all. -->
          <div class="item" hidden={!hit(FIND.appearance.theme)}>
            <div class="row">
              <span id="settings-mode">Mode</span>
              <div class="mode" role="radiogroup" aria-labelledby="settings-mode">
                {#each MODES as option}
                  <label class="control" class:on={theme.mode === option.id}>
                    <input
                      class="offscreen"
                      type="radio"
                      name="settings-mode"
                      value={option.id}
                      checked={theme.mode === option.id}
                      onchange={() => (theme = { ...theme, mode: option.id })}
                    />
                    {option.label}
                  </label>
                {/each}
              </div>
            </div>
            <p class="note">System follows Windows' light or dark setting.</p>
          </div>
        </div>
      </div>

      <div
        class="block"
        hidden={!some(FIND.appearance.hues, FIND.appearance.dials, FIND.appearance.tokens)}
      >
        <h4 class="subhead">Adjust the palette</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.appearance.hues)}>
            <label class="row">
              <span>Interface tint <em>{Math.round(theme.surface_hue)}°</em></span>
              <input type="range" min="0" max="359" step="1" bind:value={theme.surface_hue} />
            </label>
            <label class="row">
              <span>Accent <em>{Math.round(theme.accent_hue)}°</em></span>
              <input type="range" min="0" max="359" step="1" bind:value={theme.accent_hue} />
            </label>
            <p class="note">The interface is built from these two. Word-status colours keep their own.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.appearance.hues.label}</span></summary>
              <p>
                <!-- Says the one thing a learner cannot see by dragging, and would otherwise discover by
                     trying: the four word colours are not on this dial. -->
                The whole interface is built from these two. New, Seen, Known and Ignored keep their own
                colours whatever you choose, so a word's status never blends into the accent.
              </p>
            </details>
          </div>
          <!-- With the hues, where the search index has always filed them. They rendered under
               "Part-of-speech colours" while search announced them here. -->
          <div class="item" hidden={!hit(FIND.appearance.dials)}>
            <label class="row">
              <span>Tint strength <em>{Math.round(theme.tint * 100)}%</em></span>
              <input type="range" min="0" max="2.5" step="0.01" bind:value={theme.tint} />
            </label>
            <label class="row">
              <span>Contrast <em>{Math.round(theme.contrast * 100)}%</em></span>
              <input type="range" min="0.7" max="1.6" step="0.01" bind:value={theme.contrast} />
            </label>
            <label class="row">
              <span>Depth <em>{theme.depth > 0 ? "+" : ""}{Math.round(theme.depth * 100)}</em></span>
              <input type="range" min="-0.08" max="0.08" step="0.01" bind:value={theme.depth} />
            </label>
            <p class="note">Tint at nothing gives plain greys. Depth adds ink either way.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.appearance.dials.label}</span></summary>
              <p>
                Tint at nothing gives plain greys. Depth is more ink either way — blacker on a dark
                palette, greyer on a light one.
              </p>
            </details>
          </div>

          <!-- The sharp knife, and behind a fold because it is one. Everything above is derived, so
               it cannot be internally inconsistent; a colour set here is taken at its word. -->
          <div class="item" hidden={!hit(FIND.appearance.tokens)}>
            <details class="tokens">
              <summary>Set individual colours</summary>
              <p class="note">These override the dials. Only text against the page is held to a readable minimum.</p>
              <details class="more">
                <summary>More <span class="offscreen">about {FIND.appearance.tokens.label}</span></summary>
                <p>
                  These override what the dials work out. Only text against the page is held to a
                  readable minimum — everything else is yours to get wrong, and warned about rather than
                  corrected.
                </p>
              </details>
              {#if warnings.length > 0}
                <p class="note warn">
                  {#each warnings as warning}
                    <span class="warned"
                      >--{warning.token} is {warning.ratio.toFixed(1)}:1 against the page, below {warning.wanted}:1.</span
                    >
                  {/each}
                </p>
              {/if}
              {#each OVERRIDABLE as token}
                <label class="row token-row">
                  <span>--{token}</span>
                  <span class="token-set">
                    <input
                      type="color"
                      value={theme.overrides[token] ?? paletteNow[token] ?? "#000000"}
                      oninput={(event) => setToken(token, event.currentTarget.value)}
                    />
                    {#if theme.overrides[token]}
                      <button class="control" onclick={() => clearToken(token)} data-tip="Back to derived"
                        >Reset</button
                      >
                    {/if}
                  </span>
                </label>
              {/each}
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.appearance.kinds, FIND.appearance.kindColours)}>
        <h4 class="subhead">Part-of-speech colours</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.appearance.kinds)}>
            <label class="row">
              <span>Kind hues <em>{Math.round(theme.kind_spin)}°</em></span>
              <span class="token-set">
                <input type="range" min="0" max="359" step="1" bind:value={theme.kind_spin} />
                <!-- Offered only once there is something to undo, which is the rule every colour well on
                     this page already follows: a reset that is always there is a control for a state you
                     are usually not in. -->
                {#if Math.round(theme.kind_spin) !== 0}
                  <button class="control" onclick={() => (theme = { ...theme, kind_spin: 0 })}
                    >Set to default</button
                  >
                {/if}
              </span>
            </label>
            <!-- **The dial needs to show its work.** Six numbers moving at once is not something a
                 degree readout conveys, and the failure it causes — a kind landing on the accent — is
                 invisible until you meet it on a card. The strip is the six colours as the senses will
                 wear them, with the accent beside them to compare against. -->
            <div class="row kind-strip">
              <span>Preview</span>
              <!-- **The names, in their own colours — not anonymous squares.** A row of coloured chips
                   says which colours exist and nothing about what they mean, so every one of them had to
                   be hovered to be understood. Drawn as the word, the strip is the thing it is previewing:
                   a kind is a small coloured label in a sense list, and this is one. The accent is set
                   apart at the end because it is what the six are being measured against, not one of
                   them. -->
              <span class="swatches">
                {#each KIND_COLOURS as swatch}
                  <span class="swatch" style={`color: ${paletteNow[swatch.token]}`}>{swatch.label}</span>
                {/each}
                <span class="swatch accent" style={`color: ${paletteNow.accent}`}>Accent</span>
              </span>
            </div>
            <p class="note">Turns all six kind colours together, keeping them as far apart as they are.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.appearance.kinds.label}</span></summary>
              <p>
                <!-- The one thing dragging this cannot show: it turns the set, it does not spread it. -->
                The colour a sense is marked with, by what kind of word it is. This turns all six together
                and keeps them as far apart as they are now — they are read against <em>each other</em>, so
                spreading them is not on offer.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.appearance.kindColours)}>
            <details class="tokens">
              <summary>Set each part of speech</summary>
              {#each KIND_COLOURS as swatch}
                <label class="row token-row">
                  <!-- The label drawn in the colour it names, which is the only preview that matters
                       here: a kind is a small coloured word in a sense list, and this is one. -->
                  <span style={`color: ${paletteNow[swatch.token]}`}>{swatch.label}</span>
                  <span class="token-set">
                    <input
                      type="color"
                      value={theme.overrides[swatch.token] ?? paletteNow[swatch.token] ?? "#000000"}
                      oninput={(event) => setToken(swatch.token, event.currentTarget.value)}
                    />
                    {#if theme.overrides[swatch.token]}
                      <button
                        class="control"
                        onclick={() => clearToken(swatch.token)}
                        data-tip="Back to derived">Reset</button
                      >
                    {/if}
                  </span>
                </label>
              {/each}
              <p class="note">One at a time, when the dial cannot get you there.</p>
              <details class="more">
                <summary>More <span class="offscreen">about {FIND.appearance.kindColours.label}</span></summary>
                <p>
                  One at a time, when the dial cannot get you there. The wash behind a chosen filter chip
                  follows whatever you pick, so the row you narrow by keeps matching the senses you read.
                </p>
              </details>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.appearance.decor)}>
        <h4 class="subhead">Decoration</h4>
        <div class="cluster">
          <!-- **Each motif's switch heads its own item, and everything under it goes with it.** Off
               keeps the strength, colour and spacing exactly as they were, so switching back on is
               the look you had rather than a slider to find again. The rows are removed rather
               than dimmed while off — tuning a motif you cannot see is not worth a screenful — and
               the shared ones (size, softening) go only when both are off. A motif that is on at
               zero strength still dims its own colour and spacing, as before. -->
          <div class="item" hidden={!hit(FIND.appearance.decor)}>
            <label class="row">
              <span>Clouds</span>
              <input type="checkbox" bind:checked={decorCloudOn} />
            </label>
            {#if decorCloudOn}
              <label class="row">
                <span>Strength <em>{Math.round(decorCloud * 100)}%</em></span>
                <input
                  type="range" min="0" max="0.16" step="0.01"
                  bind:value={decorCloud}
                />
              </label>
              <label class="row" class:disabled={decorCloud === 0}>
                <span>Cloud colour</span>
                <select bind:value={decorCloudTint} disabled={decorCloud === 0}>
                  <option value="ink">Ink</option>
                  <option value="blossom">Blossom</option>
                  <option value="accent">Accent</option>
                </select>
              </label>
              <!-- Spacing, which is a different question from size: a band of small clouds packed
                   tight and a band of small clouds strung out are both things somebody might want,
                   and one slider cannot say which. Two of them, because the clouds run sideways
                   along the full width of the window and the sakura runs down a rail a couple of
                   centimetres across — a single number would be a compromise in both. -->
              <label class="row" class:disabled={decorCloud === 0}>
                <span>Clouds sit apart by <em>{Math.round(decorCloudSpread * 100)}%</em></span>
                <input
                  type="range" min="0.5" max="2.5" step="0.01"
                  bind:value={decorCloudSpread}
                  disabled={decorCloud === 0}
                />
              </label>
              <label class="row" class:disabled={decorCloud === 0}>
                <span>Clouds sit above the footer by <em>{decorCloudPad.toFixed(1)}rem</em></span>
                <input
                  type="range" min="0" max="4" step="0.1"
                  bind:value={decorCloudPad}
                  disabled={decorCloud === 0}
                />
              </label>
              <label class="row" class:disabled={!decorBlur || decorCloud === 0}>
                <span>Cloud softness <em>{decorCloudBlur.toFixed(1)}px</em></span>
                <input
                  type="range" min="0" max="8" step="0.1"
                  bind:value={decorCloudBlur}
                  disabled={!decorBlur || decorCloud === 0}
                />
              </label>
            {/if}
          </div>
          <div class="item" hidden={!hit(FIND.appearance.decor)}>
            <label class="row">
              <span>Sakura in the rail</span>
              <input type="checkbox" bind:checked={decorSakuraOn} />
            </label>
            {#if decorSakuraOn}
              <label class="row">
                <span>Strength <em>{Math.round(decorSakura * 100)}%</em></span>
                <input
                  type="range" min="0" max="0.34" step="0.01"
                  bind:value={decorSakura}
                />
              </label>
              <label class="row" class:disabled={decorSakura === 0}>
                <span>Sakura colour</span>
                <select bind:value={decorSakuraTint} disabled={decorSakura === 0}>
                  <option value="ink">Ink</option>
                  <option value="blossom">Blossom</option>
                  <option value="accent">Accent</option>
                </select>
              </label>
              <label class="row" class:disabled={decorSakura === 0}>
                <span>Sakura sit apart by <em>{Math.round(decorSakuraSpread * 100)}%</em></span>
                <input
                  type="range" min="0.5" max="2.5" step="0.01"
                  bind:value={decorSakuraSpread}
                  disabled={decorSakura === 0}
                />
              </label>
              <!-- Three pixels against the clouds' eight, and the ceiling is the artwork's rather than
                   a matter of taste: a blossom is a 1.4px non-scaling stroke, so spreading it much
                   further drops its peak alpha to nothing and the slider stops being softness and
                   becomes a second opacity. -->
              <label class="row" class:disabled={!decorBlur || decorSakura === 0}>
                <span>Sakura softness <em>{decorSakuraBlur.toFixed(1)}px</em></span>
                <input
                  type="range" min="0" max="3" step="0.1"
                  bind:value={decorSakuraBlur}
                  disabled={!decorBlur || decorSakura === 0}
                />
              </label>
            {/if}
          </div>
          <div class="item" hidden={!hit(FIND.appearance.decor)}>
            {#if decorCloudOn || decorSakuraOn}
              <label class="row" class:disabled={!showsDecor}>
                <span>Pattern size <em>{Math.round(decorScale * 100)}%</em></span>
                <input
                  type="range" min="0.6" max="1.8" step="0.01"
                  bind:value={decorScale}
                  disabled={!showsDecor}
                />
              </label>
              <!-- Softness, which the opacities above deliberately do without a switch for. The
                   difference is that a blur has a cost a zero does not remove: `filter: blur(0px)`
                   still puts the element on its own composited layer. So off is a real `none`, and
                   only a switch can say off. -->
              <label class="row" class:disabled={!showsDecor}>
                <span>Soften the watermarks</span>
                <input type="checkbox" bind:checked={decorBlur} disabled={!showsDecor} />
              </label>
            {/if}
            <p class="note">Both sit behind everything and neither is clickable. The switches keep each look for later.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.appearance.decor.label}</span></summary>
              <p>
                A band of stylised cloud at the foot of the page and a sakura sprig down the rail,
                both behind everything and neither clickable. Switch either off, or drag its strength
                to nothing; switching off keeps the strength, colour and spacing for when you switch
                it back on. Blossom is the same red the app uses for an ignored word and for the
                buttons that throw work away — at these strengths that is unlikely to read as a
                warning, but it is why the colour is worth a thought before raising it.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.appearance.animation)}>
        <h4 class="subhead">Motion</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.appearance.animation)}>
            <label class="row">
              <span>Animate the interface</span>
              <input type="checkbox" bind:checked={animations} />
            </label>
            <label class="row" class:disabled={!animations}>
              <span>Speed <em>{animationMs}ms</em></span>
              <input
                type="range"
                min="60"
                max="400"
                step="1"
                bind:value={animationMs}
                disabled={!animations}
              />
            </label>
          </div>
        </div>
      </div>
    </div>
    {/if}

    {#if section === "capture"}
    <div class="group">
      <div
        class="block"
        hidden={!some(
          FIND.capture.captureBind,
          FIND.capture.overlayBind,
          FIND.capture.switchBind,
        )}
      >
        <h4 class="subhead">Shortcuts</h4>
        <div class="cluster">
          {#if hit(FIND.capture.captureBind)}
            <div class="item">
              <BindRecorder
                label="Capture"
                bind={captureBind}
                onchange={onCaptureBind}
                onrecording={(active) => (recordingCapture = active)}
              />
            </div>
          {/if}
          {#if hit(FIND.capture.overlayBind)}
            <div class="item">
              <BindRecorder
                label="Show / hide"
                bind={overlayBind}
                onchange={onOverlayBind}
                onrecording={(active) => (recordingOverlay = active)}
              />
            </div>
          {/if}

          <!-- With the other two, not in Advanced. It was filed there and read as missing:
               three controls that do the same kind of thing, one of them two sections away. -->
          {#if hit(FIND.capture.switchBind)}
            <div class="item">
              <BindRecorder
                label="Switch overlay off / on"
                bind={switchBind}
                clearable
                onchange={onSwitchBind}
                onrecording={(active) => (recordingSwitch = active)}
              />
              <p class="note">Unbound by default. Switches the whole overlay off, and back on again.</p>
              <details class="more">
                <summary>More <span class="offscreen">about {FIND.capture.switchBind.label}</span></summary>
                <p>
                  Nothing is bound to this by default. Set it and the key switches the whole overlay
                  off — the frame goes away and neither capture binding answers — and switches it
                  back on. It keeps working while everything else is off, because it is the way back.
                </p>
              </details>
            </div>
          {/if}
        </div>
      </div>

      <div
        class="block"
        hidden={!some(FIND.capture.hold, FIND.capture.lockTap, FIND.capture.clickDelay)}
      >
        <h4 class="subhead">How the bindings behave</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.capture.hold)}>
            <label class="row">
              <span>Hold to hide after <em>{holdMs}ms</em></span>
              <input type="range" min="100" max="1000" step="1" bind:value={holdMs} />
            </label>
            <p class="note">Tap the show/hide binding to bring the frame to you; hold it this long to hide it.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.capture.hold.label}</span></summary>
              <p>
                Tap the show/hide binding to bring the frame to your cursor. Hold it this long and
                the frame hides there and then, and stays hidden until you tap again. Capture is
                off while it is hidden.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.capture.lockTap)}>
            <label class="row">
              <span>
                Double tap to lock, within
                <em>{lockTapMs === 0 ? "off" : `${lockTapMs}ms`}</em>
              </span>
              <!-- **The only slider that cannot simply step by one, and the reason is in the store.**
                   `Settings::lock_tap` passes zero through as "off" and clamps everything else up to
                   150, because a window too short for a second tap to land in is not a setting anybody
                   wants. So 1–149 are numbers this control can show and the app will never hold.

                   That was already true at `step="50"` — 50 and 100 were both stops, and both became
                   150 the moment they were read, while this row went on printing what was dragged to.
                   One-unit steps would have made it 149 such values instead of two. `snapLockTap` puts
                   the two halves back in agreement: everything below the floor resolves to off or to
                   the floor, whichever it is nearer, so what is printed is what is stored. -->
              <input
                type="range"
                min="0"
                max="1000"
                step="1"
                value={lockTapMs}
                oninput={(event) => (lockTapMs = snapLockTap(event.currentTarget.valueAsNumber))}
              />
            </label>
            <p class="note">Two quick taps lock the frame where it landed; two more release it.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.capture.lockTap.label}</span></summary>
              <p>
                Tap the show/hide binding twice this quickly and the frame locks where it has just
                landed — the same lock as the button in the header, so tapping twice again releases
                it. A locked frame still shows and hides on the binding; it just stops travelling to
                your cursor. Set this to <em>off</em> if you tap the binding in quick succession for
                other reasons.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.capture.clickDelay)}>
            <label class="row">
              <span>Overlay button waits <em>{clickDelayMs}ms</em> for a second click</span>
              <input type="range" min="100" max="600" step="1" bind:value={clickDelayMs} />
            </label>
            <p class="note">How long a click waits to learn whether it was half of a double click.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.capture.clickDelay.label}</span></summary>
              <p>
                The overlay button in the header shows and hides on a click and switches the
                feature off on a double click, so a click has to wait this long to find out which
                it was. Lower is a quicker button; higher gives a slow double click more room to
                arrive.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.capture.hideDuring, FIND.capture.frameChrome)}>
        <h4 class="subhead">The frame itself</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.capture.hideDuring)}>
            <label class="row">
              <span>Hide the frame while capturing</span>
              <input type="checkbox" bind:checked={hideDuringCapture} />
            </label>
            <p class="note">Turn on if the frame's border ever shows up in the recognized text.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.capture.hideDuring.label}</span></summary>
              <p>
                Off, the frame's transparent middle keeps it out of its own screenshot with no
                flicker. On, it hides for a moment instead — certain, but you see it blink on
                every read. Turn this on if the frame's border ever shows up in the recognized
                text.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.capture.frameChrome)}>
            <label class="row">
              <span>Tint the region <em>{frameTint === 0 ? "off" : `${Math.round(frameTint * 100)}%`}</em></span>
              <input type="range" min="0" max="0.5" step="0.01" bind:value={frameTint} />
            </label>
            <p class="note warn">A tint turns on hiding the frame while capturing, so it blinks on every read.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.capture.frameChrome.label}</span></summary>
              <p>
                A wash over what the frame is pointed at, so it can be found at a glance.
                <em>This turns on hiding the frame while capturing, and cannot work without it</em> —
                a tint the frame is still wearing when the picture is taken is a tint the recognizer
                reads. That is the flicker above, on every read.
              </p>
            </details>
          </div>
        </div>
      </div>
    </div>
    {/if}

    {#if section === "search"}
    <div class="group">
      <div class="block" hidden={!some(FIND.search.indexes)}>
        <h4 class="subhead">Dictionary indexes</h4>
        <div class="cluster">
          <div class="item dictionary-upkeep" hidden={!hit(FIND.search.indexes)}>
            {#if upkeepState === "loading"}
              <p class="note">Checking dictionary indexes…</p>
            {:else if upkeepState === "failed"}
              <p class="note warn" role="status">
                The dictionary index status could not be read.{upkeepError ? ` ${upkeepError}` : ""}
              </p>
              {#if retryDictionaryUpkeep}
                <button class="control" onclick={() => retryDictionaryUpkeep?.()}>Check again</button>
              {/if}
            {:else if !upkeepKnown}
              <p class="note">No dictionary is installed yet.</p>
            {:else if upkeepPending.length === 0}
              <p class="note">
                <!-- What is claimed is what was checked. Every index being present says the
                     dictionary can be asked every kind of question; it says nothing about whether a
                     particular word is in it. -->
                Every installed dictionary has all of its indexes.
              </p>
            {:else}
              <p class="note">
                {upkeepPending.length === 1
                  ? `${upkeepPending[0].title} is missing an index.`
                  : `${upkeepPending.length} dictionaries are missing an index.`}
                Building it downloads nothing and takes a few seconds per dictionary.
              </p>
              <ul>
                <!-- Keyed on the install name. Two dictionaries can share a title, and keying on
                     that collapsed them into one row — or crashed the list with a duplicate key. -->
                {#each upkeepPending as entry (entry.id)}
                  <li>
                    <span>{entry.title}</span>
                    <small>
                      {[
                        entry.has_sequence ? null : "definitions for other spellings",
                        entry.has_glosses ? null : "English search",
                        entry.has_kanji ? null : "kanji compounds",
                      ]
                        .filter(Boolean)
                        .join(", ")}
                    </small>
                  </li>
                {/each}
              </ul>
              <button class="control" onclick={updateIndexes} disabled={updatingIndexes}>
                {updatingIndexes ? "Building…" : "Update dictionary indexes"}
              </button>
              <details class="more">
                <summary>More <span class="offscreen">about {FIND.search.indexes.label}</span></summary>
                <p>
                  {upkeepPending.length === 1
                    ? `${upkeepPending[0].title} is missing an index.`
                    : `${upkeepPending.length} dictionaries are missing an index.`}
                  Words spelled a second way — <span lang="ja">抜きとる</span> for
                  <span lang="ja">抜き取る</span> — are filed as a pointer to the main entry, and without
                  this index the pointer is all that can be shown. Building it uses the dictionary
                  already installed: <strong>nothing is downloaded</strong>, and it takes a few seconds
                  per dictionary.
                </p>
              </details>
            {/if}
            {#if indexResult}
              <p class="note" class:warn={indexFailed} role="status">{indexResult}</p>
            {/if}
          </div>
        </div>
      </div>

      <div
        class="block"
        hidden={!some(FIND.search.frequency, FIND.search.frequencySource, FIND.search.direction)}
      >
        <h4 class="subhead">Ranking</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.search.frequency)}>
            <label class="row">
              <span>Order results by</span>
              <select bind:value={frequencySource}>
                <option value="">The dictionary's order</option>
                <option value="jpdb">JPDB — anime, visual novels, novels</option>
                <option value="bccwj">BCCWJ — books, news, blogs</option>
              </select>
            </label>
            <p class="note">Ranks an English search by how common each word is, from a list you installed.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.search.frequency.label}</span></summary>
              <p>
                An English search returns a dozen words that mean the same thing, and the dictionary
                cannot rank them — 22,553 of its entries share its top score. A frequency list can.
                Install one from the setup panel first; the two count different Japanese and disagree
                about what is common. Applies to your next search — a big list takes a moment to read.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.search.frequencySource)}>
            <label class="row">
              <span>Show the frequency source</span>
              <input type="checkbox" bind:checked={showFrequencySource} />
            </label>
            <p class="note">Names the list under ranked results. Off hides only that line, not the ordering.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.search.frequencySource.label}</span></summary>
              <p>
                Names the list beneath ranked Reference results, for example “Sorted most common first
                by JPDBv2.” Turning this off hides only that line; the selected list still orders results.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.search.direction)}>
            <label class="row">
              <span>Frequency numbers are</span>
              <select bind:value={frequencyDirection}>
                <option value="rank">Ranks — 1 is the commonest</option>
                <option value="count">Counts — the largest is the commonest</option>
              </select>
            </label>
            <p class="note">Only for a list you installed yourself. The two offered here are always read as ranks.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.search.direction.label}</span></summary>
              <p>
                Only affects a list you installed yourself. The two offered here are ranks, and are read
                that way whatever this says — the app fetched them, so it already knows. For your own list
                nothing about a bare number says which it is, and reading a rank list as counts orders
                every search exactly backwards.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.search.credits)}>
        <h4 class="subhead">What a result shows</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.search.credits)}>
            <label class="row">
              <span>Name the source — analysis</span>
              <input type="checkbox" bind:checked={showSourceAnalysis} />
            </label>
            <label class="row">
              <span>Name the source — reference</span>
              <input type="checkbox" bind:checked={showSourceReference} />
            </label>
            <p class="note">Shows each definition's dictionary as a chip. The footer credits them either way.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.search.credits.label}</span></summary>
              <p>
                Puts the dictionary a definition came from beside its tags, as another chip. Two
                switches because the pages are not alike: a search returns eighteen results and
                repeats the same name on every one, where the analysis page shows a single word.
              </p>
              <p>
                <em>Attribution does not depend on this.</em> Jitendex and JMnedict are CC BY-SA and
                are credited in the footer whatever these are set to; this only decides whether each
                entry says so again.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.search.cards)}>
        <h4 class="subhead">Opening a result</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.search.cards)}>
            <label class="row">
              <span>Click once to focus, twice to open</span>
              <input type="checkbox" bind:checked={focusBeforeOpen} />
            </label>
            <p class="note">Off, one click opens a result. Either way the keyboard reaches every one.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.search.cards.label}</span></summary>
              <p>
                A click lifts a card; a second click opens it. Off, one click opens it. Either way a
                click on an open card's heading closes it, the card stays selectable — copying a word or
                dragging across a definition never opens anything — and Tab, the arrow keys and Enter
                reach every result from the keyboard.
              </p>
            </details>
          </div>
        </div>
      </div>
    </div>
    {/if}

    {#if section === "recognition"}
    <div class="group">
      <div class="block" hidden={!some(FIND.recognition.installation, FIND.recognition.storage)}>
        <h4 class="subhead">Installation</h4>
        <div class="cluster">
          <div class="item installation-status" hidden={!hit(FIND.recognition.installation)}>
            <div>
              <strong>{setupStore.snapshot?.required_ready ? "Screen capture is ready" : "Setup is incomplete"}</strong>
              <span>
                {setupStore.snapshot?.required_ready
                  ? "Both required OCR engines passed verification."
                  : setupStore.snapshot?.current_issue || "Resume setup to install and verify both engines."}
              </span>
            </div>
            {#if setupStore.snapshot?.phase === "downloading" || setupStore.snapshot?.phase === "installing" || setupStore.snapshot?.phase === "verifying"}
              <button class="control" onclick={() => setupStore.pause()}>Pause setup</button>
            {:else if !setupStore.snapshot?.required_ready}
              <button class="control" onclick={() => setupStore.start()}>Resume setup</button>
            {/if}
            <button class="control" onclick={() => setupStore.repair()}>Repair installation</button>
          </div>
          <div
            class="item"
            hidden={!some(FIND.recognition.installation, FIND.recognition.storage)}
          >
            <details class="installation-details">
              <summary>Downloaded resources and repair details</summary>
              <p class="note">
                Stored in <code>{setupStore.snapshot ? displayPath(setupStore.snapshot.storage.target) : "checking…"}</code>.
                A complete repair may need up to {humanBytes(setupStore.snapshot?.storage.peak_bytes ?? null)} free.
              </p>
              <ul hidden={!hit(FIND.recognition.installation)}>
                {#each setupStore.snapshot?.components.filter((component) => component.required) ?? [] as component}
                  <li>
                    <span>{component.label}</span>
                    <small>{component.verification.replaceAll("_", " ")}</small>
                    {#if hasSavedPartial(component)}
                      <small>{savedPhrase(component.downloaded_bytes, component.total_bytes, humanBytes) ?? "Download paused."}</small>
                      <!-- Says what it throws away. "Start over" left the reader to guess. -->
                      <button class="control" onclick={() => setupStore.restartArtifact(component.id)}
                        >Discard partial download <span class="offscreen">of {component.label}</span></button
                      >
                    {/if}
                  </li>
                {/each}
              </ul>
              <label class="resource-location" hidden={!hit(FIND.recognition.storage)}>
                <span>Move downloaded resources</span>
                <div>
                  <input bind:value={moveResourcesTo} />
                  <button class="control" onclick={browseResourceLocation}>Browse…</button>
                  <button class="control" onclick={moveResources} disabled={movingResources}>
                    {movingResources ? "Copying and verifying…" : "Move resources"}
                  </button>
                </div>
              </label>
              <label class="remove-old" hidden={!hit(FIND.recognition.storage)}>
                <input type="checkbox" bind:checked={removeOldResources} />
                Remove unchanged old copies after restart
              </label>
              {#if moveResourcesResult && hit(FIND.recognition.storage)}
                <p class="note">{moveResourcesResult}</p>
              {/if}
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.recognition.engine, FIND.recognition.loaded)}>
        <h4 class="subhead">Reading the screen</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.recognition.engine)}>
            <!-- A custom listbox contains several buttons, so this cannot be a `<label>`: labels may
                 contain only one labelable control. The selector carries its own accessible name. -->
            <div class="row">
              <span>Engine</span>
              <OcrSelect bind:value={ocr} label="Recognition engine" />
            </div>
            <p class="note">Auto pairs PP-OCRv6 with manga-ocr. PaddleOCR-VL reads best, with a local service.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.recognition.engine.label}</span></summary>
              <p>
                Auto keeps confident horizontal PP-OCRv6 reads, and checks manga-ocr for vertical,
                incomplete or rejected reads. Both engines are required for the supported Auto setup;
                direct choices remain for difficult material. PaddleOCR-VL read most accurately of
                anything measured here, needs the local service below, and is never entered by Auto.
              </p>
            </details>

            <!-- Mounted at last. This component existed and was imported by nothing while the
                 reader it installs was paused; it sits under the engine selector because that is
                 where someone reads the option and wonders how to get it. -->
            {#if hit(FIND.recognition.engine)}
              <VlmSetup
                selected={ocr === "vlm"}
                endpoint={vlmEndpoint}
                size={vlmOffer?.size ?? ""}
                onconfigure={(endpoint, model) => {
                  vlmEndpoint = endpoint;
                  vlmModel = model;
                }}
                onchoose={() => {
                  ocr = "vlm";
                }}
              />
            {/if}
            <!-- Said only when true: the managed service and any endpoint on this computer keep the
                 image here, and a warning that fires for them would teach the reader to ignore it. -->
            {#if ocr === "vlm" && vlmEndpoint.trim() !== "" && !isLoopback(vlmEndpoint)}
              <p class="note warn">Each captured image is sent to the recognition endpoint on another computer.</p>
            {/if}
          </div>

          <div class="item" hidden={!hit(FIND.recognition.loaded)}>
            <div class="row">
              <span>Loaded</span>
              <!-- The same names the selector above uses. This row printed the raw engine id, so a
                   reader who chose "PaddleOCR-VL (local server)" was told they had loaded `vlm`. -->
              <span class="status" class:missing={ocrEngine === "stub"}>
                {engineLabel(ocrEngine)}
              </span>
            </div>
            {#if ocrEngine === "stub"}
              <p class="note">Resume setup above. Partial downloads are kept.</p>
              <details class="more">
                <summary>More <span class="offscreen">about {FIND.recognition.loaded.label}</span></summary>
                <p>
                  Resume setup above. Sakatsu preserves partial downloads and verifies both engines
                  before marking screen capture ready.
                </p>
              </details>
            {/if}
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.recognition.prepare)}>
        <h4 class="subhead">Image preparation</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.recognition.prepare)}>
            <label class="row">
              <span>Enlarge small captures</span>
              <input type="checkbox" bind:checked={preprocessUpscale} />
            </label>
            <label class="row">
              <span>Trim empty margin</span>
              <input type="checkbox" bind:checked={preprocessTighten} />
            </label>
            <label class="row">
              <span>Local contrast</span>
              <input type="checkbox" bind:checked={preprocessContrast} />
            </label>
            <p class="note">Applied to the crop before the engine sees it. All off by default.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.recognition.prepare.label}</span></summary>
              <p>
                Applied to the crop before the engine above sees it. All off by default, because
                none of them is known to help <em>your</em> material — they help or hurt depending
                on the engine and the source, so they are switchable rather than tuned once and
                hidden. <code>core/tests/ocr_corpus.rs</code> scores them against a folder of
                screenshots if you want an answer rather than an impression.
              </p>
            </details>
          </div>
        </div>
      </div>
    </div>
    {/if}

    {#if section === "anki"}
    <div class="group">
      <div class="block" hidden={!some(FIND.anki.deck, FIND.anki.filter)}>
        <h4 class="subhead">Where cards go</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.anki.deck)}>
            <label class="row">
              <span>Deck</span>
              {#if decks.length > 0 && shownDecks.length > 0}
                <select bind:value={ankiDeck}>
                  <!-- **Nothing chosen is a state, and it has to be selectable.** Without this option a
                       `<select>` whose value is not among its children shows blank and can write that
                       blank back through the binding — the same invisible edit that used to happen when
                       a stale deck was filtered away. Stated instead, and disabled so it cannot be
                       chosen on purpose once a real deck exists. -->
                  {#if !target}
                    <option value={ankiDeck} disabled>Choose a deck…</option>
                  {/if}
                  {#each shownDecks as deck}<option value={deck}>{deck}</option>{/each}
                </select>
              {:else if decks.length > 0}
                <!-- Anki is answering and nothing in it is ours. Said plainly rather than filled in
                     with somebody else's deck — see `offerableDecks` for why an empty answer is the
                     safe one. -->
                <span class="status missing">No Sakatsu decks found</span>
              {:else}
                <!-- Names the part that is actually silent. Anki can be open and this still
                     fail, because the decks come over AnkiConnect and that is a separate add-on
                     — and "Anki is not running" sent you to look at a window already in front of
                     you. True either way: no Anki means no AnkiConnect. -->
                <span class="status missing">AnkiConnect is not answering</span>
              {/if}
            </label>

            {#if decks.length > 0 && shownDecks.length === 0}
              <p class="note warn">No deck here is Sakatsu's, so export is unavailable. <strong>Create one below.</strong></p>
              <details class="more">
                <summary>More <span class="offscreen">about {FIND.anki.deck.label}</span></summary>
                <p>
                  Nothing here was made by Sakatsu Reader, so there is nowhere to send a card and the
                  export button is unavailable. <strong>Create one below</strong> — or turn the switch off
                  to file cards into a deck of your own, which is a choice worth making on purpose.
                </p>
              </details>
            {:else if decks.length > 0 && !target}
              <p class="note warn">
                No deck is chosen, so the export button is unavailable until one is.
              </p>
            {/if}
          </div>

          {#if decks.length > 0}
            <div class="item" hidden={!hit(FIND.anki.filter)}>
              <label class="row">
                <span>Hide decks that aren't Sakatsu's</span>
                <input type="checkbox" bind:checked={ankiOurDecksOnly} />
              </label>
              <p class="note">
                <!-- Two things a learner cannot work out from the list itself. That empty decks stay
                     is the one that matters: without it, a deck made with the button below would
                     vanish from the list the moment it was created — which is exactly what happened
                     the first time this shipped. And the bypass has to be said out loud, or the
                     setting reads as broken on the collection where it does nothing. -->
                {#if shownDecks.length === decks.length && decks.length > 0}
                  Every deck here could take an export, so nothing is hidden.
                {:else}
                  Showing {shownDecks.length} of {decks.length}: decks made here, plus any already
                  holding cards from here.
                {/if}
              </p>
              <details class="more">
                <summary>More <span class="offscreen">about {FIND.anki.filter.label}</span></summary>
                <p>
                  Showing {shownDecks.length} of {decks.length}: decks made here, plus any already
                  holding cards from here. <em>Default</em> is never among them — it is Anki's own deck
                  and the place cards land when nothing is chosen. It is not ignored when that leaves
                  nothing: nowhere to send a card is safer than somewhere wrong.
                </p>
              </details>
            </div>
          {/if}
        </div>
      </div>

      {#if decks.length > 0}
        <div class="block" hidden={!some(FIND.anki.manage)}>
          <h4 class="subhead">Manage decks</h4>
          <div class="cluster">
            <div class="item" hidden={!hit(FIND.anki.manage)}>
              <label class="row">
                <span>Deck to act on</span>
                <!-- Only the decks these buttons can actually act on — see `manageableDecks`. It was
                     every deck at first, which put thirty-odd shared decks in a list where all four
                     buttons were disabled for every one of them. -->
                {#if manageDecks.length > 0}
                  <select bind:value={deckTarget} disabled={deckBusy || deckMode !== null}>
                    {#each manageDecks as deck}<option value={deck}>{deck}</option>{/each}
                  </select>
                {:else}
                  <!-- Create still works: it needs no target. So this says what is missing rather
                       than leaving an empty control that looks broken. -->
                  <span class="status missing">No decks made here yet</span>
                {/if}
              </label>

              {#if deckMode}
                <label class="row">
                  <span>{deckMode === "create" ? "Name it" : "Rename to"}</span>
                  <input
                    class="control"
                    type="text"
                    bind:this={nameField}
                    bind:value={deckName}
                    disabled={deckBusy}
                    placeholder={deckMode === "create" ? "Japanese::Reading" : deckTarget}
                    onkeydown={(event) => {
                      // Enter commits, because a field that appeared for one purpose should not need
                      // the pointer to finish it.
                      if (event.key === "Enter") commitName();
                    }}
                  />
                </label>
                <div class="row deck-actions">
                  <button class="control" onclick={commitName} disabled={deckBusy || !deckName.trim()}>
                    {deckMode === "create" ? "Create" : "Rename"}
                  </button>
                  <button class="control" onclick={stopNaming} disabled={deckBusy}>Cancel</button>
                </div>
              {:else}
                <div class="row deck-actions">
                  <button
                    class="control tip-right"
                    onclick={() => startNaming("create")}
                    disabled={deckBusy}
                    data-tip="Make a new, empty deck">Create</button
                  >
                  <button
                    class="control tip-right"
                    onclick={() =>
                      runDeck("anki_duplicate_deck", {
                        source: deckTarget,
                        target: copyName(decks, deckTarget),
                      })}
                    disabled={deckBusy || manageDecks.length === 0}
                    data-tip="Copy {deckTarget}'s settings to “{copyName(
                      decks,
                      deckTarget,
                    )}”. The cards stay where they are."
                    >Duplicate</button
                  >
                  <button
                    class="control tip-right"
                    onclick={() => startNaming("rename")}
                    disabled={deckBusy || !managed}
                    data-tip={managed
                      ? `Rename ${deckTarget}`
                      : `${deckTarget} holds cards 屠つ did not make`}>Rename</button
                  >
                  <button
                    class="control danger tip-right"
                    onclick={() => {
                      if (confirmingDelete) runDeck("anki_delete_deck", { name: deckTarget });
                      else confirmingDelete = true;
                    }}
                    disabled={deckBusy || !managed}
                    data-tip={managed
                      ? `Delete ${deckTarget} and its cards`
                      : `${deckTarget} holds cards 屠つ did not make`}
                    >{confirmingDelete ? deleteLabel : "Delete"}</button
                  >
                </div>
              {/if}
              {#if deckError}
                <p class="note warn">{deckError}</p>
              {:else}
                <p class="note">
                  <!-- Two things a learner cannot see from the deck list, and both change what the
                       buttons above will do. -->
                  {#if manageDecks.length === 0}
                    Only decks made here can be changed, and there are none yet. Create one.
                  {:else if !selected}
                    Duplicating copies a deck's settings, not its cards.
                  {:else if managed}
                    {deckTarget} holds {selected.ours}
                    {selected.ours === 1 ? "card" : "cards"}, all made here — so it can be renamed or
                    deleted.
                  {:else}
                    {deckTarget} was not made here, so it is left to Anki.
                  {/if}
                </p>
                {#if manageDecks.length === 0 || managed}
                  <details class="more">
                    <summary>More <span class="offscreen">about {FIND.anki.manage.label}</span></summary>
                    {#if manageDecks.length === 0}
                      <p>
                        Only decks made here can be changed, and there are none yet. Create one — decks
                        made in Anki stay Anki's, and so does Default.
                      </p>
                    {:else if selected}
                      <p>
                        {deckTarget} holds {selected.ours}
                        {selected.ours === 1 ? "card" : "cards"}, all made here — so it can be renamed or
                        deleted. Duplicating copies its settings, not its cards.
                      </p>
                    {/if}
                  </details>
                {/if}
              {/if}
            </div>
          </div>
        </div>
      {/if}
    </div>
    {/if}

    {#if section === "updates"}
      <div class="block" hidden={!some(FIND.updates.release)}>
        <Updates bind:enabled={checkUpdatesOnStartup} oninstall={oninstallupdate} />
      </div>
    {/if}

    {#if section === "diagnostics"}
    <div class="group">
      <div class="block" hidden={!some(FIND.diagnostics.engines, FIND.diagnostics.report)}>
        <h4 class="subhead">What is running</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.diagnostics.engines)}>
            {#if runtimeStatus}
              <dl class="engines" aria-label="Engines">
                {#each runtimeStatus.engines as engine (engine.role)}
                  {@const row = engineRow(engine, statusName)}
                  <dt>{row.role}</dt>
                  <dd>
                    <strong>{row.name}</strong>
                    {row.detail}
                    {#if (engine.role === "recognition" || engine.role === "translation") && engine.state === "ready"}
                      {@const role = engine.role}
                      <button class="control unload" onclick={() => unload(role)}
                        >Unload from memory <span class="offscreen">for {role}</span></button
                      >
                    {/if}
                  </dd>
                {/each}
                {#each [["Last recognition", runtimeStatus.recognition], ["Last analysis", runtimeStatus.analysis], ["Last translation", runtimeStatus.translation]] as const as [name, result] (name)}
                  <dt>{name}</dt>
                  <dd>{resultPhrase(result, statusName) ?? "none this session"}</dd>
                {/each}
              </dl>
              {#if unloadMessage}
                <p class="note" class:warn={unloadFailed} role={unloadFailed ? "alert" : "status"}>
                  {unloadMessage}
                </p>
              {/if}
            {:else if runtimeStatusFailed}
              <p class="note warn" role="alert">The engines could not be read. Reopen this section to try again.</p>
            {:else}
              <p class="note" role="status">Reading the engines…</p>
            {/if}
            <p class="note">Which engine each job uses and where it runs, as reported. Nothing is guessed.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.diagnostics.engines.label}</span></summary>
              <p>
                Each line says what the settings ask for, what is loaded, and where it runs: in the
                app, in a local service the app starts, or at an endpoint you set. "CPU" means the
                engine was built to run on the processor. "GPU acceleration active" with a layer
                count is what the PaddleOCR-VL server reported about its model's layers; it does not
                mean every step runs on the graphics card. Where nothing was reported, nothing is
                said. The last results are this session's, and a result served from the cache says
                so.
              </p>
            </details>
          </div>
          <div class="item" hidden={!hit(FIND.diagnostics.report)}>
            <div class="row">
              <span>Copy details for a bug report</span>
              <!-- Busy, never disabled: disabling the button that has focus throws keyboard
                   focus back to the page, and the reader then has to find their place again. -->
              <button class="control" onclick={previewReport} aria-busy={reportState === "loading"}>
                {reportState === "ready" ? "Refresh" : "Preview"}
              </button>
            </div>
            <p class="note">Versions, engines and setup only. Never your text, decks, folders or addresses.</p>
            {#if reportState === "loading"}
              <p class="note" role="status">Building the report…</p>
            {:else if reportState === "ready"}
              <!-- A read-only text area rather than a focusable `pre`: it is the control a
                   keyboard already knows how to scroll and select in. -->
              <textarea class="report" readonly rows="10" aria-label="Bug report details" value={report}></textarea>
              <div class="row">
                <span class="status" role="status">{reportCopied ? "Copied." : ""}</span>
                <button class="control" onclick={copyReport}>Copy</button>
              </div>
            {:else if reportState === "failed"}
              <p class="note warn" role="alert">The report could not be built: {reportError}</p>
            {/if}
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.diagnostics.report.label}</span></summary>
              <p>
                The report is assembled from a fixed list of facts: the app's version, the system,
                which engines are configured and loaded, what the last recognition, analysis and
                translation used, which resources are installed, and whether logging is on. It
                contains no passage, translation, screenshot, vocabulary, deck name, folder,
                address or error message. Nothing is sent anywhere: you preview it here, copy it,
                and paste it where you choose. The log file is separate, and shown below.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.diagnostics.performance)}>
        <h4 class="subhead">Session acceleration</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.diagnostics.performance)}>
            <div class="row">
              <span>Prepared results</span>
              <span class="status">
                {warmupDiagnostics
                  ? `${warmupDiagnostics.analysis_entries} Analysis · ${warmupDiagnostics.reference_entries} Reference`
                  : "not measured yet"}
              </span>
            </div>
            {#if warmupDiagnostics}
              <p class="note">
                {warmupDiagnostics.analysis_hits + warmupDiagnostics.reference_hits} visible requests used
                prepared answers; {warmupDiagnostics.analysis_misses + warmupDiagnostics.reference_misses}
                computed in the foreground. Approximately {warmupDiagnostics.estimated_saved_ms.toFixed(1)} ms
                of previously measured work was avoided this session.
              </p>
            {/if}
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.diagnostics.sentences)}>
        <h4 class="subhead">Your corpus</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.diagnostics.sentences)}>
            <label class="row">
              <span>Keep the Japanese you capture</span>
              <input type="checkbox" bind:checked={collectSentences} />
            </label>
            <p class="note warn">A file of every passage you capture — a record of what you have been reading.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.diagnostics.sentences.label}</span></summary>
              <p>
                Every capture that contains Japanese is appended to a file of its own, one passage per
                line, with duplicates dropped — so reading the same panel twice adds nothing. That file is
                a corpus: it is what a sentence the app read wrongly can be collected in, and what makes a
                report of a bad reading reproducible instead of remembered.
                <em>It is a record of what you have been reading</em>, which is worth knowing before you
                turn it on and worth turning off again when you are done.
              </p>
            </details>
          </div>

          <!-- **The count, not the size, and that is the whole reason this row is not the log's.**
               A corpus measured in kilobytes tells a reader nothing they can act on; how many
               passages they have gathered is exactly the thing that decides whether it is worth
               sending anywhere. The size follows it because a file that is growing is a file that
               is working, which is the log row's job and worth keeping. -->
          <div class="item" hidden={!hit(FIND.diagnostics.sentences)}>
            <div class="row">
              <span>Collected</span>
              <span class="status">
                {corpusFile?.passages
                  ? `${corpusFile.passages} passage${corpusFile.passages === 1 ? "" : "s"} · ${formatSize(corpusFile.bytes)}`
                  : "nothing yet"}
              </span>
            </div>
            <p class="note path">{corpusFile?.path ?? ""}</p>
            <div class="row">
              <button class="control" onclick={onrevealcorpus} disabled={!corpusFile?.exists}>
                Show the file
              </button>
            </div>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.diagnostics.log, FIND.diagnostics.file)}>
        <h4 class="subhead">Logging</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.diagnostics.log)}>
            <label class="row">
              <span>Write a log file</span>
              <input type="checkbox" bind:checked={logging} />
            </label>
            <p class="note">Off keeps no record at all. On writes what each step did and how long it took.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.diagnostics.log.label}</span></summary>
              <p>
                Off, Sakatsu Reader keeps no record of anything. On, it writes what it did and how
                long each step took — which engine ran, why a clipboard line was skipped, how long a
                capture took. That is what makes a problem describable after the fact instead of
                only while it is happening.
              </p>
            </details>
          </div>

          {#if logging}
            <div class="item" hidden={!hit(FIND.diagnostics.file)}>
              <div class="row">
                <span>File</span>
                <span class="status">{logFile ? formatSize(logFile.bytes) : "not written yet"}</span>
              </div>
              <p class="note path">{logFile?.path ?? ""}</p>
              <div class="row">
                <button class="control" onclick={onreveallog} disabled={!logFile?.exists}>Show the file</button>
              </div>
            </div>
          {/if}
        </div>
      </div>

      <div class="block" hidden={!some(FIND.diagnostics.console)}>
        <h4 class="subhead">The analyzer's console</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.diagnostics.console)}>
            <label class="row">
              <span>Show the analyzer's console window</span>
              <!-- **Stated as showing, not as being quiet, and the inversion is the point.** Hiding it
                   is the default and the ordinary state, so a switch labelled "quiet mode" spends its
                   life on and reads as a preference somebody ought to have an opinion about. Nobody
                   does. What is worth reaching for is the other direction — turning the window *on*,
                   once, because something is wrong — and a row says that plainly.

                   `checked={!quiet}` with an explicit handler rather than `bind:checked`, which cannot
                   take a negation. The stored field stays `quiet`, so nobody's saved setting flips. -->
              <input
                type="checkbox"
                checked={!quiet}
                onchange={(event) => (quiet = !event.currentTarget.checked)}
              />
            </label>
            <p class="note">Turn this on when the analyzer stops working for no visible reason.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.diagnostics.console.label}</span></summary>
              <p>
                Ichiran runs through WSL, which is a console program, so a black console window would
                flash up every single time text is analyzed. It is hidden, and the window is empty
                anyway — its output is captured and shown in the app rather than printed there.
                <em>Turn this on when the analyzer stops working for no visible reason.</em> That window
                is the only place WSL can ask you something, and while it is hidden the question is
                invisible and the app simply looks broken.
              </p>
            </details>
          </div>
        </div>
      </div>

      <div class="block" hidden={!some(FIND.diagnostics.restart)}>
        <h4 class="subhead">Restarting</h4>
        <div class="cluster">
          <div class="item" hidden={!hit(FIND.diagnostics.restart)}>
            <div class="row">
              <span>Restart Sakatsu Reader</span>
              <button class="control" onclick={onrestart}>Restart now</button>
            </div>
            <p class="note">Nothing here needs one; every setting applies as you change it.</p>
            <details class="more">
              <summary>More <span class="offscreen">about {FIND.diagnostics.restart.label}</span></summary>
              <p>
                Nothing here needs one. Every setting applies as you change it, including the
                analyzer, and a dictionary or table you fetch is loaded the moment it finishes. The
                button stays because restarting is still a reasonable thing to try when something
                has gone strange.
              </p>
            </details>
          </div>
        </div>
      </div>
    </div>
    {/if}

    </div>
  </div>
</section>

<style>
  /* **Positioned so the heading stays a direct child of `.page`.**
     `theme.css` styles the page title through `.page > h2`, and wrapping the heading and the
     button in a row would stop that matching, leaving this component to restate four
     properties it does not own. The button is lifted out of the flow instead. */
  .settings {
    position: relative;
    /* **Its own measure, centred.** The reading page fills the window by default (2026-10-23); a
       page of labelled controls does not want to. Full width put each label at the far left and its
       switch at the far right of a 1,624px card. 64rem is the nav plus a comfortable column of
       cards, and the measure Glossary takes too. Narrower windows never reach it. */
    width: 100%;
    max-width: 64rem;
    margin-inline: auto;
    /* What the narrow layout below asks about: the page's width, which the Interface size changes
       as much as the window does. Here and not on `.body`, because a container query styles a
       container's descendants and never the container — `.body` has to change direction, so the
       question is asked one level up. Not the scroller (that is `.pane`), per `traps.md`. */
    container: settings / inline-size;
  }
  /* `.exit` is in `theme.css` — Help draws the same button. */

  /* **No hover state, deliberately.** It had one and it read as a glow beside the page title,
     which is a lot of movement for a control you are leaving by. Nothing is lost: the pointer
     still turns, and `data-tip` still puts the destination under it, so the button says it is a
     button without lighting up to do it. The transition went with the hover — there is
     nothing left for it to animate. */

  /* **The search sits above both columns, and it is the page's, not the pane's.** It searches all
     seven sections; putting it inside the one on screen would say it searched that one.

     Full width rather than beside the heading, because the results hang off its bottom edge and a
     narrow box would drop a list of two-line results into a column too tight to read. */
  /* **One `.find` block, and there were two.** The drawer's header had its own further down this
     file with `flex: 1` on it, to take the width the title used to sit in. Both selectors matched;
     the later one won; and a `flex: 1` label in a *column* grew to fill the page, leaving a 350px
     band of nothing under a 30px box. Nothing catches that — both rules are used, so the
     unused-selector pass is silent. `traps.md` says to grep the file for a class name before
     adding it to a component this long, and this is the entry earning its keep. */
  .find {
    flex: 0 0 auto;
    display: block;
  }
  .find input {
    width: 100%;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: 0.85rem;
    padding: 0.35rem 0.6rem;
  }
  .find input::placeholder {
    color: var(--text-faint);
  }

  /* What a search found, across the sections. A list, not a grid: the section is the smaller half
     of each line and reads as a qualifier on the row's name rather than a column of its own. */
  .results {
    flex: 0 0 auto;
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 40vh;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
  }
  .result {
    display: flex;
    align-items: baseline;
    gap: var(--gap-sm);
    width: 100%;
    padding: 0.3rem var(--gap);
    background: none;
    border: 0;
    text-align: left;
    cursor: pointer;
    color: var(--text);
  }
  .result:hover,
  .result:focus-visible {
    background: var(--accent-wash);
  }
  .result .in {
    flex: 0 0 auto;
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
    /* Wide enough for "Diagnostics" so the row names line up down the list. A ragged left edge on
       the half you are actually reading is what makes a result list hard to scan. */
    min-width: 6.5rem;
  }
  .none {
    padding: 0.3rem var(--gap);
    color: var(--text-dim);
  }

  /* Two columns: where you can go, and where you are. */
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: var(--gap-lg);
  }

  /* **The nav the title bar's menu duplicates, and the duplication is the point.** The menu is how
     you arrive on a section; this is how you move once you are here, without going back up to the
     title bar for every hop.

     Scrolls on its own so a short window does not put Diagnostics out of reach while the pane
     beside it has room to spare. */
  .sections {
    flex: 0 0 11rem;
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow-y: auto;
    /* Matches `.page-body`'s top padding so the first section lines up with the heading of
       the section it opens, rather than sitting a few pixels above it. */
    padding-top: var(--gap-sm);
    padding-bottom: var(--gap-lg);
  }
  .sections .tab {
    /* Left-aligned and full width: this is a list of places, and a centred label in a full-width
       button reads as a row of buttons that happen to be stacked. Flex for the icon in front. */
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    width: 100%;
    text-align: left;
    padding: 0.35rem var(--gap);
    background: none;
    border: 0;
    border-radius: var(--radius-sm);
    color: var(--text-dim);
    cursor: pointer;
    transition:
      background var(--anim) var(--ease),
      color var(--anim) var(--ease);
  }
  .sections .tab:hover {
    color: var(--text);
    background: var(--surface-sunken);
  }
  .sections .tab.current {
    background: var(--surface);
    color: var(--text);
    font-weight: 600;
  }
  /* In `rem`, so it follows the Interface size with the label beside it — `traps.md` records the
     icon buttons that stayed eighteen device pixels while everything around them grew. Fainter
     than the label until the section is the one open; the drawing is a landmark, not the name. */
  .section-icon {
    flex: 0 0 auto;
    width: 1.1rem;
    height: 1.1rem;
    color: var(--text-faint);
  }
  .sections .tab:hover .section-icon,
  .sections .tab.current .section-icon {
    color: var(--accent);
  }
  /* The label says it already, so the tooltip is only for the strip below, where it does not.

     **The `aria-label` on each button is not redundant.** A tooltip is `::after` with
     `content: attr(data-tip)`, and generated content counts toward a name computed from content —
     shown in the strip, it made every button "General General". Named outright, a button is
     called its section whichever layout is drawing it. */
  .sections .tab::after {
    display: none;
  }

  /* **Narrow, the nav becomes a strip of icons above the pane.** An 11rem column beside the pane
     left the pane 184px at the 480px window, and 12px once the Interface size was 24px — every
     card a column of single letters. Under 31rem the nav lies across the top instead, icons only,
     and the pane gets the whole width. Each button keeps its name: the label is hidden the way
     `.offscreen` hides things, not removed, and the tooltip says it on hover. The pane's own
     heading names the section you are in. */
  @container settings (max-width: 31rem) {
    .body {
      flex-direction: column;
      gap: var(--gap);
    }
    .sections {
      flex: 0 0 auto;
      flex-direction: row;
      flex-wrap: wrap;
      gap: var(--gap-xs);
      overflow: visible;
      padding: 0;
    }
    .sections .tab {
      width: auto;
      padding: 0.4rem;
    }
    .sections .tab::after {
      display: block;
    }
    .tab-label {
      position: absolute;
      width: 1px;
      height: 1px;
      margin: -1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .pane {
      padding-right: var(--gap-sm);
    }
  }

  /* **The section's own title, and a line on what it is for.** It replaced a 0.72rem uppercase
     eyebrow per section that named the page in its smallest type; the page you are on deserves
     the largest heading in the pane, the way the title bar's page names are the largest in theirs. */
  .pane-head {
    margin: 0 0 var(--gap-lg);
  }
  .pane-head h3 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 600;
    color: var(--text);
  }
  .pane-head p {
    margin: 0.2rem 0 0;
    font-size: 0.82rem;
    color: var(--text-dim);
    line-height: 1.5;
  }

  /* The section itself, and the only part that scrolls with the settings in it.
   *
   * **The right margin matches the left one, and it is not `.page-body`'s.** That shared rule
   * gives every page `--gap-sm` at the right edge, which is the correct amount when the only
   * thing on the other side is the window. Here there is a nav, held off by `.body`'s
   * `--gap-lg`, and 19.5px on one side against 6.75px on the other reads as the content having
   * drifted toward the scrollbar. Matching them is what makes the column look placed rather
   * than pushed.
   *
   * Scoped here rather than changed in `theme.css`: Reference and Glossary have no nav beside
   * them, so there is nothing for their right edge to be equal *to*. */
  .pane {
    flex: 1;
    min-width: 0;
    padding-right: var(--gap-lg);
  }

  /* **A block is a heading and a card, and a card holds one item per setting.**
   *
   * The card is what the rule above each heading used to be — the thing that says where one idea
   * stops — drawn as a container instead of a line, so the rows inside read as belonging together
   * rather than as one long column with occasional dividers. koharu's preferences are the model.
   *
   * The heading sits outside its card, small, and the card does the grouping. It used to carry a
   * rule above it as the divider, which needed `:first-of-type` to drop the first one and a
   * `.searching` override for when search hid that one — DOM order is not what is on screen. A
   * card has no such edge case, and the rules went with the line.
   *
   * Under a search a block with no surviving rows hides whole, heading and card together — see
   * `some()` and the block-keys test in `settings-notes.test.mjs`. */
  .block {
    margin: 0 0 var(--gap-lg);
  }
  .block > h4 {
    margin: 0 0 var(--gap-sm);
    padding-left: 0.1rem;
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    color: var(--text-dim);
    font-weight: 600;
  }
  .cluster {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: 0 var(--gap);
  }

  /* One setting: its row, its one line, and its More. The row and the line sit close, because
     the line is about the row; the item's padding is what separates it from the next one.

     **A grid, so a closed More shares its note's line.** On a line of its own, "More" cost every
     setting the line the short note had saved — measured at 900px, Appearance came out 14%
     taller than the paragraphs it replaced. So the note takes the first column and the closed
     More sits at the end of it; opened, the More takes a full-width row of its own below. Every
     other child spans both columns — `:global`, because a child component's root (a recorder,
     a setup panel) carries none of this file's scoping class and would otherwise be flowed into
     a column. */
  .item {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: var(--gap);
    row-gap: 0.3rem;
    padding: 0.6rem 0;
  }
  .item > :global(*) {
    grid-column: 1 / -1;
  }
  .item > .note:has(+ .more:not([open])) {
    grid-column: 1;
  }
  .item > .note + .more:not([open]) {
    grid-column: 2;
    align-self: start;
  }

  /* **A divider above every visible item but the first — counted from what is on screen.**
     `:first-child` would be DOM order, and under a search the first item is often hidden, which
     would leave the first *visible* one wearing a rule against the card's own top edge. Each item
     after a visible one draws the line; hidden ones draw nothing because they are not drawn. No
     `overflow: hidden` on the card to tidy the corners, either: a tooltip inside it would be cut
     off at its edge (`traps.md`). */
  .cluster > .item:not([hidden]) ~ .item {
    border-top: 1px solid var(--border);
  }

  /* **The reasoning, folded, and the word that opens it is all you see.** Smaller and fainter than
     the line above it, because it is the part you go looking for rather than the part you read.
     The `summary` stays a `list-item` — `traps.md`'s most expensive entry — so the marker stays.
     Its accessible name is longer than what it shows: "More about <the setting>", from hidden
     text inside it, because forty buttons all called "More" are a list nobody can use. */
  .more {
    font-size: 0.72rem;
    color: var(--text-faint);
  }
  .more summary {
    width: fit-content;
    cursor: pointer;
    /* The note's own line height, so a More beside it sits on the same baseline. */
    line-height: 1.55;
  }
  .more summary:hover {
    color: var(--text-dim);
  }
  /* **Out of layout while shut, not only out of sight.** A closed `<details>` keeps laying its
     content out in Chromium — invisible, but boxed — and a closed More sits in the item's narrow
     second column, so its paragraphs were being set 37px wide and reported by the containment
     check as text spilling out of the card. */
  .more:not([open]) > p {
    display: none;
  }
  .more p {
    margin: 0.3rem 0 0;
    line-height: 1.55;
    color: var(--text-dim);
  }
  .more em {
    font-style: italic;
  }

  /* **Wraps, because a row has a floor and the pane does not.** `input[type="range"]` keeps
     `min-width: 9rem` so a slider never shrinks to uselessness, and a flex item's automatic
     minimum stops the label beside it collapsing either — so at the 480px window the pane is
     184px and the four size rows needed 188-204px. They overflowed rather than wrapping,
     which put their content over the edge of the group.

     Wrapping costs nothing where the row already fits: it only ever engages once the line is
     too full, and the alternative there is spilling. The slider then takes its own line at
     full width, which is better resolution than squeezing it would have given.

     Found by `ui/tests/layout/contained.ts`, not by eye. */
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--gap-sm);
    font-size: 0.85rem;
    color: var(--text-dim);
  }
  .row em {
    font-style: normal;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--text-faint);
  }
  .row.disabled {
    opacity: 0.5;
  }

  /* **A slider is as long as the row can spare, because length is resolution.**
   *
   * This was `flex: 0 0 9rem` — a fixed 135px track — while the row it sat in was 570px and a
   * third of it was empty. A hue runs 0–359, so 359 stops were being drawn across about 119px of
   * usable track: **three degrees per pixel of drag.** `step="1"` was already right and made no
   * difference, because the step was never what was limiting it. You cannot land on a value the
   * control has no pixel for.
   *
   * `26rem` is arithmetic, not taste. The widest range here is the 359 stops of a hue; a thumb is
   * about 16px and never reaches either end, so one stop per pixel needs 359 + 16 = 375px, and
   * 26rem is 390px at the default 15px root. Past that the extra length buys nothing a person can
   * use, and rows start reading as a wall of tracks.
   *
   * The `9rem` basis survives as the floor: a narrow window shrinks the slider back to what it
   * always was rather than squeezing the label out. Resolution is worse there, and arrow keys —
   * which have always moved exactly one step — are the way to a precise value. */
  /* Layout only — the colour is `theme.css`'s, shared with the title bar's menu. */
  input[type="range"] {
    flex: 1 1 9rem;
    min-width: 9rem;
    max-width: 26rem;
  }
  /* No `input[type="checkbox"]` rule here any more. `theme.css` draws the switch itself, and
     `accent-color` only ever reached the platform's checkbox — which `appearance: none` retired.
     Deleted rather than left to be shadowed: a component's `input[type="checkbox"]` compiles to
     `input[type="checkbox"].svelte-hash` and is injected after `theme.css`, so it would still
     have won the ties. See traps.md. */
  /* The OCR field carries this same geometry inside `OcrSelect`; scoped styles cannot cross
     that component boundary, while the other settings selects still live here directly. */
  /* **A `<select>` is as wide as its widest option, and will not shrink** — measured at 129px on
   * a short deck name and 397px once a deck called
   * `CompTIA Security+ (SY0-601)::4.0 Operations and Incident Response` is in the list. Its
   * intrinsic width comes from the longest `<option>` rather than the chosen one, and a flex
   * item's `min-width` defaults to `auto`, so the row cannot squeeze it.
   *
   * Defensive rather than a fix for anything observed: 397px still fits the pane at the widths
   * measured, so this was **not** what put a scrollbar under the settings page — that was the
   * tooltips, see `traps.md`. It is kept because the width comes from a learner's collection
   * rather than from anything here, and a longer deck name in a narrower window is a real
   * possibility nobody would think to test.
   *
   * The chosen value is all a `<select>` shows, so a long name is clipped by the control. */
  /* The box is `theme.css`'s now; what stays here is this page's own type size. */
  select {
    font-size: 0.8rem;
    /* A long option — a deck name from somebody's collection — may not widen the card. */
    max-width: 100%;
  }

  .status {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--good);
  }
  .status.missing {
    color: var(--red);
  }

  /* **Light, Dark, System as three segments, and the chosen one wears the "on" idiom** —
     `controls.md`'s shape for a choice with three states. Labels styled as `.control`, because the
     radio inside each is `.offscreen` and the label is the thing you see and press. The ring is the
     app's `:focus-visible` treatment moved onto the label, since the focused element itself is
     not drawn. Wraps, so a narrow pane puts System on a second line rather than past the edge. */
  .mode {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--gap-xs);
  }
  .mode label {
    padding: 0.2rem 0.7rem;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .mode label.on {
    color: var(--accent);
    background: var(--accent-wash);
    border-color: var(--accent-edge);
  }
  .mode label:has(:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  /* Four buttons acting on one deck, so they group at the end rather than spreading across
     the row the way a label-and-control pair does. Wrapping because the delete button grows
     when it is armed — "Delete 12 cards?" is wider than "Delete", and a row that cannot wrap
     would push the others off instead. */
  .deck-actions {
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  /* The per-token panel, folded because it is the sharp knife.
     **No `display: flex` on the summary**, which would delete the disclosure triangle — the
     trap `traps.md` records as having cost three separate times. It stays a `list-item`. */
  .tokens {
    margin: 0;
  }
  .tokens summary {
    font-size: 0.85rem;
    color: var(--text-dim);
    cursor: pointer;
  }
  /* Inside an open fold the rows have no item to space them, so they space themselves. Margins,
     not `display: grid` on the `<details>` — which does not lay out a disclosure's content
     (`traps.md`). */
  .tokens > .row,
  .tokens > .note,
  .tokens > .more {
    margin-top: var(--gap-sm);
  }
  /* Mono, because these are token names rather than prose and lining them up is what makes a
     dozen of them scannable. */
  .token-row span:first-child {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--text-faint);
  }
  .token-set {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
  }
  /* The platform draws the well itself; this only gives it a size and the app's edge. */
  /* The six as the senses will wear them, with the accent last to compare against. Squares rather
     than the round wells above, because these are a readout and not a control — nothing here is
     clickable, and a swatch that looks like the pickers would invite a click that does nothing. */
  /* Wraps, because seven words is more than seven squares and a narrow settings pane is real. */
  .kind-strip .swatches { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 0.2rem var(--gap-sm); }
  /* The treatment a kind wears in a sense list — `Term.svelte`'s badge, minus the button. Same
     face, same case, same letterspacing, so the preview is drawn the way the thing it previews is. */
  .kind-strip .swatch {
    font-family: var(--font-display);
    font-size: 0.63rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  /* Set apart, because it is the thing the others are being measured against rather than one of
     them. The gap says so without needing a label. */
  .kind-strip .swatch.accent { margin-left: var(--gap-sm); }
  .token-set input[type="color"] {
    width: 2.2rem;
    height: 1.3rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: none;
    cursor: pointer;
  }
  /* One finding per line: several ratios running together read as a paragraph nobody parses. */
  .warned {
    display: block;
  }

  /* A setting's one line. Closer to its row than to the next setting, which is the item's job
     now: the row and the note sit `0.3rem` apart inside it and the item's padding separates it
     from the next — a note equidistant between two rows reads as belonging to neither.

     **One line of copy, not one rendered line.** `settings-notes.test.mjs` holds the copy to 110
     characters; at the largest Interface size in a narrow window that still wraps, and it is
     free to. Nothing here may clip it — no `nowrap`, no ellipsis, no fixed height — and
     `settings-layout.spec.ts` checks that at both ends of the Interface slider. */
  .note {
    margin: 0;
    padding-left: 0.1rem;
    font-size: 0.72rem;
    color: var(--text-faint);
    line-height: 1.55;
  }

  /* The bind recorders are a child component, so reaching them needs `:global`. Scoped to an
     item so it cannot leak to anything else that grows a `.bind`. The item spaces them now, so
     what is left is the row's type size — a recorder is a row, and should read as one beside
     the sliders under it rather than a step larger. */
  .item :global(.bind) {
    font-size: 0.85rem;
  }
  /* Red is for the things that want attention. A caveat about a model that can be
     confidently wrong is one of them. */
  .note.warn {
    color: var(--red);
  }

  /* Local layout only — the colours and spacing are the section's existing tokens. A dictionary
     and what it is missing read as one line each, the way the downloaded-resources list does. */
  .dictionary-upkeep ul {
    margin: 0 0 var(--gap);
    padding-left: 1.1rem;
  }
  .dictionary-upkeep li {
    font-size: 0.78rem;
  }
  .dictionary-upkeep li small {
    display: block;
    color: var(--text-faint);
    font-size: 0.72rem;
  }
  .note em {
    font-style: italic;
  }

  /* Selectable, and wrapping, because the point of showing it is that someone can read
     it out or select it. There is deliberately no copy button: replacing the clipboard is
     expected beside a reference headword, but would be a surprise beside a diagnostic path. */
  .note.path {
    font-family: var(--font-mono);
    font-size: 0.68rem;
    overflow-wrap: anywhere;
    user-select: text;
  }

  /* The engines, one per line: the job on the left, what runs it on the right. A grid of two
     columns rather than rows of `.row`, because each answer wraps and a label beside a wrapped
     answer has to stay at its top. */
  .engines {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: var(--gap-xs) var(--gap);
    margin: var(--gap-sm) 0;
    font-size: 0.85rem;
  }
  .engines dt {
    color: var(--text-dim);
  }
  .engines dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .engines .unload {
    margin-left: var(--gap-sm);
    font-size: 0.75rem;
    padding: 0.1rem 0.5rem;
  }

  /* The report as the reader will paste it: monospaced, and wrapped rather than scrolled
     sideways. Resizable downwards only, so it can never push the card wider. */
  .report {
    display: block;
    box-sizing: border-box;
    width: 100%;
    margin: var(--gap-sm) 0 0;
    padding: var(--gap-sm);
    resize: vertical;
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.72rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--surface-sunken);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    user-select: text;
  }

  /* Every button in a row, rather than one class per button. "Show the file" had this and
     Restart had nothing, so it rendered as the browser's default in a panel where nothing
     else does. */
  /* Size only; the rest is `.control` in theme.css. */
  .row button {
    font-size: 0.75rem;
    padding: 0.2rem 0.6rem;
  }
  /* What setup has done, and the buttons that act on it, on one wrapping line — an item laid
     out across rather than down, because the status and its actions are one thing. The box it
     used to draw for itself is the card's now. */
  .item.installation-status {
    display: flex;
    flex-direction: row;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--gap-sm);
  }
  .installation-status > div {
    flex: 1 1 15rem;
    display: grid;
    gap: 0.12rem;
  }
  .installation-status span {
    color: var(--text-dim);
    font-size: 0.76rem;
  }
  .installation-status button,
  .installation-details button {
    padding: 0.2rem 0.55rem;
    font-size: 0.74rem;
  }
  .installation-details {
    color: var(--text-dim);
    font-size: 0.8rem;
  }
  .installation-details summary { cursor: pointer; color: var(--text-faint); }
  .installation-details ul { list-style: none; margin: var(--gap-sm) 0 0; padding: 0; }
  .installation-details li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: var(--gap-sm);
    align-items: center;
    padding: var(--gap-xs) 0;
    border-bottom: 1px solid var(--border);
  }
  .installation-details small { color: var(--text-faint); }
  .resource-location { display: grid; gap: var(--gap-xs); margin: var(--gap-sm) 0; }
  /* Wraps, so the two buttons drop under a path field that has taken the width rather than
     pushing past the card at the largest Interface size. */
  .resource-location > div { display: flex; flex-wrap: wrap; gap: var(--gap-sm); }
  .resource-location input {
    flex: 1 1 12rem;
    min-width: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    color: var(--text);
    padding: 0.35rem 0.5rem;
  }
  .remove-old { display: flex; align-items: center; gap: var(--gap-sm); margin-bottom: var(--gap-sm); }
  /* One download per item, laid out across: what it is, then what you can do about it. The box
     each used to draw for itself is the card's now. */
  .item.optional-resource {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: var(--gap);
  }
  .optional-resource > div:first-child { flex: 1 1 15rem; min-width: 0; display: grid; gap: 0.1rem; }
  .optional-resource span,
  .optional-resource small { color: var(--text-dim); font-size: 0.74rem; }
  /* The name of a download or of setup's state reads at a row label's size: it is the label of
     its item, and at the body size it outweighed every setting around it. Same for the buttons. */
  .optional-resource strong,
  .installation-status strong {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text);
  }
  .optional-resource button {
    font-size: 0.75rem;
    padding: 0.2rem 0.6rem;
  }
  .optional-resource .installed { color: var(--good); }
  .optional-resource .missing { color: var(--red); }
  .optional-progress { display: grid; grid-template-columns: minmax(5rem, 8rem) auto auto; gap: var(--gap-sm); align-items: center; }
  .optional-progress progress { width: 100%; accent-color: var(--accent); }
  /* What a paused download kept, and its two buttons. Wraps, because "Discard partial download"
     beside "Resume" does not fit a narrow card on one line. */
  .optional-saved {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--gap-xs) var(--gap-sm);
    min-width: 0;
  }
  @media (max-width: 32rem) {
    .item.optional-resource { align-items: stretch; flex-direction: column; }
    .optional-resource > button { align-self: flex-start; }
    .optional-progress { grid-template-columns: minmax(0, 1fr) auto auto; }
  }
  code {
    /* A path has nowhere to break, and an install folder under a long account name is wider
       than the card. */
    overflow-wrap: anywhere;
    font-family: var(--font-mono);
    background: var(--surface-raised);
    padding: 0.05rem 0.3rem;
    border-radius: var(--radius-sm);
  }
</style>
