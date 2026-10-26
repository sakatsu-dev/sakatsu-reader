import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { FIND, anyMatch, matches, search } from "./settings-index.js";

const SECTIONS = [
  { id: "reading", label: "General" },
  { id: "appearance", label: "Appearance" },
  { id: "capture", label: "Capture" },
  { id: "search", label: "Search" },
  { id: "recognition", label: "Recognition" },
  { id: "anki", label: "Anki" },
  { id: "diagnostics", label: "Diagnostics" },
  { id: "updates", label: "Updates" },
];

const component = readFileSync(
  fileURLToPath(new URL("./Settings.svelte", import.meta.url)),
  "utf8",
);
const model = readFileSync(fileURLToPath(new URL("../model.ts", import.meta.url)), "utf8");

/** Every `FIND.section.key` in the markup, with the line it is on. */
function referenced() {
  const out = [];
  const lines = component.split("\n");
  for (const [index, line] of lines.entries()) {
    for (const found of line.matchAll(/FIND\.(\w+)\.(\w+)/g)) {
      out.push({ section: found[1], key: found[2], line: index + 1 });
    }
  }
  return out;
}

/** Where each conditionally mounted section starts and ends in the markup. */
function groups() {
  const bounds = [];
  const lines = component.split("\n");
  for (const [index, line] of lines.entries()) {
    const opened = line.match(/\{#if section === "(\w+)"\}/);
    if (opened) {
      if (bounds.length > 0) bounds[bounds.length - 1].end = index;
      bounds.push({ section: opened[1], start: index, end: lines.length });
    }
  }
  return bounds;
}

test("the sections here are the sections the app has", () => {
  // `SETTINGS_SECTIONS` lives in `model.ts` because the title bar builds its menu from it too.
  // Duplicated above so this file needs no bundler; checked against the real one so the copy
  // cannot rot.
  const real = [...model.matchAll(/\{ id: "(\w+)", label: "([^"]+)" \}/g)].map((found) => ({
    id: found[1],
    label: found[2],
  }));
  assert.deepEqual(real, SECTIONS);
});

test("every section has an index, and every index belongs to a section", () => {
  assert.deepEqual(Object.keys(FIND).sort(), SECTIONS.map((s) => s.id).sort());
});

test("every row has a label and something to match on", () => {
  for (const [section, rows] of Object.entries(FIND)) {
    for (const [key, row] of Object.entries(rows)) {
      assert.ok(row.label.trim(), `${section}.${key} has no label`);
      assert.ok(row.keywords.trim(), `${section}.${key} has nothing to match on`);
      // The label is printed in the results list, so anything that is not plain text shows up
      // there verbatim. `Hide decks that aren'''t Sakatsu'''s` shipped like that.
      assert.ok(
        !/['"]{2,}|\\\\/.test(row.label),
        `${section}.${key}'s label has stray quoting: ${JSON.stringify(row.label)}`,
      );
    }
  }
});

/**
 * **The invariant this file exists for.**
 *
 * The index says which section a row is in; the markup decides where it actually renders. A row
 * indexed under one and rendered under another is found by search, announced as being somewhere
 * it is not, and nothing errors — you click the result, land on the section, and it is not
 * there. Moving a row between sections is precisely when this breaks, and moving rows between
 * sections is a thing that will keep happening.
 */
test("a row is indexed under the section that actually renders it", () => {
  const bounds = groups();
  assert.ok(bounds.length === SECTIONS.length, "found every section group in the markup");

  for (const use of referenced()) {
    const group = bounds.find((b) => use.line > b.start && use.line <= b.end);
    assert.ok(group, `FIND.${use.section}.${use.key} on line ${use.line} is outside every group`);
    assert.equal(
      use.section,
      group.section,
      `line ${use.line} renders FIND.${use.section}.${use.key} inside the "${group.section}" section`,
    );
  }
});

test("every indexed row is actually rendered somewhere", () => {
  const used = new Set(referenced().map((use) => `${use.section}.${use.key}`));
  for (const [section, rows] of Object.entries(FIND)) {
    for (const key of Object.keys(rows)) {
      assert.ok(
        used.has(`${section}.${key}`),
        `${section}.${key} is searchable but nothing renders it — a result that goes nowhere`,
      );
    }
  }
});

test("every rendered row is indexed", () => {
  for (const use of referenced()) {
    assert.ok(
      FIND[use.section]?.[use.key],
      `line ${use.line} uses FIND.${use.section}.${use.key}, which is not in the index`,
    );
  }
});

test("an empty query leaves every section whole", () => {
  for (const rows of Object.values(FIND)) {
    for (const row of Object.values(rows)) {
      assert.equal(matches(row, ""), true);
    }
  }
  assert.deepEqual(search("", SECTIONS), []);
});

test("a search names the section the row is really in", () => {
  const found = search("installation", SECTIONS);
  assert.equal(found.length, 1);
  assert.equal(found[0].section, "recognition");
  assert.equal(found[0].sectionLabel, "Recognition");
  assert.equal(found[0].label, "Repair screen capture installation");
});

test("installation and resource storage live with recognition", () => {
  assert.equal("setup" in FIND.appearance, false);
  assert.equal("setup" in FIND.diagnostics, false);
  assert.equal("installation" in FIND.recognition, true);
  assert.equal("storage" in FIND.recognition, true);
});

test("a subheading knows whether anything under it survived the search", () => {
  const cluster = [FIND.appearance.hues, FIND.appearance.dials];
  assert.equal(anyMatch("", cluster), true, "an empty query keeps everything");
  assert.equal(anyMatch("contrast", cluster), true, "one match is enough");
  assert.equal(anyMatch("furigana", cluster), false, "and no match hides the heading");
});

test("no two rows in one section answer to the same name", () => {
  for (const [section, rows] of Object.entries(FIND)) {
    const labels = Object.values(rows).map((row) => row.label);
    assert.equal(
      new Set(labels).size,
      labels.length,
      `${section} has two rows with the same label, so a result cannot say which`,
    );
  }
});

/**
 * **Every slider moves by one unit of the number printed beside it.**
 *
 * The steps were whatever each row was written with: 5% on the scales, 2% on contrast, 0.5% on
 * the clouds, 50ms on two of the timings, 25ms on a third. Nothing was wrong with any one of
 * them, and together they made a page where dragging one slider one notch moved 1 and the next
 * moved 50 — so the only way to find out what a slider would do was to drag it.
 *
 * The rule is the label: a row printing a percentage steps 0.01, one printing whole milliseconds
 * or pixels steps 1. That keeps the control and its readout saying the same thing, and it is
 * checkable, which a convention that lives only in a review comment is not.
 */
test("a slider steps by one unit of what its own label shows", () => {
  const rows = component.split(/<label\b/);
  const checked = [];
  for (const row of rows) {
    const range = row.match(/type="range"[^>]*/s);
    if (!range) continue;
    const step = range[0].match(/step="([\d.]+)"/);
    const shown = row.match(/<em>(.*?)<\/em>/s);
    if (!step || !shown) continue;

    // What the readout is in: a percentage is the value times a hundred, so one displayed unit
    // is 0.01 of it; one decimal place of rem is 0.1; anything printed whole is 1.
    const label = shown[1];
    const expected = label.includes("* 100)") ? "0.01" : label.includes("toFixed(1)") ? "0.1" : "1";
    checked.push({ label: label.replace(/\s+/g, " ").trim(), step: step[1], expected });
  }

  assert.ok(checked.length >= 18, `found ${checked.length} sliders, expected the whole page`);
  for (const row of checked) {
    assert.equal(
      row.step,
      row.expected,
      `a slider reading "${row.label}" steps by ${row.step}, which is not one of what it prints`,
    );
  }
});

/**
 * The one slider whose floor is not its minimum.
 *
 * `Settings::lock_tap` passes zero through as "off" and clamps everything else up to 150, so
 * every value between is one the control can show and the app will never hold. `snapLockTap`
 * closes that; this checks the number it closes it to is still the number Rust uses, because the
 * two live in different languages in different files and nothing else connects them.
 */
test("the double-tap floor in the interface is the floor the store enforces", () => {
  const store = readFileSync(
    fileURLToPath(new URL("../../../core/src/store/mod.rs", import.meta.url)),
    "utf8",
  );
  const clamp = store.match(/self\.lock_tap_ms\.clamp\((\d+), 1_000\)/);
  assert.ok(clamp, "lock_tap still clamps in the store");
  // A literal substring, not a pattern: the point is that this exact number appears in that
  // exact expression, and a regex here only adds escaping to get wrong.
  assert.ok(
    component.includes(`Math.max(${clamp[1]}, Math.min(1000, ms))`),
    `the store's floor is ${clamp[1]}ms and snapLockTap does not use it`,
  );
});

/**
 * **A heading that repeats the row under it says nothing twice.**
 *
 * `APPEARANCE › Palette › Palette` was the case that prompted this: a section heading, a
 * subheading and a row label, two of them the same word, before a single control. A subheading
 * earns its line by naming what the rows below it have in common — if the first row already says
 * it, the heading is a blank line with text in it.
 *
 * Compared against the *first* row only. A group whose later rows happen to share a word with it
 * is fine — "Decoration" over Clouds and Sakura is doing its job.
 */
test("no subheading repeats the label of the row beneath it", () => {
  const lines = component.split("\n");
  const repeats = [];
  for (const [at, line] of lines.entries()) {
    if (!line.includes('class="subhead"')) continue;
    const inline = line.match(/>([^<>]+)<\/h4>/);
    const heading = inline
      ? inline[1].trim()
      : (lines.slice(at + 1, at + 6).find((next) => next.includes("</h4>")) ?? "")
          .replace(/<\/?h4[^>]*>/g, "")
          .trim();
    const row = lines
      .slice(at + 1, at + 20)
      .map((next) => next.match(/<span>([^<{]+?)\s*(?:<em>|<\/span>)/))
      .find(Boolean);
    const label = row ? row[1].trim() : "";
    if (heading && label && heading.toLowerCase() === label.toLowerCase()) {
      repeats.push(`line ${at + 1}: "${heading}" over "${label}"`);
    }
  }
  assert.deepEqual(repeats, [], `a subheading repeats its first row:\n${repeats.join("\n")}`);
});
