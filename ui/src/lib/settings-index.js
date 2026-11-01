/**
 * What the settings search searches, and which section holds each thing.
 *
 * # Why the keywords are authored rather than scraped
 *
 * Matching rendered text would tie the search to the exact words a label happens to use —
 * "hotkey" and "shortcut" find nothing when the label says "Capture" — and matching the
 * explanatory notes as well would drag half the panel up on any common word. So the keywords say
 * what a setting is *about*, including the words it does not print.
 *
 * The label is the newer half. As a drawer, search hid the rows that did not match and the
 * matching label was already on screen; a page has to be able to *name* a match two sections
 * away, so the name has to live in the index rather than only in the markup.
 *
 * # Why this is a module and not part of the component
 *
 * **Because the index says where a row lives, and the markup decides where it actually is.** Those
 * are two statements of the same fact in two places, and they disagree silently: a row indexed
 * under Appearance and rendered under Diagnostics is found by search, announced as being in
 * Appearance, and is not there when you look. Nothing errors. Out here `settings-index.test.mjs`
 * can read the component and check the two agree — which is the whole reason it moved.
 *
 * @typedef {{ label: string, keywords: string }} Row
 * @typedef {Record<string, Record<string, Row>>} Index
 */

/**
 * Every searchable row, by section id then row key.
 *
 * The section ids are `SETTINGS_SECTIONS` in `model.ts`; the row keys are what the markup writes
 * as `hit(FIND.section.key)`.
 *
 * @type {Index}
 */
export const FIND = {
  reading: {
    markSeen: {
      label: "Mark focused words as Seen",
      keywords:
        "mark focused selected words seen learning status click arrow keys automatic reading",
    },
    keyboard: {
      label: "Windows keyboard navigation",
      keywords:
        "windows keyboard navigation tab focus traversal accessibility roving selected word controls",
    },
    lineBreaks: {
      label: "Keep the capture's line breaks",
      keywords:
        "line breaks newlines wrap reflow paragraph one line scroll height passage shape bubble",
    },
    guesses: {
      label: "Mark uncertain readings",
      keywords:
        "furigana reading guess guessed uncertain unsure dotted underline caveat ambiguous check",
    },
    parts: {
      label: "Show a word's parts",
      keywords: "word parts components grouped compound split pieces breakdown look up lookup",
    },
    sizes: {
      label: "Interface, analyzed text and definition sizes",
      keywords:
        "interface size font text scale bigger smaller analyzed definitions panel zoom resize",
    },
    colours: {
      label: "Word and selection colours",
      keywords:
        "colour color word status new seen known ignored underline rule button chip selection selected highlight accent detach custom",
    },
    analyzer: {
      label: "Analyzer",
      keywords:
        "analyzer ichiran built-in automatic parsing segmentation engine offline local advanced",
    },
    preloadIchiran: {
      label: "Preload Ichiran at startup",
      keywords: "ichiran preload startup launch start background warm first passage load latency memory",
    },
    ichiranServer: {
      label: "Keep Ichiran running between passages",
      keywords:
        "ichiran persistent server session speed latency slow fast performance keep running warm restart process sidecar memory wsl",
    },
    extras: {
      label: "Optional downloads",
      keywords:
        "optional downloads translation pronunciation audio frequency jpdb bccwj models features",
    },
  },
  appearance: {
    theme: {
      label: "Theme",
      keywords:
        "theme mode light dark system colour color palette appearance night preset",
    },
    hues: {
      label: "Interface and accent colour",
      keywords:
        "hue colour color accent tint surface warm cool purple blue green custom",
    },
    dials: {
      label: "Tint strength, contrast and depth",
      keywords:
        "tint saturation contrast depth darkness lighter darker grey gray legibility",
    },
    kinds: {
      label: "Kind hues",
      keywords:
        "kind kinds part of speech pos noun verb adjective adverb expression function particle sense colour color hue rotate turn spin coding",
    },
    kindColours: {
      label: "Set each part of speech",
      keywords:
        "kind noun verb adjective adverb expression function particle individual colour color hex override reset sense",
    },
    tokens: {
      label: "Set individual colours by hand",
      keywords:
        "token override custom hex individual colour color advanced expert reset",
    },
    animation: {
      label: "Animate the interface",
      keywords: "animate animation interface motion speed duration",
    },
    decor: {
      label: "Decoration — clouds and blossom",
      keywords:
        "decoration decorative watermark pattern clouds kasumi sakura blossom petals opacity tint size scale bigger smaller padding gap spread spacing apart density closer further blur blurry soften soft softness haze hazy focus defocus sharp sharpness crisp show hide on off toggle switch turn",
    },
  },
  capture: {
    captureBind: {
      label: "Capture",
      keywords: "capture hotkey shortcut key binding read frame",
    },
    overlayBind: {
      label: "Show / hide the overlay",
      keywords: "show hide overlay hotkey shortcut key binding frame",
    },
    hold: {
      label: "Hold to hide",
      keywords: "hold hide delay overlay frame binding",
    },
    lockTap: {
      label: "Double tap to lock",
      keywords: "double tap lock frame overlay binding pin place threshold",
    },
    clickDelay: {
      label: "The overlay button's wait for a second click",
      keywords: "overlay button click double click delay defer wait",
    },
    hideDuring: {
      label: "Hide the frame while capturing",
      keywords: "hide frame while capturing flicker border screenshot",
    },
    switchBind: {
      label: "Switch the overlay off / on",
      keywords:
        "switch overlay off on disable enable hotkey shortcut key binding",
    },
    frameChrome: {
      label: "Tint the region",
      keywords: "overlay frame tint wash colour color region highlight",
    },
  },
  search: {
    indexes: {
      label: "Update dictionary indexes",
      keywords:
        "dictionary import publisher automatic updates index indexes rebuild update dictionary definitions spellings redirect cross reference "
        + "maintenance upgrade sequence gloss kanji offline local no download repair arrow",
    },
    frequency: {
      label: "Order results by",
      keywords:
        "frequency common order sort rank corpus jpdb bccwj results reference",
    },
    frequencySource: {
      label: "Show the frequency source",
      keywords:
        "show hide frequency source list corpus jpdb bccwj results reference label",
    },
    direction: {
      label: "Frequency numbers are",
      keywords: "rank count direction frequency numbers backwards corpus advanced",
    },
    credits: {
      label: "Name the source",
      keywords:
        "source dictionary credit attribution jitendex jmnedict badge chip name",
    },
  },
  recognition: {
    installation: {
      label: "Repair screen capture installation",
      keywords:
        "installation setup repair resume pause screen capture ocr downloads verify models",
    },
    storage: {
      label: "Move downloaded resources",
      keywords:
        "move downloaded resources storage location drive localappdata disk space models dictionaries",
    },
    engine: {
      label: "Engine",
      keywords: "recognition engine ocr auto automatic manga paddle model",
    },
    loaded: {
      label: "Loaded model",
      keywords: "loaded engine status installed model",
    },
    prepare: {
      label: "Image preparation",
      keywords:
        "image preparation enlarge upscale trim margin crop local contrast",
    },
  },
  anki: {
    deck: {
      label: "Deck",
      keywords: "anki ankiconnect deck cards flashcards export",
    },
    filter: {
      label: "Hide decks that aren't Sakatsu's",
      keywords: "anki deck filter hide narrow list only mine sakatsu",
    },
    manage: {
      label: "Manage decks",
      keywords:
        "anki deck new create duplicate copy rename delete remove manage",
    },
  },
  diagnostics: {
    engines: {
      label: "What is running",
      keywords:
        "engine engines status running device gpu cpu acceleration vulkan layers loaded recognition analysis translation analyzer ocr model which",
    },
    report: {
      label: "Copy details for a bug report",
      keywords:
        "bug report issue copy details diagnostics version system support problem help share",
    },
    performance: {
      label: "Session acceleration",
      keywords:
        "performance speed latency preload prefetch warmup cache analysis reference diagnostics session",
    },
    log: {
      label: "Write a log file",
      keywords: "log file diagnostics write record",
    },
    sentences: {
      label: "Keep the Japanese you capture",
      keywords:
        "corpus collect keep sentences captured japanese text ocr recognized gather sample record privacy log content report misread",
    },
    file: { label: "The log file", keywords: "log file size path show reveal" },
    console: {
      label: "Show the analyzer's console window",
      keywords:
        "quiet mode console window wsl ichiran flash sidecar analyzer debug diagnostic prompt hidden show",
    },
    restart: {
      label: "Restart",
      keywords: "restart reload relaunch apply analyzer parser needs restart",
    },
  },
  updates: { release: { label: "App updates", keywords: "update updates release version download install restart automatic startup daily" } },
};

/**
 * Does this row match what has been typed?
 *
 * An empty box matches everything, so a section is whole until something is typed.
 *
 * @param {Row} row
 * @param {string} needle already trimmed and lower-cased
 * @returns {boolean}
 */
export function matches(row, needle) {
  return needle === "" || row.keywords.includes(needle);
}

/**
 * Does a subheading have anything under it right now?
 *
 * A divider is a promise that something follows it. Under a search that hides most rows, a
 * heading whose whole cluster is filtered out is a rule and a label with nothing beneath —
 * which reads as a section that failed to load rather than one that did not match.
 *
 * @param {string} needle already trimmed and lower-cased
 * @param {Row[]} rows every row the heading covers
 * @returns {boolean}
 */
export function anyMatch(needle, rows) {
  return rows.some((row) => matches(row, needle));
}

/**
 * Every row that matches, named with the section holding it.
 *
 * @param {string} needle already trimmed and lower-cased
 * @param {readonly { id: string, label: string }[]} sections `SETTINGS_SECTIONS`
 * @returns {{ section: string, sectionLabel: string, label: string }[]}
 */
export function search(needle, sections) {
  if (needle === "") return [];
  const out = [];
  for (const entry of sections) {
    for (const row of Object.values(FIND[entry.id] ?? {})) {
      if (matches(row, needle)) {
        out.push({
          section: entry.id,
          sectionLabel: entry.label,
          label: row.label,
        });
      }
    }
  }
  return out;
}
