import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const here = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(here, "Reference.svelte"), "utf8");

test("Reference uses stable inline rows and retires only its layout control", () => {
  assert.doesNotMatch(source, /LayoutChoice|promoteSelected|class:cards/);
  assert.match(source, /const arranged = \$derived\(visible\)/);
  assert.match(source, /{#each arranged as result, index \(result\.key\)}/);
  assert.match(source, /const result = arranged\[next\]/);
  assert.match(source, /aria-expanded={expanded}/);
  assert.match(source, /aria-controls={detailId}/);
});

test("heavy detail affordances only mount under a selected union member", () => {
  const detailStart = source.indexOf("{#if expanded}");
  assert.notEqual(detailStart, -1, "the selected inline detail should remain identifiable");
  // **Every use is behind the fold, and the count is not the test.** It used to insist on
  // exactly one use of each, which was a proxy for "not mounted in a preview" and stopped being
  // true the moment `Character` earned a second home: the panel beside an opened word, which the
  // detail places itself so the kanji sits next to the senses rather than stacked above them.
  // Both uses are still inside `{#if expanded}`, which is the property that actually matters.
  // `WordHeader` was here until the characters stopped opening one at a time. It existed to draw
  // the row of chips that chose between them, so a column that shows every character has no use
  // for it — and it had no other caller.
  for (const component of ["WordBody", "WordCharacters", "Character", "TermDetail"]) {
    const uses = [...source.matchAll(new RegExp(`<${component}`, "g"))].map((match) => match.index);
    assert.ok(uses.length > 0, `${component} is drawn somewhere`);
    for (const at of uses) {
      assert.ok(at > detailStart, `every ${component} belongs to expansion, not to a preview`);
    }
  }
  // The cap, and now also *which* two. `sensesForKinds` picks the senses the active filter
  // matched, so a card filtered to Adverb stops showing its noun glosses; the `slice` after it
  // is what keeps a preview cheap either way.
  assert.match(
    source,
    /sensesForKinds\(word, tab\.wordKinds\)\.slice\(0, 2\)/,
    "word previews expose at most two meanings, chosen by the active filter",
  );
});

test("frequency-source prose remains an independent opt-in", () => {
  assert.match(source, /showFrequencySource: boolean/);
  assert.match(source, /{#if showFrequencySource && tab\.rankedBy/);
  assert.match(source, /Sorted most common first by {tab\.rankedBy}\./);
});

test("POS membership uses every sense and filtering never duplicates a result", () => {
  assert.match(source, /wordKinds\(result\.value\)\.some/);
  assert.match(source, /faceted\.filter/);
  assert.doesNotMatch(source, /flatMap\([^)]*wordKinds/);
});

test("empty search, no results, and filtered-out results are distinct states", () => {
  assert.match(source, /!tab\.asked/);
  assert.match(source, /results\.length === 0/);
  assert.match(source, /visible\.length === 0/);
  assert.match(source, /No results remain under these filters/);
});

test("every button in the search form declares a type", () => {
  const form = source.match(/<form[\s\S]*?<\/form>/)?.[0];
  assert.ok(form);
  const bare = [...form.matchAll(/<button(?![^>]*\stype[=\s])[^>]*>/g)].map((match) => match[0]);
  assert.deepEqual(bare, []);
});
