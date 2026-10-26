import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

/**
 * **A global rule reaches every component, including the ones that were not asking.**
 *
 * `theme.css` is not scoped, so a class name it styles lands on every element wearing that name
 * anywhere in the app. That is the point for the shared vocabulary below — `.control`, `.page`,
 * `.cap` — and a trap for anything else: a `.mark` rule written for status chips once crushed the
 * app's logo, which had been `.mark` privately for months. `traps.md` records it.
 *
 * Svelte scopes a component's own styles, so the danger is one-directional: a *component* cannot
 * leak into `theme.css`'s territory, but `theme.css` can walk into any component that happens to
 * use the same word. This is the list of names where that is intended, and the test is what makes
 * adding a sixteenth a decision rather than an accident.
 */

const here = (name) => fileURLToPath(new URL(name, import.meta.url));
const theme = readFileSync(here("../theme.css"), "utf8");

const components = readdirSync(here("."), { withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".svelte"))
  .map((entry) => ({ name: entry.name, source: readFileSync(here(entry.name), "utf8") }))
  .concat({ name: "App.svelte", source: readFileSync(here("../App.svelte"), "utf8") });

/**
 * The shared vocabulary: a class `theme.css` draws that more than one component wears **on
 * purpose**. Each is a control or a surface the app owns rather than a name a component picked for
 * itself, which is exactly the distinction that keeps a global rule honest.
 */
const SHARED = new Set([
  "control", //      the app's button
  "primary", //      …and its two emphases
  "danger",
  "tip-right", //    the tooltip's side
  "page", //         a view, and the scroller inside it
  "page-body",
  "cap", //          a title-bar button — Caption and AppMenu
  "glyph", //        24-viewBox artwork inside one
  "section", //      a heading over a run of facts
  "spinner", //      work in progress
  "flag", //         a small stated fact, and its plain variant
  "base",
  "disclose", //    a result card's hidden control — Reference and Glossary both carry one
  "card", //        …and the card its keyboard focus is drawn on
  "exit", //        the × out of a detour page — Settings and Help
  "offscreen", //   text for assistive technology only — App's page headings, Settings' More names
]);

/** Class names `theme.css` styles, so every component wearing one inherits it. */
function globallyStyled() {
  const body = theme.replace(/\/\*[\s\S]*?\*\//g, "");
  return new Set([...body.matchAll(/\.([a-z][a-z0-9-]*)/g)].map((match) => match[1]));
}

/** Class names a component puts in its markup, static and conditional alike. */
function wornBy(source) {
  const names = new Set();
  for (const match of source.matchAll(/class="([^"]*)"/g)) {
    for (const word of match[1].matchAll(/[a-z][a-z0-9-]*/g)) names.add(word[0]);
  }
  for (const match of source.matchAll(/class:([a-z][a-z0-9-]*)/g)) names.add(match[1]);
  return names;
}

test("every globally styled name shared by two components is one the app means to share", () => {
  const global = globallyStyled();
  const users = new Map();
  for (const { name, source } of components) {
    for (const worn of wornBy(source)) {
      if (global.has(worn)) users.set(worn, [...(users.get(worn) ?? []), name]);
    }
  }

  const unexpected = [...users]
    .filter(([name, files]) => files.length > 1 && !SHARED.has(name))
    .map(([name, files]) => `.${name} — ${files.sort().join(", ")}`);

  assert.deepEqual(
    unexpected,
    [],
    "these class names are styled in `theme.css` and worn by more than one component, which means " +
      "one component's rule is reaching another's markup. Either the name is genuinely shared — add " +
      "it to SHARED with a note saying what it is — or one of them should be renamed:\n" +
      unexpected.join("\n"),
  );
});

/**
 * The other direction of the same trap: a name in `SHARED` that nothing shares any more is a rule
 * kept alive by a list rather than by a caller.
 */
test("the shared list has no names nothing shares", () => {
  const global = globallyStyled();
  const counts = new Map();
  for (const { source } of components) {
    for (const worn of wornBy(source)) {
      if (global.has(worn)) counts.set(worn, (counts.get(worn) ?? 0) + 1);
    }
  }
  const stale = [...SHARED].filter((name) => (counts.get(name) ?? 0) < 2);
  assert.deepEqual(stale, [], `listed as shared but worn by fewer than two components: ${stale}`);
});
