import assert from "node:assert/strict";
import test from "node:test";

import {
  KIND_HUES,
  LIMITS,
  PRESETS,
  deriveTokens,
  freePresetName,
  KIND_CLEARANCE_FLOOR,
  bestKindSpin,
  kindClearance,
  kindHues,
  matchPalette,
  palettesFor,
  resolveMode,
  themeWarnings,
} from "./theme.js";

const dark = (over = {}) => deriveTokens({ mode: "dark", ...over }, "dark");
const light = (over = {}) => deriveTokens({ mode: "light", ...over }, "light");

/**
 * The palette that was written out in `theme.css` before any of this existed.
 *
 * This is the whole promise of the first slice: turning the colours into something derived must
 * not change what anybody sees. If a stop is retuned deliberately, this list is what says so out
 * loud rather than a screenshot somebody happens to compare.
 */
const WAS = {
  "surface-sunken": "#05040a",
  bg: "#0a090c",
  surface: "#121016",
  "surface-raised": "#1a1721",
  "surface-hover": "#221d2b",
  border: "#292432",
  "border-strong": "#3a3346",
  text: "#e7e1ea",
  "text-dim": "#a49dad",
  "text-faint": "#837c8e",
  accent: "#a683e8",
  "accent-strong": "#8b62dd",
  red: "#e2687a",
  "red-strong": "#cc4f63",
  good: "#6fbf95",
  blue: "#6aa9f0",
  yellow: "#e3b341",
};

test("the dark theme is the palette the app already had", () => {
  const tokens = dark();
  for (const [name, expected] of Object.entries(WAS)) {
    // One unit of tolerance per channel: the stops are three decimal places of OKLCH, and the
    // last bit of an 8-bit channel is below anything an eye or a screenshot will report.
    const got = tokens[name];
    const off = [1, 3, 5].map((at) =>
      Math.abs(parseInt(got.slice(at, at + 2), 16) - parseInt(expected.slice(at, at + 2), 16)),
    );
    assert.ok(
      off.every((d) => d <= 1),
      `--${name} should still be ${expected}, got ${got}`,
    );
  }
});

test("the translucent tokens keep the alphas they had", () => {
  const tokens = dark();
  assert.match(tokens["accent-wash"], /, 0\.18\)$/);
  assert.match(tokens["accent-edge"], /, 0\.45\)$/);
  assert.match(tokens["red-wash"], /, 0\.16\)$/);
  // Blue and yellow carried a firmer edge than the rest, and the derivation keeps that rather
  // than tidying it into one number.
  assert.match(tokens["blue-edge"], /, 0\.5\)$/);
  assert.match(tokens["yellow-edge"], /, 0\.5\)$/);
  assert.match(tokens["good-edge"], /, 0\.45\)$/);
});

test("a wash takes the deeper half of a pair, as it always did", () => {
  const tokens = dark();
  // `--accent-wash` was built from `--accent-strong`, not from `--accent`. Reading it off the
  // lighter one would lift every "this is on" surface by a step.
  assert.ok(tokens["accent-wash"].startsWith("rgba(139, 98, 221"));
  assert.ok(tokens["red-wash"].startsWith("rgba(204, 79, 99"));
});

test("every surface is ordered the same way whatever the hue", () => {
  // The claim the whole thing rests on: *a thing is lighter because it is nearer*. In HSL this
  // silently breaks as the hue turns, which is the reason for the conversion in `theme.js`.
  const rising = ["surface-sunken", "bg", "surface", "surface-raised", "surface-hover"];
  for (let hue = 0; hue < 360; hue += 15) {
    const tokens = dark({ surfaceHue: hue });
    const lightnesses = rising.map((name) => {
      const hex = tokens[name];
      return [1, 3, 5].reduce((sum, at) => sum + parseInt(hex.slice(at, at + 2), 16), 0);
    });
    for (let i = 1; i < lightnesses.length; i += 1) {
      assert.ok(
        lightnesses[i] > lightnesses[i - 1],
        `at hue ${hue}, ${rising[i]} is not lighter than ${rising[i - 1]}`,
      );
    }
  }
});

test("light puts its rules darker than its surfaces, not lighter", () => {
  const tokens = light();
  const sum = (hex) => [1, 3, 5].reduce((t, at) => t + parseInt(hex.slice(at, at + 2), 16), 0);
  // The relationship that inverts rather than mirrors. A border lighter than the card it edges is
  // a border nobody can see, which is what a mirrored dark ramp would have produced.
  assert.ok(sum(tokens.border) < sum(tokens.surface), "a light border must be darker than surface");
  assert.ok(sum(tokens["border-strong"]) < sum(tokens.border));
  assert.ok(sum(tokens.text) < sum(tokens["text-dim"]), "text is the darkest of the three");
});

test("text carries its weight against the background in both modes", () => {
  for (const [name, tokens] of [
    ["dark", dark()],
    ["light", light()],
  ]) {
    const ratio = contrastOf(tokens.text, tokens.bg);
    assert.ok(ratio >= 7, `${name}: --text on --bg is ${ratio.toFixed(1)}:1, wanted 7 or better`);
    const dim = contrastOf(tokens["text-dim"], tokens.bg);
    assert.ok(dim >= 4.5, `${name}: --text-dim on --bg is ${dim.toFixed(1)}:1, wanted 4.5`);
  }
});

test("what sits on the accent fill is chosen by contrast, not by taste", () => {
  // `tokens.md` used to say "never guess this" of these two, because the answer flips between
  // white and near-black depending on the fill — a judgement made once, by hand, for one purple.
  //
  // Three, not 4.5: WCAG's figure for text on a user-interface component, which is what a filled
  // button's label is. Holding it to the body-text figure would reject white on the app's own
  // accent, which measures 4.31:1 and is what it has always shown.
  for (let hue = 0; hue < 360; hue += 20) {
    for (const tokens of [dark({ accentHue: hue }), light({ accentHue: hue })]) {
      const onStrong = contrastOf(tokens["on-accent-strong"], tokens["accent-strong"]);
      const onAccent = contrastOf(tokens["on-accent"], tokens.accent);
      assert.ok(onStrong >= 3, `hue ${hue}: text on the fill is only ${onStrong.toFixed(1)}:1`);
      assert.ok(onAccent >= 3, `hue ${hue}: text on the hover fill is only ${onAccent.toFixed(1)}:1`);
    }
  }
});

test("the pair the palette shipped with is the pair that comes back", () => {
  // The regression the live check caught: maximising contrast flipped every primary button's
  // label from white to near-black, for four hundredths of a ratio. Both hand-picked values are
  // reproduced now, which is the evidence that preferring paper is the rule somebody was already
  // applying by eye.
  const tokens = dark();
  assert.equal(tokens["on-accent-strong"], "#ffffff", "the button label went dark");
  assert.equal(tokens["on-accent"], "#05040a", "the hover label should stay near-black");
});

test("the status hues are the app's, and no input moves them", () => {
  const plain = dark();
  const turned = dark({ accentHue: 40, surfaceHue: 120 });
  for (const name of ["red", "good", "blue", "yellow"]) {
    assert.equal(turned[name], plain[name], `--${name} followed an input it should not see`);
  }
  // But they do change between modes, which is the half that "locked" must not be read as.
  assert.notEqual(light().blue, plain.blue, "the light blue must not be the dark one");
});

test("the shadow is not the same under a light panel", () => {
  // 45% black is depth against dark and a smudge against white, so it is a token and it changes.
  assert.notEqual(light()["shadow-popup"], dark()["shadow-popup"]);
});

test("system resolves to whatever the machine says, and anything else ignores it", () => {
  assert.equal(resolveMode("system", "light"), "light");
  assert.equal(resolveMode("system", "dark"), "dark");
  assert.equal(resolveMode("light", "dark"), "light");
  assert.equal(resolveMode("dark", "light"), "dark");
});

test("a hue outside the circle is wrapped rather than thrown out", () => {
  assert.equal(dark({ accentHue: 297 + 720 }).accent, dark({ accentHue: 297 }).accent);
  assert.equal(dark({ accentHue: -63 }).accent, dark({ accentHue: 297 }).accent);
});

test("every token the stylesheet expects is produced", () => {
  const tokens = dark();
  const wanted = [
    ...Object.keys(WAS),
    "accent-wash",
    "accent-edge",
    "red-wash",
    "red-edge",
    "good-wash",
    "good-edge",
    "blue-wash",
    "blue-edge",
    "yellow-wash",
    "yellow-edge",
    // Selection is its own family so it can be detached from the accent; left alone it holds
    // the accent's values, which is what the reader has always drawn.
    "selection",
    "selection-wash",
    "selection-edge",
    "on-accent",
    "on-accent-strong",
    "shadow-popup",
    // One colour and one wash for each part of speech, in `WORD_KINDS` order. Listed rather than
    // generated so adding a kind has to be a decision here too — the sense colours, the filter
    // chips and this list are three places that must name the same five things.
    "kind-expression",
    "kind-expression-wash",
    "kind-noun",
    "kind-noun-wash",
    "kind-adjective",
    "kind-adjective-wash",
    "kind-adverb",
    "kind-adverb-wash",
    "kind-verb",
    "kind-verb-wash",
    "kind-function",
    "kind-function-wash",
  ];
  for (const name of wanted) {
    assert.ok(tokens[name], `--${name} is missing from the derivation`);
  }
  assert.equal(Object.keys(tokens).length, wanted.length, "an unexpected token appeared");
});

test("every preset that ships is legible", () => {
  // The point of deriving rather than hand-picking: a palette cannot be added that quietly fails
  // a contrast floor, because none of them are hand-picked. This is what makes the list safe to
  // grow.
  for (const preset of PRESETS) {
    const resolved = preset.theme.mode === "light" ? "light" : "dark";
    const tokens = deriveTokens(preset.theme, resolved);

    const body = contrastOf(tokens.text, tokens.bg);
    assert.ok(body >= 7, `${preset.name}: --text on --bg is only ${body.toFixed(1)}:1`);

    const dim = contrastOf(tokens["text-dim"], tokens.bg);
    assert.ok(dim >= 4.5, `${preset.name}: --text-dim on --bg is only ${dim.toFixed(1)}:1`);

    const onFill = contrastOf(tokens["on-accent-strong"], tokens["accent-strong"]);
    assert.ok(onFill >= 3, `${preset.name}: a button label is only ${onFill.toFixed(1)}:1`);

    // A rule has to be visible against the surface it edges, in either direction.
    const edge = contrastOf(tokens.border, tokens.surface);
    assert.ok(edge >= 1.2, `${preset.name}: --border barely differs from --surface`);
  }
});

test("Sumi is the palette the app already had, and leads the list", () => {
  assert.equal(PRESETS[0].name, "Sumi");
  const sumi = deriveTokens(PRESETS[0].theme, "dark");
  for (const [name, expected] of Object.entries(WAS)) {
    assert.equal(sumi[name], dark()[name], `--${name} drifted from the default theme`);
    assert.ok(expected, name);
  }
});

/**
 * **Measured on what they paint, not on what they are set to.**
 *
 * This compared `mode:surfaceHue:accentHue` and passed for months while Washi and Fuji were, on
 * screen, one palette under two names: `light 300/297` against `light 320/300` is twenty degrees
 * of surface hue, and at the chroma a near-white surface carries, twenty degrees moves nothing.
 * They derived to a mean RGB distance of 2.2 with `surface-raised` identical at `#ffffff`.
 * Distinct inputs are not distinct palettes, and only the inputs were being checked.
 *
 * The floor is 10. The measured pairs after Fuji went are 17.6 (Sumi/Ai), 23.4 (Ai/Matcha), 26.7
 * (Washi/Kohi) and up — so 10 is comfortably clear of every real pair and an order of magnitude
 * above the one that was wrong.
 */
test("no two presets paint the same palette", () => {
  const channels = (hex) => [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16));
  const apart = (a, b) =>
    Math.sqrt(channels(a).reduce((sum, c, i) => sum + (c - channels(b)[i]) ** 2, 0));

  const painted = PRESETS.map((preset) => ({
    name: preset.name,
    tokens: deriveTokens(preset.theme, preset.theme.mode),
  }));

  for (let i = 0; i < painted.length; i += 1) {
    for (let j = i + 1; j < painted.length; j += 1) {
      const shared = Object.keys(painted[i].tokens).filter(
        (key) =>
          typeof painted[i].tokens[key] === "string" &&
          painted[i].tokens[key].startsWith("#") &&
          String(painted[j].tokens[key] ?? "").startsWith("#"),
      );
      const mean =
        shared.reduce((sum, key) => sum + apart(painted[i].tokens[key], painted[j].tokens[key]), 0) /
        shared.length;
      assert.ok(
        mean >= 10,
        `${painted[i].name} and ${painted[j].name} are the same palette — mean RGB distance ${mean.toFixed(1)} across ${shared.length} tokens`,
      );
    }
  }
});

test("Fuji is gone, and a learner still on it keeps their colours", () => {
  // Removed as a duplicate of Washi. A theme is stored as *inputs*, never as a preset name, so
  // somebody sitting on `light 320/300` is unaffected: the palette list simply shows Custom, and
  // every colour on screen stays exactly where it was.
  assert.equal(
    PRESETS.some((preset) => preset.name === "Fuji"),
    false,
  );
  const theirs = deriveTokens({ mode: "light", surfaceHue: 320, accentHue: 300 }, "light");
  assert.match(theirs.bg, /^#[0-9a-f]{6}$/, "their palette still derives");
});

test("the dials do nothing at their defaults", () => {
  // The property that let them be added at all: a palette that sets none of them derives exactly
  // what it did before they existed.
  const plain = dark();
  const explicit = dark({ tint: 1, contrast: 1, depth: 0 });
  assert.deepEqual(explicit, plain);
});

test("tint moves the greys and leaves the colours alone", () => {
  const flat = dark({ tint: 0 });
  const plain = dark();

  // At zero the surfaces are neutral: no cast at all.
  const [r, g, b] = [1, 3, 5].map((at) => parseInt(flat.surface.slice(at, at + 2), 16));
  assert.ok(Math.max(r, g, b) - Math.min(r, g, b) <= 1, `--surface is still tinted: ${flat.surface}`);

  // But the accent is the learner's other dial and the status hues are the app's, so a control
  // named "how strong is the tint" has no business desaturating either.
  assert.equal(flat.accent, plain.accent);
  assert.equal(flat.blue, plain.blue);
  assert.equal(flat.yellow, plain.yellow);
});

test("contrast widens the gap it was already reading correctly", () => {
  const ratio = (tokens) => contrastOf(tokens.text, tokens.bg);
  assert.ok(ratio(dark({ contrast: 1.4 })) > ratio(dark()));
  assert.ok(ratio(dark({ contrast: 0.75 })) < ratio(dark()));
  // And the direction holds in light, where text is the darker of the two.
  assert.ok(ratio(light({ contrast: 1.4 })) > ratio(light()));
});

test("depth reads the same way round in both modes", () => {
  const sum = (hex) => [1, 3, 5].reduce((t, at) => t + parseInt(hex.slice(at, at + 2), 16), 0);
  // Deeper means away from the page's own end of the scale, so one dial does not invert under
  // the learner's hand when they switch mode.
  assert.ok(sum(dark({ depth: 0.06 }).bg) < sum(dark().bg), "a deeper dark should be darker");
  assert.ok(sum(light({ depth: 0.06 }).bg) < sum(light().bg), "a deeper light should be greyer");
});

test("a dial set past its limit is held, not obeyed", () => {
  assert.deepEqual(dark({ tint: 99 }), dark({ tint: LIMITS.tint.max }));
  assert.deepEqual(dark({ contrast: -5 }), dark({ contrast: LIMITS.contrast.min }));
  assert.deepEqual(dark({ depth: Number.NaN }), dark({ depth: 0 }));
});

test("text stays readable however the dials are set", () => {
  // The one relationship that is clamped rather than reported. Everything else is theirs to get
  // wrong; this pair failing hides the controls that would undo it.
  for (const contrast of [0.7, 1, 1.6]) {
    for (const depth of [-0.08, 0, 0.08]) {
      for (const tint of [0, 1, 2.5]) {
        for (const mode of ["dark", "light"]) {
          const tokens = deriveTokens({ mode, contrast, depth, tint }, mode);
          const ratio = contrastOf(tokens.text, tokens.bg);
          assert.ok(ratio >= 3, `${mode} ${contrast}/${depth}/${tint}: ${ratio.toFixed(1)}:1`);
        }
      }
    }
  }
});

test("an override is taken at its word", () => {
  const tokens = dark({ overrides: { accent: "#ABCDEF", "surface-raised": "#123456" } });
  // Lower-cased, but otherwise exactly what was asked for — the dials do not get to argue.
  assert.equal(tokens.accent, "#abcdef");
  assert.equal(tokens["surface-raised"], "#123456");
});

test("a malformed override is ignored rather than painted", () => {
  const plain = dark();
  const tokens = dark({
    overrides: { accent: "not a colour", bg: "#abc", surface: "", border: "#12345g" },
  });
  for (const name of ["accent", "bg", "surface", "border"]) {
    assert.equal(tokens[name], plain[name], `--${name} took a value that is not a colour`);
  }
});

test("even an override cannot make the app unreadable", () => {
  // Somebody can set text to the page colour; they cannot be left unable to read the settings
  // that would undo it. This is the single clamp, and it moves lightness only.
  const tokens = dark({ overrides: { text: "#0a090c" } });
  assert.notEqual(tokens.text, "#0a090c");
  assert.ok(contrastOf(tokens.text, tokens.bg) >= 3);
});

test("warnings report what is not clamped", () => {
  const tokens = dark({ overrides: { "text-dim": "#111014" } });
  const warned = themeWarnings(tokens);
  const dim = warned.find((entry) => entry.token === "text-dim");
  assert.ok(dim, `--text-dim should have been reported: ${JSON.stringify(warned)}`);
  assert.ok(dim.ratio < dim.wanted);

  // And a palette nobody has broken says nothing at all.
  assert.deepEqual(themeWarnings(dark()), []);
  assert.deepEqual(themeWarnings(light()), []);
});

test("a saved palette cannot take a name something else answers to", () => {
  assert.equal(freePresetName([], "Night"), "Night");
  assert.equal(freePresetName(["Night"], "Night"), "Night 2");
  assert.equal(freePresetName(["Night", "Night 2"], "Night"), "Night 3");

  // Case-insensitively, because two rows reading "Night" and "night" is a list nobody trusts.
  assert.equal(freePresetName(["night"], "Night"), "Night 2");

  // And the shipped palettes are taken, so a saved one can never shadow Sumi.
  assert.equal(freePresetName(PRESETS.map((p) => p.name), "Sumi"), "Sumi 2");

  // Whitespace is not a name.
  assert.equal(freePresetName([], "   "), "Palette");
  assert.equal(freePresetName([], "  Dusk  "), "Dusk");
});

/**
 * Contrast between two `#rrggbb` strings.
 *
 * **Measured from the hex, not by approximating it back to a grey.** The first version of this
 * helper searched for the nearest neutral and read its lightness, which is fine for the near-grey
 * tokens and badly wrong for a saturated fill — it reported a red button at 2.7:1 that is really
 * 4.9:1, and failed the derivation for a fault in the test.
 */
function contrastOf(a, b) {
  const light = Math.max(relativeLuminance(a), relativeLuminance(b));
  const dark = Math.min(relativeLuminance(a), relativeLuminance(b));
  return (light + 0.05) / (dark + 0.05);
}

/** WCAG relative luminance, straight from the channels. */
function relativeLuminance(hex) {
  const [r, g, b] = [1, 3, 5].map((at) => {
    const channel = parseInt(hex.slice(at, at + 2), 16) / 255;
    return channel <= 0.04045 ? channel / 12.92 : Math.pow((channel + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/**
 * **Picking a status colour moves the whole family, or the interface contradicts itself.**
 *
 * Seen is drawn three ways: the underline in the reader is `--yellow`, the Seen button's fill is
 * `--yellow-wash`, its border is `--yellow-edge`. Overrides are otherwise a flat token-to-hex
 * replacement, which would have recoloured the underline and left the button filled with the old
 * yellow — the two saying different things about the same word.
 */
test("a chosen status colour takes its wash and edge with it", () => {
  const picked = "#ff8800";
  const tokens = deriveTokens({ mode: "dark", overrides: { yellow: picked } }, "dark");
  const plain = deriveTokens({ mode: "dark" }, "dark");

  assert.equal(tokens.yellow, picked, "the solid is what was picked");
  assert.notEqual(tokens["yellow-wash"], plain["yellow-wash"], "the wash followed");
  assert.notEqual(tokens["yellow-edge"], plain["yellow-edge"], "the edge followed");

  // And it followed *to the right place*: the wash is the chosen colour at the wash's alpha.
  const channels = (value) => (value.match(/[\d.]+/g) ?? []).map(Number);
  const [r, g, b, alpha] = channels(tokens["yellow-wash"]);
  assert.equal(alpha, 0.16, "the wash keeps the alpha the derivation uses");
  assert.ok(r > g && g > b, `the wash is the orange that was picked, not the old yellow`);
  assert.equal(channels(tokens["yellow-edge"])[3], 0.5, "yellow's edge is the firmer one");

  // The families that were not touched are untouched.
  for (const name of ["blue", "blue-wash", "blue-edge", "good-wash", "red-edge"]) {
    assert.equal(tokens[name], plain[name], `--${name} moved and should not have`);
  }
});

test("a hand-set wash outranks the one a chosen solid would derive", () => {
  // Both were picked, so neither is a default to be helpfully replaced.
  const tokens = deriveTokens(
    { mode: "dark", overrides: { blue: "#ff0000", "blue-wash": "#00ff00" } },
    "dark",
  );
  assert.equal(tokens["blue-wash"], "#00ff00", "the hand-set wash survived");
  assert.match(tokens["blue-edge"], /^rgba\(/, "and the edge still came from the solid");
});

test("selection is the accent until it is detached", () => {
  const joined = deriveTokens({ mode: "dark", accentHue: 297 }, "dark");
  assert.equal(joined.selection, joined.accent, "left alone, the word you are on wears the accent");
  assert.equal(joined["selection-wash"], joined["accent-wash"]);
  assert.equal(joined["selection-edge"], joined["accent-edge"]);

  const apart = deriveTokens(
    { mode: "dark", accentHue: 297, overrides: { selection: "#22cc66" } },
    "dark",
  );
  assert.equal(apart.selection, "#22cc66");
  assert.notEqual(apart["selection-wash"], apart["accent-wash"], "the wash detached too");
  assert.notEqual(apart["selection-edge"], apart["accent-edge"], "and the edge");
  assert.equal(apart.accent, joined.accent, "and the accent itself is untouched");
});

test("moving the accent still moves the selection that has not been detached", () => {
  // The default has to keep tracking, or "selection follows the accent" is only true at startup.
  const one = deriveTokens({ mode: "dark", accentHue: 297 }, "dark");
  const two = deriveTokens({ mode: "dark", accentHue: 150 }, "dark");
  assert.notEqual(one.selection, two.selection);
  assert.equal(two.selection, two.accent);
});

/**
 * **Everything a saved palette carries reaches the list you pick from.**
 *
 * This is the test that was missing. `allPalettes` rebuilt each saved palette field by field, and
 * when palettes grew decoration it kept rebuilding the old three — so applying one restored the
 * colours and left the watermarks alone, which is indistinguishable from the feature never having
 * been written. Nothing failed, because the read was cast (`palette as { decor?: ... }`) and a
 * cast that asserts a property stops the compiler asking whether it exists.
 *
 * Compared by *keys* rather than by naming decor, so it keeps holding for the next field somebody
 * adds to a palette and forgets here.
 */
test("a saved palette reaches the picker with everything it was saved with", () => {
  const decor = {
    cloud: 0.13,
    cloud_tint: "accent",
    sakura: 0.31,
    sakura_tint: "blossom",
    scale: 1.7,
    cloud_pad: 2.0,
    cloud_spread: 2.4,
    sakura_spread: 0.6,
    animations: false,
    animation_ms: 320,
  };
  const saved = {
    name: "DecorTest",
    theme: {
      mode: "dark",
      surface_hue: 210,
      accent_hue: 40,
      tint: 1.4,
      contrast: 0.9,
      depth: 0.03,
      overrides: { blue: "#ff8800" },
    },
    decor,
  };

  const listed = palettesFor(PRESETS, [saved]).find((entry) => entry.name === "DecorTest");
  assert.ok(listed, "the saved palette is in the list");

  for (const key of Object.keys(saved)) {
    assert.ok(
      key in listed,
      `a saved palette carries "${key}" and the picker drops it — applying one cannot restore it`,
    );
  }
  assert.deepEqual(listed.theme, saved.theme, "the colours arrive whole");
  assert.deepEqual(listed.decor, decor, "and so does the decoration");
  assert.equal(listed.mine, true, "and it is marked as the learner's, so it can be renamed");
});

test("a shipped palette carries no decoration, and says so rather than omitting it", () => {
  // Present-and-undefined, not absent: both halves of the list have the same shape, so a caller
  // reading `.decor` gets `undefined` rather than a property that is not there on some entries.
  const sumi = palettesFor(PRESETS, []).find((entry) => entry.name === "Sumi");
  assert.ok("decor" in sumi, "the field exists on shipped entries too");
  assert.equal(sumi.decor, undefined, "and carries nothing, so picking one leaves decoration be");
  assert.equal(sumi.mine, false);
  // A shipped palette is the derivation with nothing bent.
  assert.deepEqual({ tint: sumi.theme.tint, contrast: sumi.theme.contrast, depth: sumi.theme.depth },
    { tint: 1, contrast: 1, depth: 0 });
  assert.deepEqual(sumi.theme.overrides, {});
});

test("the learner's palettes come after the ones that ship", () => {
  const listed = palettesFor(PRESETS, [{ name: "Mine", theme: { mode: "dark" }, decor: null }]);
  assert.equal(listed.length, PRESETS.length + 1);
  assert.equal(listed[listed.length - 1].name, "Mine");
});

// --- Part-of-speech hues --------------------------------------------------------------------

/** The gaps between the six kind hues, sorted — the shape that has to survive a turn. */
const spacing = (hues) => {
  const ring = Object.values(hues).sort((a, b) => a - b);
  return ring.map((h, i) => (ring[(i + 1) % ring.length] - h + 360) % 360).sort((a, b) => a - b);
};

test("turning the kind wheel moves every kind and changes none of the gaps", () => {
  const rest = spacing(kindHues(0));
  for (const spin of [12, 31, 97, 180, 359]) {
    const turned = kindHues(spin);
    assert.deepEqual(spacing(turned), rest, `spacing survives ${spin}°`);
    for (const [name, hue] of Object.entries(KIND_HUES)) {
      assert.equal(turned[name], (hue + spin) % 360, `${name} turned by ${spin}°`);
    }
  }
});

test("a turn past the circle wraps rather than running off it", () => {
  assert.deepEqual(kindHues(360), kindHues(0));
  for (const hue of Object.values(kindHues(400))) {
    assert.ok(hue >= 0 && hue < 360, `${hue} is on the circle`);
  }
});

/**
 * **The numbers on `PRESETS` are measured, and this is the measurement.**
 *
 * The failure they exist to prevent is a kind landing on the palette's accent: untuned, `Function`
 * sits 11° from Ai's and `Verb` 5° from Kohi's, which on screen is one colour wearing two names.
 * 36° is the ceiling — the kind set has two 36° gaps of its own, so no rotation can put every kind
 * further than that from any one hue — and every shipped palette is asserted to reach it.
 */
test("every shipped palette keeps its kinds clear of its accent", () => {
  for (const preset of PRESETS) {
    const hues = Object.values(kindHues(preset.theme.kindSpin ?? 0));
    const nearest = kindClearance(preset.theme.kindSpin ?? 0, preset.theme.accentHue ?? 297);
    assert.ok(
      nearest >= KIND_CLEARANCE_FLOOR,
      `${preset.name}: nearest kind is ${nearest.toFixed(0)}° from its accent, wanted ${KIND_CLEARANCE_FLOOR}°`,
    );
  }
});

test("the untuned wheel is what the shipped spins are correcting", () => {
  // Negative side of the test above: without their spins, two palettes fail it outright. If this
  // ever stops being true the spins have become decoration and should be reconsidered, not kept.
  assert.ok(kindClearance(0, 250) < 36, "Ai's accent sits inside the untuned wheel");
  assert.ok(kindClearance(0, 40) < 36, "Kohi's accent sits inside the untuned wheel");
});

test("a kind set by hand takes its wash with it", () => {
  const picked = "#ff0000";
  const tokens = dark({ overrides: { "kind-noun": picked } });
  assert.equal(tokens["kind-noun"], picked);
  // The wash is what a chosen filter chip fills with. Derived from the hue, it would have stayed
  // the old colour and the row you narrow by would stop matching the senses you read.
  // The colour that was picked, at the alpha every wash uses — so a chosen kind sits in the
  // interface exactly the way a derived one does.
  assert.equal(tokens["kind-noun-wash"], "rgba(255, 0, 0, 0.16)");
  // And only that kind moves.
  assert.equal(tokens["kind-verb"], dark()["kind-verb"]);
});

test("a hand-set kind wash is left alone", () => {
  const tokens = dark({
    overrides: { "kind-noun": "#ff0000", "kind-noun-wash": "#00ff00" },
  });
  assert.equal(tokens["kind-noun-wash"], "#00ff00");
});

test("the spin reaches the tokens, and moves every kind", () => {
  const rest = dark();
  const turned = dark({ kindSpin: 97 });
  for (const name of Object.keys(KIND_HUES)) {
    assert.notEqual(turned[`kind-${name}`], rest[`kind-${name}`], `${name} moved`);
    assert.notEqual(turned[`kind-${name}-wash`], rest[`kind-${name}-wash`], `${name} wash moved`);
  }
  // And nothing else did: the spin is the kinds' dial and no one else's.
  for (const token of ["bg", "surface", "text", "accent", "blue", "yellow", "good", "red"]) {
    assert.equal(turned[token], rest[token], `${token} untouched`);
  }
});

test("a palette carries its spin into the settings it writes", () => {
  const ai = palettesFor(PRESETS, []).find((palette) => palette.name === "Ai");
  assert.equal(ai.theme.kind_spin, 97);
});

test("the best rotation reaches the ceiling for any accent", () => {
  for (let accent = 0; accent < 360; accent += 7) {
    const { spin, clearance } = bestKindSpin(accent);
    assert.equal(clearance, KIND_CLEARANCE_FLOOR, `accent ${accent}° reaches the ceiling`);
    assert.equal(kindClearance(spin, accent), clearance, `the spin it names is the spin it measured`);
  }
});

test("the shipped spins are the ones this calculation produces", () => {
  // The numbers on PRESETS were not chosen by eye. Any rotation reaching the ceiling would do, so
  // this asserts the clearance rather than the exact degree — but it does prove the shipped value
  // is one the app itself would pick.
  for (const preset of PRESETS) {
    const accent = preset.theme.accentHue ?? 297;
    assert.equal(
      kindClearance(preset.theme.kindSpin ?? 0, accent),
      bestKindSpin(accent).clearance,
      `${preset.name} is as clear of its accent as any rotation can be`,
    );
  }
});

// --- Which palette a theme is ---------------------------------------------------------------

const shipped = palettesFor(PRESETS, []);

test("a shipped palette recognises itself", () => {
  for (const palette of shipped) {
    assert.equal(matchPalette(shipped, palette.theme), palette.name);
  }
});

test("a theme that matches nothing is nobody's palette", () => {
  const sumi = shipped.find((palette) => palette.name === "Sumi");
  const moved = { ...sumi.theme, accent_hue: 120 };
  assert.equal(matchPalette(shipped, moved), null);
});

/**
 * The regression this exists for: `kind_spin` was added to the theme after this comparison was
 * written and left out of it, so turning the kind wheel left the list still naming Ai while
 * showing colours Ai does not have.
 */
test("turning the kind wheel alone stops a palette matching", () => {
  const ai = shipped.find((palette) => palette.name === "Ai");
  assert.equal(matchPalette(shipped, ai.theme), "Ai");
  assert.equal(matchPalette(shipped, { ...ai.theme, kind_spin: 200 }), null);
});

test("a fractional hue from an older file still matches a whole-degree palette", () => {
  const sumi = shipped.find((palette) => palette.name === "Sumi");
  assert.equal(matchPalette(shipped, { ...sumi.theme, accent_hue: 296.6 }), "Sumi");
});

test("mode is part of the match, so Sumi and Washi never answer for each other", () => {
  const sumi = shipped.find((palette) => palette.name === "Sumi");
  assert.equal(matchPalette(shipped, { ...sumi.theme, mode: "light" }), "Washi");
});
