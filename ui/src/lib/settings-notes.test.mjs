import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { SECTION_ICONS } from "./settings-icons.js";

/**
 * The shape of a settings row: one short line you always see, and the reasoning behind a
 * **More** you open when you want it.
 *
 * Each section used to print every explanation in full — 57 notes, most of them two to six
 * lines — so a section showed about four settings to a screen and you read paragraphs to find
 * a switch. These tests keep the short line short, keep a block's heading honest under search,
 * keep each explanation in one home, and keep the nav's icons and descriptions complete.
 */

const read = (path) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), "utf8");
const component = read("./Settings.svelte");
const help = read("./Help.svelte");
const model = read("../model.ts");

/** Only the markup: the script and the styles hold no rows. */
const markup = component.slice(component.indexOf("</script>"), component.indexOf("<style>"));

const SECTIONS = [...model.matchAll(/\{ id: "(\w+)", label: "([^"]+)" \}/g)].map((found) => ({
  id: found[1],
  label: found[2],
}));

/** Which section an offset in `markup` falls in, or null before the first. */
function sectionAt(offset) {
  let found = null;
  for (const opened of markup.matchAll(/\{#if section === "(\w+)"\}/g)) {
    if (opened.index > offset) break;
    found = opened[1];
  }
  return found;
}

/**
 * The words a piece of markup prints, one string per `{#if}` branch.
 *
 * Expressions are dropped rather than guessed at — `{deckTarget}` is a deck name nobody can
 * measure from here — so what is counted is the copy somebody wrote. A block marker (`{#if}`,
 * `{:else}`, `{/if}`, `{#each}`) ends one branch and starts the next. Braces are matched rather
 * than found by pattern, because an expression can hold a template literal with braces of its own.
 */
function branches(source) {
  const text = source.replace(/<!--[\s\S]*?-->/g, "");
  const out = [""];
  for (let at = 0; at < text.length; at += 1) {
    if (text[at] !== "{") {
      out[out.length - 1] += text[at];
      continue;
    }
    let depth = 0;
    let end = at;
    for (; end < text.length; end += 1) {
      if (text[end] === "{") depth += 1;
      else if (text[end] === "}" && --depth === 0) break;
    }
    assert.ok(end < text.length, `unbalanced braces in: ${source.slice(at, at + 60)}`);
    if (/^\{[#:/]/.test(text.slice(at, at + 2))) out.push("");
    at = end;
  }
  return out
    .map((part) =>
      part
        .replace(/<[^>]+>/g, "")
        .replace(/\s+/g, " ")
        .trim(),
    )
    .filter(Boolean);
}

/** Every `.note` paragraph in the markup, with the section it sits in. */
function notes() {
  return [...markup.matchAll(/<p\s+class="note[^"]*"[^>]*>([\s\S]*?)<\/p>/g)].map((found) => ({
    source: found[0],
    body: found[1],
    section: sectionAt(found.index),
  }));
}

/**
 * Notes allowed past the limit, each with its reason.
 *
 * A readout is not an explanation: it is the app reporting a measurement, and its length is
 * the sentence the numbers need. Each entry must match exactly one note, so one that no longer
 * matches anything fails instead of quietly exempting nothing.
 */
const LONG_ON_PURPOSE = [
  {
    match: "visible requests used",
    why: "the session-acceleration readout: four live numbers in one sentence",
  },
];

/**
 * **The short line is short.** 110 characters of copy is a writing limit, not a promise of one
 * rendered line — at the largest interface size in a 480px window nothing that long fits on one,
 * and the notes are free to wrap. `settings-layout.spec.ts` checks the rendering: no clipping,
 * no ellipsis, nothing escaping its card. This checks that the reasoning went into More rather
 * than staying on screen.
 */
test("a setting's visible note is one short line of copy", () => {
  const all = notes();
  assert.ok(all.length >= 35, `found ${all.length} notes; the parse has stopped seeing them`);

  for (const allowed of LONG_ON_PURPOSE) {
    const hits = all.filter((note) => note.source.includes(allowed.match));
    assert.equal(hits.length, 1, `LONG_ON_PURPOSE "${allowed.match}" matches ${hits.length} notes`);
  }

  const long = [];
  for (const note of all) {
    if (LONG_ON_PURPOSE.some((allowed) => note.source.includes(allowed.match))) continue;
    for (const branch of branches(note.body)) {
      if (branch.length > 110) long.push(`${note.section}: ${branch.length} — ${branch}`);
    }
  }
  assert.deepEqual(
    long,
    [],
    `a note runs past 110 characters; move the reasoning into More:\n${long.join("\n")}`,
  );
});

/**
 * **Every More says which setting it is about.** On screen it reads "More" and its row is beside
 * it; to a screen reader's list of buttons it would be one of forty identical "More"s. The rest
 * of its name is hidden text taken from the search index's own label, so the name and the search
 * result cannot call a setting two different things.
 */
test("every More is named after its setting, from the search index", () => {
  const folds = [
    ...markup.matchAll(/<details class="more"[^>]*>\s*<summary>([\s\S]*?)<\/summary>/g),
  ];
  for (const fold of folds) {
    assert.match(
      fold[1],
      /^More <span class="offscreen">about \{FIND\.\w+\.\w+\.label\}<\/span>$/,
      `a More's summary is not the named form: ${fold[1]}`,
    );
  }
});

/**
 * Each block's heading survives exactly the searches that leave one of its rows.
 *
 * A block hides when `some(...)` of its rows match. A row inside it that the list forgets is a
 * row that can match a search while its heading and card are hidden around it — found by
 * search, announced in the results, and invisible when you go there. "How the text is read"
 * shipped that way: it listed the analyzer and not the Ichiran switch beneath it.
 */
test("a block hides only when every row inside it does", () => {
  const starts = [...markup.matchAll(/<div\s+class="block"\s+hidden=\{!some\(([^)]*)\)\}\s*>/g)];
  const ends = [...markup.matchAll(/<div\s+class="block"|\{#if section === "\w+"\}/g)].map(
    (m) => m.index,
  );
  const missing = [];
  for (const start of starts) {
    const section = sectionAt(start.index);
    const listed = new Set(
      [...start[1].matchAll(/FIND\.(\w+)\.(\w+)/g)].map((m) => `${m[1]}.${m[2]}`),
    );
    const after = start.index + start[0].length;
    const end = ends.find((offset) => offset > after) ?? markup.length;
    for (const used of markup.slice(after, end).matchAll(/FIND\.(\w+)\.(\w+)/g)) {
      const key = `${used[1]}.${used[2]}`;
      if (!listed.has(key))
        missing.push(`${section}: a block lists ${[...listed].join(", ")} but holds ${key}`);
    }
  }
  assert.deepEqual([...new Set(missing)], [], missing.join("\n"));
});

/** Every row in a converted section sits in a block, so none floats outside a card. */
test("every row is inside a block", () => {
  const firstBlock = new Map();
  for (const start of markup.matchAll(/<div\s+class="block"/g)) {
    const section = sectionAt(start.index);
    if (!firstBlock.has(section)) firstBlock.set(section, start.index);
  }
  for (const section of SECTIONS) {
    const open = markup.indexOf(`{#if section === "${section.id}"}`);
    const first = firstBlock.get(section.id);
    assert.ok(first, `${section.label} has no blocks`);
    assert.doesNotMatch(
      markup.slice(open, first),
      /FIND\./,
      `${section.label} has a row before its first block`,
    );
  }
});

/**
 * **One home per explanation.** Help lists the sections and opens them; the reasoning for a row
 * lives in that row's More. A sentence in both is two copies that will disagree the first time
 * one is edited — which is the rule Help's own header comment has always stated.
 */
test("Help repeats no sentence from a setting's More", () => {
  const helpText = branches(help.slice(help.indexOf("</script>"))).join(" ");
  const bodies = [
    ...markup.matchAll(/<details class="more"[^>]*>[\s\S]*?<\/summary>([\s\S]*?)<\/details>/g),
  ];
  const repeated = [];
  for (const body of bodies) {
    for (const branch of branches(body[1])) {
      for (const sentence of branch.split(/(?<=[.!?])\s+/)) {
        if (sentence.length >= 40 && helpText.includes(sentence)) repeated.push(sentence);
      }
    }
  }
  assert.deepEqual(repeated, [], `Help repeats a setting's explanation:\n${repeated.join("\n")}`);
});

test("every section has an icon drawn as paths", () => {
  assert.deepEqual(
    Object.keys(SECTION_ICONS),
    SECTIONS.map((section) => section.id),
  );
  for (const [id, paths] of Object.entries(SECTION_ICONS)) {
    assert.ok(paths.length > 0, `${id} has no icon`);
    for (const d of paths) assert.match(d, /^M[\d.\s,a-zA-Z-]+$/, `${id}: not a path: ${d}`);
  }
});

test("every section has one line saying what it is for", () => {
  const block = model.slice(model.indexOf("export const SETTINGS_ABOUT"));
  const about = [...block.slice(0, block.indexOf("};")).matchAll(/^\s+(\w+): "([^"]+)",$/gm)];
  assert.deepEqual(
    about.map((found) => found[1]),
    SECTIONS.map((section) => section.id),
    "SETTINGS_ABOUT lists the sections in their order",
  );
  for (const [, id, line] of about) {
    assert.ok(line.length <= 90, `${id}'s description is ${line.length} characters`);
    assert.match(line, /\.$/, `${id}'s description is a sentence`);
  }
});
