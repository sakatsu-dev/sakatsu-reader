/**
 * The palette, derived rather than written out.
 *
 * A theme is a handful of inputs — a mode and two hues — and every colour token falls out of
 * them. That is what makes a second mode possible at all: light is not a second copy of the
 * stylesheet, it is a second set of stops fed through the same derivation.
 *
 * ## Why OKLCH, and why the stops are data
 *
 * The surface tokens encode a perceptual claim: *a thing is lighter because it is nearer*. HSL
 * does not preserve that across hues — yellow at 50% lightness reads far brighter than blue at
 * 50% — so rotating the surface hue in HSL would silently reorder the layers. OKLCH does preserve
 * it, which is the whole reason for the conversion below.
 *
 * The stops themselves were **measured from the palette that already existed** rather than
 * invented, so the dark theme this produces is the dark theme the app has always had, to the
 * byte. `theme.test.mjs` asserts exactly that. Each stop carries its own lightness, chroma and a
 * small hue offset from its family, because the hand-picked originals wander a degree or two and
 * reproducing them means keeping that.
 *
 * ## Output is hex, not `oklch()`
 *
 * The browser would happily render `oklch()`, but then the value the app displays and the value
 * this file measured contrast against would be two different computations, and gamut mapping
 * sits between them. Converting here means one answer, and it means the default dark theme can be
 * asserted against the literal values it replaced.
 */

// --- Colour space ---------------------------------------------------------------------------
//
// Björn Ottosson's Oklab, and the sRGB transfer function either side of it. Small enough to hold
// rather than take a dependency for, and a dependency here would be a supply-chain question for
// a page of arithmetic.

/** @param {number} c a linear channel, 0–1 @returns {number} the sRGB channel, 0–1 */
function toGamma(c) {
  return c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
}

/** @param {number} c an sRGB channel, 0–1 @returns {number} the linear channel, 0–1 */
function toLinear(c) {
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

/**
 * OKLCH to linear sRGB, unclamped.
 *
 * Unclamped on purpose: the caller decides what to do about a colour outside the gamut, and
 * silently clipping here would hide a stop that had drifted out of it.
 *
 * @param {number} l lightness, 0–1
 * @param {number} c chroma
 * @param {number} h hue in degrees
 * @returns {[number, number, number]} linear r, g, b
 */
function oklchToLinear(l, c, h) {
  const rad = (h * Math.PI) / 180;
  const a = c * Math.cos(rad);
  const b = c * Math.sin(rad);

  const lc = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const mc = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const sc = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;

  return [
    4.0767416621 * lc - 3.3077115913 * mc + 0.2309699292 * sc,
    -1.2684380046 * lc + 2.6097574011 * mc - 0.3413193965 * sc,
    -0.0041960863 * lc - 0.7034186147 * mc + 1.707614701 * sc,
  ];
}

/**
 * OKLCH to a `#rrggbb` string, clamped into the gamut.
 *
 * @param {number} l @param {number} c @param {number} h
 * @returns {string}
 */
export function oklchToHex(l, c, h) {
  return (
    "#" +
    oklchToLinear(l, c, h)
      .map((channel) => {
        const v = Math.round(Math.min(1, Math.max(0, toGamma(channel))) * 255);
        return v.toString(16).padStart(2, "0");
      })
      .join("")
  );
}

/**
 * OKLCH to `rgba(r, g, b, a)`, for the wash and edge tokens which are translucent.
 *
 * @param {number} l @param {number} c @param {number} h @param {number} alpha
 * @returns {string}
 */
function oklchToRgba(l, c, h, alpha) {
  const [r, g, b] = oklchToLinear(l, c, h).map((channel) =>
    Math.round(Math.min(1, Math.max(0, toGamma(channel))) * 255),
  );
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/**
 * WCAG relative luminance of an OKLCH colour.
 *
 * WCAG rather than anything newer because the ratio it feeds is the number every accessibility
 * guideline is written in, and a threshold nobody can look up is a threshold nobody checks.
 *
 * @param {number} l @param {number} c @param {number} h
 * @returns {number}
 */
function luminance(l, c, h) {
  const [r, g, b] = oklchToLinear(l, c, h).map((channel) =>
    toLinear(Math.min(1, Math.max(0, toGamma(channel)))),
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * The WCAG contrast ratio between two OKLCH colours, 1–21.
 *
 * @param {{l: number, c: number, h: number}} a
 * @param {{l: number, c: number, h: number}} b
 * @returns {number}
 */
export function contrast(a, b) {
  const one = luminance(a.l, a.c, a.h);
  const two = luminance(b.l, b.c, b.h);
  const light = Math.max(one, two);
  const dark = Math.min(one, two);
  return (light + 0.05) / (dark + 0.05);
}

// --- The stops ------------------------------------------------------------------------------

/**
 * A colour, as lightness and chroma plus a hue *offset* from its family.
 *
 * The offset is what lets one hue input move a whole family while keeping the small wanders the
 * originals had — `--text` sits 15° round from `--bg` and always did.
 *
 * @typedef {{l: number, c: number, dh?: number}} Stop
 */

/** The hue each family sits at by default, in degrees. */
export const DEFAULT_HUES = { surface: 300, accent: 297 };

/**
 * The four word-status hues, which the app owns and a learner does not choose.
 *
 * Locked from editing, **not frozen**: only the hue is fixed here, and each mode gives them its
 * own lightness and chroma below. A blue picked to read on near-black does not read on near-white,
 * so freezing the values outright would have been the same bug in the other direction.
 */
/** @type {Record<string, number>} */
const STATUS_HUES = { red: 13, yellow: 85, good: 160, blue: 252 };

/**
 * The five part-of-speech hues, in `WORD_KINDS` order.
 *
 * A long sense list is the problem these solve: 何 has eleven meanings running across pronoun,
 * noun, adverb, interjection and prefix, and drawn in one colour they are eleven lines a reader
 * has to read to tell apart. A rule in the kind's hue lets the shape be read before the words.
 *
 * **Spaced evenly at 72°, and deliberately not dodging the status hues.** Four hues are already
 * spoken for by word status and a fifth — the accent — is the learner's to choose, so there is no
 * arrangement of five more that clears all of them; trying produced a set crowded into the gaps
 * and barely distinguishable from *each other*, which is the job that actually matters here. What
 * makes the reuse safe is that the two never wear the same shape: a status is a filled pill with a
 * word in it, up in the header, and a kind is a hairline rule and a small label down in the senses.
 * The label carries the meaning in both places; the colour only makes it faster.
 */
export const KIND_HUES = {
  expression: 333,
  noun: 189,
  adjective: 261,
  adverb: 117,
  verb: 45,
  // The sixth, placed in the widest room left rather than by re-spacing the other five — moving
  // them would repaint every sense list to make space for one kind. 36° from noun and adjective
  // either side, which is the tightest pair here and still a cyan against a blue against an
  // indigo; `Function` mostly co-occurs with `Noun` and `Adverb`, and it is 36° and 108° from
  // those.
  function: 225,
};

/**
 * The whole kind wheel, turned.
 *
 * **The shape is the app's; where it sits is the palette's.** The six are read against each other
 * — a sense list is legible because adverb and noun are visibly different — so their *spacing* is
 * not something a theme gets to flatten. Their *position* is, and it has to be: at no rotation at
 * all, `Function` lands 11° from Ai's accent and `Verb` 5° from Kohi's, which on screen is the
 * same colour twice. Turning all six together fixes that without any of them converging.
 *
 * Measured, not chosen by eye: the spins on `PRESETS` are the ones that put every kind as far from
 * that palette's accent as the set allows, and `theme.test.mjs` re-measures them.
 *
 * @param {number} spin degrees
 * @returns {Record<string, number>}
 */
export function kindHues(spin = 0) {
  return Object.fromEntries(
    Object.entries(KIND_HUES).map(([name, hue]) => [name, wrapHue(hue + spin)]),
  );
}

/**
 * Degrees between two hues, the short way round.
 *
 * @param {number} a @param {number} b
 */
function apart(a, b) {
  const d = Math.abs(((a - b) % 360 + 360) % 360);
  return Math.min(d, 360 - d);
}

/**
 * How close the nearest kind comes to a hue, at a given rotation.
 *
 * The accent is what this is asked about. A kind sitting on it is the failure worth catching: both
 * are small saturated marks a few centimetres apart — a chip in a filter row and a label over a
 * sense — so at the same hue the reader has one colour answering to two different meanings.
 *
 * @param {number} spin degrees
 * @param {number} hue the hue to stay clear of
 */
export function kindClearance(spin, hue) {
  return Math.min(...Object.values(kindHues(spin)).map((kind) => apart(kind, hue)));
}

/**
 * The most room any rotation can buy against a hue, and the rotation that buys it.
 *
 * **36° is the ceiling and that is a property of the set, not of the search.** The wheel has two
 * 36° gaps of its own — `Function` sits between `Noun` and `Adjective` — so some kind is always
 * within 36° of any hue you name. Reaching it is therefore as good as this can be, and the search
 * below is a whole-degree sweep because the answer is wanted for one number, once, and a closed
 * form would be harder to check than to trust.
 *
 * This is the calculation the shipped palettes' spins came out of. It is exported so a learner's
 * own palette can have the same thing done for it rather than being left to find a good rotation
 * by dragging — which is how a kind ends up wearing the accent's colour.
 *
 * @param {number} hue usually the accent
 * @returns {{ spin: number, clearance: number }}
 */
export function bestKindSpin(hue) {
  let best = { spin: 0, clearance: kindClearance(0, hue) };
  for (let spin = 1; spin < 360; spin += 1) {
    const clearance = kindClearance(spin, hue);
    if (clearance > best.clearance) best = { spin, clearance };
  }
  return best;
}

/**
 * The clearance below which a kind is too near the hue it is measured against.
 *
 * The ceiling itself: anything less than the best the set can do is a rotation that gave something
 * away for nothing, since a better one always exists.
 */
export const KIND_CLEARANCE_FLOOR = 36;

/**
 * One mode's stops.
 *
 * @typedef {object} Palette
 * @property {Record<string, Stop>} surfaces
 * @property {Record<string, Stop>} text
 * @property {Record<string, Stop>} accent
 * @property {Record<string, Stop>} status
 * @property {Stop} kind
 * @property {string} shadow
 */

/**
 * Dark, measured from the palette this replaced.
 *
 * Every number here came out of the hex values that were in `theme.css`, so feeding the default
 * hues back through the derivation returns them unchanged. That is asserted, not hoped for.
 */
/** @type {Palette} */
const DARK = {
  surfaces: {
    "surface-sunken": { l: 0.114, c: 0.0175, dh: -7 },
    bg: { l: 0.143, c: 0.0067, dh: 1.1 },
    surface: { l: 0.178, c: 0.0126, dh: 0.4 },
    "surface-raised": { l: 0.213, c: 0.0199, dh: -1.6 },
    "surface-hover": { l: 0.243, c: 0.0272, dh: 0.8 },
    border: { l: 0.272, c: 0.0264, dh: 1 },
    "border-strong": { l: 0.336, c: 0.0341, dh: 1.4 },
  },
  text: {
    text: { l: 0.917, c: 0.0137, dh: 14.8 },
    "text-dim": { l: 0.707, c: 0.0243, dh: 5.5 },
    "text-faint": { l: 0.599, c: 0.0281, dh: 3.2 },
  },
  accent: {
    accent: { l: 0.685, c: 0.1479, dh: 1.4 },
    "accent-strong": { l: 0.597, c: 0.1806, dh: -1.2 },
  },
  status: {
    red: { l: 0.669, c: 0.1524 },
    "red-strong": { l: 0.598, c: 0.1588, dh: 0.6 },
    good: { l: 0.742, c: 0.1006, dh: -0.3 },
    blue: { l: 0.722, c: 0.1229, dh: 0.5 },
    yellow: { l: 0.79, c: 0.1386, dh: 0.2 },
  },
  /**
   * One stop for all five kinds.
   *
   * They play one role — a hairline and a small label — so they should differ in hue and in
   * nothing else. OKLCH is what makes that possible: equal lightness across hues *reads* equal,
   * which is the whole reason this file works in it rather than in HSL.
   */
  kind: { l: 0.76, c: 0.1 },
  /** 45% black is depth against a dark panel. */
  shadow: "0 0.5rem 1.5rem rgba(0, 0, 0, 0.45)",
};

/**
 * Light.
 *
 * **Not a mirror of dark, and it cannot be.** Two relationships invert rather than reflect:
 *
 * - *Borders.* On dark a rule is lighter than the surface it edges; on light it has to be darker,
 *   or it is invisible. So the border stops are not the far end of the surface ramp here — they
 *   are its own scale running the other way.
 * - *Hover.* `.control` lifts towards the light on dark. White is the ceiling, so on light a
 *   hovered control settles a step *down* instead. The metaphor flips; the token does not, which
 *   is the point of having tokens.
 *
 * Anchored on the light palette already in `core/src/anki/mod.rs` — a card cannot see `theme.css`,
 * so somebody had already had to choose one, and matching it keeps a card and the app that made it
 * looking related.
 */
/** @type {Palette} */
const LIGHT = {
  surfaces: {
    // A well you type into, below the page in both modes.
    "surface-sunken": { l: 0.938, c: 0.006, dh: 6 },
    bg: { l: 0.972, c: 0.004, dh: 8 },
    surface: { l: 0.988, c: 0.0028, dh: 8.4 },
    "surface-raised": { l: 1.0, c: 0.0 },
    // Down, not up: there is nothing above white to lift into.
    "surface-hover": { l: 0.948, c: 0.008, dh: 6 },
    border: { l: 0.877, c: 0.0146, dh: 8.3 },
    "border-strong": { l: 0.78, c: 0.02, dh: 7 },
  },
  text: {
    text: { l: 0.216, c: 0.0161, dh: 2.2 },
    "text-dim": { l: 0.442, c: 0.0238, dh: 4.8 },
    "text-faint": { l: 0.55, c: 0.0225, dh: 4 },
  },
  accent: {
    accent: { l: 0.506, c: 0.1649, dh: -2.9 },
    "accent-strong": { l: 0.44, c: 0.176, dh: -3 },
  },
  status: {
    // Darker and a touch stronger than their dark-mode counterparts: the same hue has to carry
    // against white, and 74% lightness green on white is a hint rather than a colour.
    red: { l: 0.53, c: 0.18 },
    "red-strong": { l: 0.47, c: 0.19, dh: 0.6 },
    good: { l: 0.52, c: 0.12, dh: -0.3 },
    blue: { l: 0.52, c: 0.17, dh: 0.5 },
    yellow: { l: 0.62, c: 0.14, dh: 0.2 },
  },
  /** Darker and stronger than dark mode's, for the reason the status stops above give. */
  kind: { l: 0.53, c: 0.13 },
  /** Far softer. 45% black under a light panel is a smudge, not a shadow. */
  shadow: "0 0.5rem 1.5rem rgba(24, 20, 30, 0.14)",
};

/** @type {Record<string, Palette>} */
const MODES = { dark: DARK, light: LIGHT };

/**
 * The contrast a label on a filled control has to carry.
 *
 * WCAG's threshold for text on a user-interface component, which is what the label on a primary
 * button is. Deliberately not 4.5 — that is the figure for body text, and holding a button label
 * to it would reject white on the app's own accent, which is what it has always shown.
 */
const UI_COMPONENT_FLOOR = 3;

/** How translucent a wash and an edge are. Both were already uniform; this names them. */
const ALPHA = {
  wash: 0.16,
  accentWash: 0.18,
  edge: 0.45,
  /** Blue and yellow carried a slightly firmer edge, and still do. */
  brightEdge: 0.5,
};

/**
 * The tokens that are one colour drawn three ways: a solid, a wash behind it, an edge around it.
 *
 * Named here because `deriveTokens` has to re-derive the other two whenever the solid is picked by
 * hand — see the loop at the end of it. The alphas match what the derivation uses for each, so a
 * chosen colour sits in the interface exactly the way a built-in one does.
 *
 * The four status families are the word states: blue is New, yellow is Seen, good is Known, red is
 * Ignored. Those names are the tokens' and the labels are the reader's, and `Settings.svelte` is
 * where the two are joined — a colour well saying "blue" for the New underline would be absurd the
 * moment somebody made it orange.
 */
const FAMILIES = [
  { base: "selection", wash: ALPHA.accentWash, edge: ALPHA.edge },
  { base: "red", wash: ALPHA.wash, edge: ALPHA.edge },
  { base: "good", wash: ALPHA.wash, edge: ALPHA.edge },
  { base: "blue", wash: ALPHA.wash, edge: ALPHA.brightEdge },
  { base: "yellow", wash: ALPHA.wash, edge: ALPHA.brightEdge },
  // The six kinds, each a solid and a wash — the wash is what a chosen filter chip fills with, so
  // a kind set by hand has to move both or the row you narrow by stops matching the senses you
  // read. No edge: nothing draws a kind as an outline. Built from the same table the derivation
  // uses, so a seventh kind cannot be added and quietly left un-re-derived here.
  ...Object.keys(KIND_HUES).map((name) => ({
    base: `kind-${name}`,
    wash: ALPHA.wash,
    edge: null,
  })),
];

// --- Derivation -----------------------------------------------------------------------------

/**
 * The palettes that ship with the app.
 *
 * **Code, not data** — these are the app's taste, they are a floor a learner can always return
 * to, and being code means they can be *corrected* in a later release where a copy in somebody's
 * settings file could not.
 *
 * A preset is only a mode and two hues, which is the whole reason there can be a list of them at
 * all: the same derivation runs behind every one, so none of them can be internally inconsistent
 * or fail a contrast floor on their own. `theme.test.mjs` checks each anyway.
 *
 * Sumi is the palette the app has always had, and stays first and default. The rest are the same
 * machinery pointed elsewhere.
 *
 * @type {ReadonlyArray<{name: string, about: string, theme: ThemeInput}>}
 */
// **Fuji is not here, and the reason is worth keeping.** It was `light, 320/300` beside Washi's
// `light, 300/297` — twenty degrees of surface hue and three of accent. On a near-white surface the
// chroma is so low that twenty degrees moves almost nothing: the two derived to a mean RGB distance
// of 2.2 across every token, with `surface-raised` identical at `#ffffff` and the largest gap
// anywhere being 6. The next-closest pair, Sumi and Ai, sits at 17.6. It was the same palette under
// a second name, and the test below now measures the output rather than the inputs so a third
// cannot be added the same way.
export const PRESETS = [
  {
    name: "Sumi",
    about: "Ink. The dark the app was built in.",
    // Sumi's accent already sits in the widest gap the kind set leaves, so it turns nothing.
    theme: { mode: "dark", surfaceHue: 300, accentHue: 297, kindSpin: 0 },
  },
  {
    name: "Washi",
    about: "Paper. Sumi's light counterpart, same hues.",
    theme: { mode: "light", surfaceHue: 300, accentHue: 297, kindSpin: 0 },
  },
  {
    name: "Ai",
    about: "Indigo, and cooler throughout.",
    // Untened, `Function` sits 11° from this accent — an indigo label on an indigo palette.
    theme: { mode: "dark", surfaceHue: 255, accentHue: 250, kindSpin: 97 },
  },
  {
    name: "Matcha",
    about: "A green cast, with a warmer accent to sit against it.",
    theme: { mode: "dark", surfaceHue: 150, accentHue: 165, kindSpin: 12 },
  },
  {
    name: "Kohi",
    about: "Warm paper and a coffee accent.",
    // The worst of the five untuned: `Verb` lands 5° off this accent, which is the same colour.
    theme: { mode: "light", surfaceHue: 60, accentHue: 40, kindSpin: 31 },
  },
];

/**
 * The palettes to choose between: the ones that ship, then the learner's own.
 *
 * **Out here so a test can see it.** This lived in `Settings.svelte` as a `$derived`, and it
 * quietly dropped `decor` when saved palettes grew one — so picking a palette applied its colours
 * and left the watermarks alone, which is exactly what it looks like when a feature was never
 * built. Nothing caught it: the read was written as `(palette as {{ decor?: ... }}).decor`, and a
 * cast that asserts a property is a cast that stops the compiler asking whether it is there.
 *
 * So the rule is: **a saved palette's fields reach the list unchanged**, and
 * `theme.test.mjs` checks that by comparing keys rather than by naming them, which is what makes
 * it hold for the next field somebody adds.
 *
 * The types come from `model.ts` through a JSDoc `import(...)`, which is erased at runtime — this
 * file stays importable by `node --test` with no build step, which is the whole reason it is `.js`.
 *
 * @param {typeof PRESETS} shipped
 * @param {import("../model").NamedTheme[]} mine
 * @returns {Array<{
 *   name: string,
 *   about: string | undefined,
 *   mine: boolean,
 *   theme: import("../model").ThemeSettings,
 *   decor: import("../model").ThemeDecor | null | undefined,
 * }>}
 */
export function palettesFor(shipped, mine) {
  return [
    ...shipped.map((preset) => ({
      name: preset.name,
      about: preset.about,
      mine: false,
      theme: {
        mode: preset.theme.mode,
        surface_hue: preset.theme.surfaceHue ?? 300,
        accent_hue: preset.theme.accentHue ?? 297,
        kind_spin: preset.theme.kindSpin ?? 0,
        // A shipped palette is the derivation with nothing bent: the dials at their identities
        // and no hand-set colours. Picking one is therefore also how you get back to plain.
        tint: 1,
        contrast: 1,
        depth: 0,
        overrides: {},
      },
      // A shipped palette is a colour statement and carries no decoration, so picking one leaves
      // the watermarks where they are. Present and undefined rather than absent, so both halves
      // of this list have the same shape.
      decor: undefined,
    })),
    ...mine.map((saved) => ({
      name: saved.name,
      // **No blurb, rather than a one-word one.** A shipped palette's `about` says what it is for —
      // which light, which mood. A palette somebody made has no such thing to say, and the word
      // standing here was "Yours.", which under a heading reading *Palette* said nothing the list
      // above had not already said by putting it under *Custom*. The note is absent instead.
      about: undefined,
      mine: true,
      theme: saved.theme,
      decor: saved.decor,
    })),
  ];
}

/**
 * Which palette in a list a theme *is*, or `null` for none of them.
 *
 * **Compared rather than remembered.** No pointer is stored alongside the theme, so nothing can go
 * stale: moving a slider drops to Custom on its own without anything having to notice, and a
 * palette cannot claim a theme it no longer describes.
 *
 * The four inputs are the four a palette actually sets. Hues are rounded because a slider yields
 * whole degrees while a stored theme may carry a fraction from an older build, and `kind_spin` is
 * in the list for a reason worth keeping: it was added later and left out of this comparison at
 * first, so turning the kind wheel left the list still naming Ai while showing colours Ai does not
 * have.
 *
 * Exported because two surfaces ask it — the settings page and the quick appearance panel — and a
 * second copy of a four-way comparison is how the two come to disagree about what "Custom" means.
 *
 * @param {ReadonlyArray<{ name: string, theme: import("../model").ThemeSettings }>} palettes
 * @param {import("../model").ThemeSettings} theme
 * @returns {string | null}
 */
export function matchPalette(palettes, theme) {
  const found = palettes.find(
    (palette) =>
      palette.theme.mode === theme.mode &&
      Math.round(palette.theme.surface_hue) === Math.round(theme.surface_hue) &&
      Math.round(palette.theme.accent_hue) === Math.round(theme.accent_hue) &&
      Math.round(palette.theme.kind_spin) === Math.round(theme.kind_spin),
  );
  return found?.name ?? null;
}

/**
 * A name for a saved palette that nothing else is already using.
 *
 * Compared without case, because "Night" and "night" being two rows in one list is a list nobody
 * trusts — and the shipped palettes count as taken, so a saved one can never shadow Sumi and leave
 * two entries answering to the same name.
 *
 * Numbers rather than a refusal: somebody typing a name that exists meant a name, and making them
 * think of another one is friction for a problem the app can solve.
 *
 * @param {string[]} taken every name already in use
 * @param {string} wanted what the learner typed
 * @returns {string} a free name, trimmed
 */
export function freePresetName(taken, wanted) {
  const claimed = new Set(taken.map((name) => name.trim().toLowerCase()));
  const base = wanted.trim() || "Palette";
  if (!claimed.has(base.toLowerCase())) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base} ${n}`;
    if (!claimed.has(candidate.toLowerCase())) return candidate;
  }
}

/**
 * @typedef {object} ThemeInput
 * @property {"light" | "dark" | "system"} mode
 * @property {number} [surfaceHue] degrees; the faint cast that keeps the greys with the accents
 * @property {number} [accentHue] degrees
 * @property {number} [tint] how strong that cast is; 1 is as designed, 0 is neutral grey
 * @property {number} [contrast] the gap between text and what it sits on; 1 is as designed
 * @property {number} [depth] shifts the whole surface ramp; positive is deeper, 0 is as designed
 * @property {number} [kindSpin] degrees; turns all six part-of-speech hues together
 * @property {Record<string, string>} [overrides] token name to `#rrggbb`, applied last
 */

/**
 * What the dials may be set to.
 *
 * Ranges rather than free numbers, and every default is the identity: at 1, 1, 0 the derivation is
 * exactly what it was before these existed, which is what lets them be added without moving
 * anybody's palette.
 */
export const LIMITS = {
  tint: { min: 0, max: 2.5, default: 1 },
  contrast: { min: 0.7, max: 1.6, default: 1 },
  depth: { min: -0.08, max: 0.08, default: 0 },
};

/**
 * The contrast a learner cannot go below, whatever they ask for.
 *
 * **The one pair that has to work.** Everything else is reported and left alone — it is their
 * palette. But text the same colour as the page behind it means the settings that would undo it
 * are themselves unreadable, and an app nobody can read their way out of is not a preference, it
 * is a trap. So this single relationship is clamped rather than merely warned about.
 */
const READABLE_FLOOR = 3;

/** What the softer floors are, for the readout rather than for enforcement. */
const WANTED = { text: 7, "text-dim": 4.5, "text-faint": 3 };

/** @param {number} value @param {{min: number, max: number}} range @returns {number} */
function hold(value, range) {
  if (!Number.isFinite(value)) return 0;
  return Math.min(range.max, Math.max(range.min, value));
}

/**
 * Bring an angle into 0–360 so a caller may hand over any number.
 *
 * @param {number} h @returns {number}
 */
function wrapHue(h) {
  return ((h % 360) + 360) % 360;
}

/**
 * Which mode a theme actually resolves to right now.
 *
 * `system` is only meaningful against something that knows the operating system's answer, so it
 * is passed in rather than read here — this file stays pure, and the caller already has to deal
 * with the fact that a webview's `prefers-color-scheme` is not to be trusted when the window
 * config pins a theme.
 *
 * @param {ThemeInput["mode"]} mode
 * @param {"light" | "dark"} osPrefers
 * @returns {"light" | "dark"}
 */
export function resolveMode(mode, osPrefers) {
  if (mode === "light" || mode === "dark") return mode;
  return osPrefers === "light" ? "light" : "dark";
}

/**
 * Every colour token, from a theme and a resolved mode.
 *
 * Returns CSS custom property names without the leading dashes, mapped to the value to write.
 * The caller adds the dashes — see `applyTheme`.
 *
 * @param {ThemeInput} theme
 * @param {"light" | "dark"} resolved
 * @returns {Record<string, string>}
 */
export function deriveTokens(theme, resolved) {
  const stops = MODES[resolved] ?? DARK;
  const surfaceHue = wrapHue(theme.surfaceHue ?? DEFAULT_HUES.surface);
  const accentHue = wrapHue(theme.accentHue ?? DEFAULT_HUES.accent);

  /** @type {Record<string, string>} */
  const out = {};
  /** @param {Stop} stop @param {number} base */
  const hex = (stop, base) => oklchToHex(stop.l, stop.c, wrapHue(base + (stop.dh ?? 0)));
  /** @param {Stop} stop @param {number} base @param {number} alpha */
  const rgba = (stop, base, alpha) =>
    oklchToRgba(stop.l, stop.c, wrapHue(base + (stop.dh ?? 0)), alpha);

  // --- The dials ----------------------------------------------------------------------------
  //
  // All three are identities at their defaults, so a theme that sets none of them derives exactly
  // what it did before they existed. They move the *greys* only: the accent is the learner's other
  // dial and the four status hues are the app's, so neither is something a "how strong is the
  // tint" control has any business desaturating.

  const tint = hold(theme.tint ?? LIMITS.tint.default, LIMITS.tint);
  // Not `contrast` — that is the function this file measures with, and shadowing it here broke
  // every derivation at once. The tests caught it immediately, which is the argument for having
  // them at this level rather than only on screen.
  const spread = hold(theme.contrast ?? LIMITS.contrast.default, LIMITS.contrast);
  const depth = hold(theme.depth ?? LIMITS.depth.default, LIMITS.depth);

  // **Deeper means more ink, in both modes.** The first attempt moved each palette away from its
  // own end of the scale, which is a tidy sentence and the wrong behaviour: turning it up made a
  // light theme *lighter*, so the dial reversed under the learner's hand when they switched mode.
  // Darker is darker either way — blacker on dark, greyer on light.
  const deeper = -depth;

  /** @param {Stop} stop @returns {Stop} */
  const surfaceStop = (stop) => ({
    l: Math.min(1, Math.max(0, stop.l + deeper)),
    c: stop.c * tint,
    dh: stop.dh,
  });

  for (const [name, stop] of Object.entries(stops.surfaces)) {
    out[name] = hex(surfaceStop(stop), surfaceHue);
  }

  // Text is pushed away from the page rather than set outright, so `contrast` widens a gap that
  // already reads correctly instead of inventing a new relationship.
  const page = surfaceStop(stops.surfaces.bg);
  /** @param {Stop} stop @returns {Stop} */
  const textStop = (stop) => ({
    l: Math.min(1, Math.max(0, page.l + (stop.l - stops.surfaces.bg.l) * spread)),
    c: stop.c * tint,
    dh: stop.dh,
  });

  for (const [name, stop] of Object.entries(stops.text)) out[name] = hex(textStop(stop), surfaceHue);

  // Accent: solid, fill, and the wash-and-edge pair that means "this one is on".
  out.accent = hex(stops.accent.accent, accentHue);
  out["accent-strong"] = hex(stops.accent["accent-strong"], accentHue);
  out["accent-wash"] = rgba(stops.accent["accent-strong"], accentHue, ALPHA.accentWash);
  out["accent-edge"] = rgba(stops.accent.accent, accentHue, ALPHA.edge);

  // **Selection is the accent until somebody says otherwise.**
  //
  // What a word wears while it is the one you are on. It was the accent directly, which is the
  // right default -- the thing you are looking at should be the app's own colour -- but it made
  // "I want the accent purple and the selection green" unsayable. Its own family costs three
  // tokens and makes that a colour anyone can pick; overriding `selection` detaches it, and
  // leaving it alone is byte-identical to what the reader had before.
  out.selection = out.accent;
  out["selection-wash"] = out["accent-wash"];
  out["selection-edge"] = out["accent-edge"];

  // Status. Hues are the app's, so these take no input at all.
  /** @type {Array<{name: string, strong: string | null, edge: number}>} */
  const status = [
    { name: "red", strong: "red-strong", edge: ALPHA.edge },
    { name: "good", strong: null, edge: ALPHA.edge },
    { name: "blue", strong: null, edge: ALPHA.brightEdge },
    { name: "yellow", strong: null, edge: ALPHA.brightEdge },
  ];
  for (const { name, strong, edge: edgeAlpha } of status) {
    const hue = STATUS_HUES[name];
    out[name] = hex(stops.status[name], hue);
    if (strong) out[strong] = hex(stops.status[strong], hue);
    // A wash takes the deeper of the pair where there is one, which is what the originals did.
    out[`${name}-wash`] = rgba(stops.status[strong ?? name], hue, ALPHA.wash);
    out[`${name}-edge`] = rgba(stops.status[name], hue, edgeAlpha);
  }

  // Parts of speech. One stop, five hues, and no input either — what a word *is* is not a
  // preference. The wash is what a chosen filter chip fills with, so the row you narrow by and the
  // senses you read are the same colour.
  for (const [name, hue] of Object.entries(kindHues(theme.kindSpin ?? 0))) {
    out[`kind-${name}`] = hex(stops.kind, hue);
    out[`kind-${name}-wash`] = rgba(stops.kind, hue, ALPHA.wash);
  }

  // **Measured, never guessed.** `tokens.md` used to say "never guess this" of the two below,
  // because the right answer flips between white and near-black depending on how light the fill
  // is — a judgement that was made once, by hand, for one purple. Asking which of the two
  // contrasts better answers it for every hue a learner might choose.
  const paper = { l: 1, c: 0, h: 0 };
  const ink = {
    l: stops.surfaces["surface-sunken"].l,
    c: stops.surfaces["surface-sunken"].c,
    h: wrapHue(surfaceHue + (stops.surfaces["surface-sunken"].dh ?? 0)),
  };
  /**
   * **Paper wins ties, and near-ties.** Taking whichever number is larger is the obvious rule and
   * it is wrong here: white on the default purple measures 4.31:1 and near-black measures 4.72:1,
   * so maximising would have flipped every primary button's label from white to almost black —
   * a loud change to the app's look, bought for four hundredths of a ratio.
   *
   * So paper is preferred while it clears the threshold WCAG sets for text on a **user-interface
   * component**, which is what a filled button's label is. Ink is taken only where paper genuinely
   * fails, which is what happens on a light or yellow accent — and is exactly the case the
   * hand-picked pair could not have covered.
   *
   * That this reproduces both of the values chosen by hand, for the hue they were chosen at, is
   * the argument that the rule is the one somebody was already applying by eye.
   *
   * @param {Stop} fill @param {number} base
   */
  const on = (fill, base) => {
    const against = { l: fill.l, c: fill.c, h: wrapHue(base + (fill.dh ?? 0)) };
    const onPaper = contrast(paper, against);
    if (onPaper >= UI_COMPONENT_FLOOR) return "#ffffff";
    return onPaper >= contrast(ink, against) ? "#ffffff" : oklchToHex(ink.l, ink.c, ink.h);
  };
  out["on-accent-strong"] = on(stops.accent["accent-strong"], accentHue);
  out["on-accent"] = on(stops.accent.accent, accentHue);

  out["shadow-popup"] = stops.shadow;

  // **Overrides last, and taken at their word.** A learner who typed a colour meant it, so the
  // dials do not get to argue with it — see `themeWarnings` for how a poor choice is reported
  // rather than corrected.
  for (const [name, value] of Object.entries(theme.overrides ?? {})) {
    if (typeof value === "string" && /^#[0-9a-fA-F]{6}$/.test(value)) {
      out[name] = value.toLowerCase();
    }
  }

  // **A family moves together, or it is not a family.**
  //
  // The override loop above is a flat token-to-hex replacement, which is right for the greys: set
  // `--border` and only the border changes. It is wrong for anything drawn as a solid *and* a
  // wash *and* an edge. Picking a new colour for Seen would have recoloured the underline in the
  // reader and left the Seen button filled with the old yellow, because `yellow-wash` is derived
  // from the hue and never saw the override.
  //
  // So for the families that are one idea in three tokens, the wash and the edge are re-derived
  // from whatever was picked -- unless those were picked by hand too, in which case the hand wins.
  // The alphas are the same ones the derivation uses, so a chosen colour behaves like a built-in.
  for (const { base, wash, edge } of FAMILIES) {
    const picked = (theme.overrides ?? {})[base];
    if (typeof picked !== "string" || !/^#[0-9a-fA-F]{6}$/.test(picked)) continue;
    const { l, c, h } = hexToOklch(picked);
    if (!(theme.overrides ?? {})[`${base}-wash`]) {
      out[`${base}-wash`] = oklchToRgba(l, c, h, wash);
    }
    if (edge !== null && !(theme.overrides ?? {})[`${base}-edge`]) {
      out[`${base}-edge`] = oklchToRgba(l, c, h, edge);
    }
  }

  // **One relationship is clamped, and only one.** Text the same colour as the page means the
  // settings that would undo it cannot be read either. Everything else is theirs to get wrong.
  out.text = readableAgainst(out.text, out.bg);
  return out;
}

/**
 * Move a colour away from its background until it can be read, and no further.
 *
 * Lightness only: the hue and chroma are what the learner chose, and shifting those to win a ratio
 * would be answering a question nobody asked. It walks in the direction that is already further
 * from the background, so text meant to be pale stays pale.
 *
 * @param {string} colour `#rrggbb`
 * @param {string} against `#rrggbb`
 * @returns {string}
 */
function readableAgainst(colour, against) {
  const ink = hexToOklch(colour);
  const page = hexToOklch(against);
  if (contrast(ink, page) >= READABLE_FLOOR) return colour;

  const away = ink.l >= page.l ? 1 : -1;
  for (let step = 1; step <= 100; step += 1) {
    const l = Math.min(1, Math.max(0, ink.l + away * step * 0.01));
    const moved = { l, c: ink.c, h: ink.h };
    if (contrast(moved, page) >= READABLE_FLOOR) return oklchToHex(l, moved.c, moved.h);
    if (l === 0 || l === 1) break;
  }
  // The far end of the scale did not do it, which takes a background near mid-grey. Black or
  // white, whichever is further: at this point the only job left is legibility.
  return contrast({ l: 1, c: 0, h: 0 }, page) >= contrast({ l: 0, c: 0, h: 0 }, page)
    ? "#ffffff"
    : "#000000";
}

/**
 * What is wrong with a palette, in the learner's terms.
 *
 * **Reported, not enforced.** Only `--text` against `--bg` is clamped, because that is the pair
 * whose failure hides the controls that would fix it. Everything else is a choice somebody made,
 * and a settings page that quietly corrects what you typed is worse than one that tells you.
 *
 * @param {Record<string, string>} tokens
 * @returns {Array<{token: string, ratio: number, wanted: number}>}
 */
export function themeWarnings(tokens) {
  const out = [];
  for (const [token, wanted] of Object.entries(WANTED)) {
    const ratio = contrast(hexToOklch(tokens[token]), hexToOklch(tokens.bg));
    if (ratio < wanted) out.push({ token, ratio, wanted });
  }
  const label = contrast(hexToOklch(tokens["on-accent-strong"]), hexToOklch(tokens["accent-strong"]));
  if (label < UI_COMPONENT_FLOOR) {
    out.push({ token: "on-accent-strong", ratio: label, wanted: UI_COMPONENT_FLOOR });
  }
  return out;
}

/**
 * `#rrggbb` back to OKLCH, so a colour somebody typed can be measured like a derived one.
 *
 * @param {string} hex
 * @returns {{l: number, c: number, h: number}}
 */
export function hexToOklch(hex) {
  const [r, g, b] = [1, 3, 5].map((at) => toLinear(parseInt(hex.slice(at, at + 2), 16) / 255));
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);

  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;

  return { l: L, c: Math.hypot(a, bb), h: wrapHue((Math.atan2(bb, a) * 180) / Math.PI) };
}

/**
 * Write the derived tokens onto an element, usually `document.documentElement`.
 *
 * Separate from the derivation so the maths can be tested without a DOM, and so the capture
 * frame — which is a second document with no access to the main window's — can call the same
 * thing with its own root.
 *
 * @param {HTMLElement} root
 * @param {ThemeInput} theme
 * @param {"light" | "dark"} resolved
 */
export function applyTheme(root, theme, resolved) {
  const tokens = deriveTokens(theme, resolved);
  for (const [name, value] of Object.entries(tokens)) {
    root.style.setProperty(`--${name}`, value);
  }
  // So a stylesheet can answer the mode directly — and so anything reading the DOM can tell what
  // it resolved to, which `system` otherwise hides.
  root.dataset.theme = resolved;
}
