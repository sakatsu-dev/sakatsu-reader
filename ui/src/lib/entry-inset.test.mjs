import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (name) => readFileSync(new URL(`./${name}`, import.meta.url), "utf8");
const theme = readFileSync(new URL("../theme.css", import.meta.url), "utf8");
const example = read("ExampleText.svelte");
const sense = read("SenseRow.svelte");
const heading = read("SenseGroupHeading.svelte");

/**
 * **One inset, three surfaces, and the bug that comes of writing it out three times.**
 *
 * An example, a sense, and the heading above a run of senses are stacked in one card and read as
 * one column. They were spaced by three separate hand-written values: the senses were moved to
 * match the examples and the group headings were left behind six pixels out, and an *unclassified*
 * heading had no inset at all — so where a run of meanings started depended on whether its tags
 * happened to name a kind the app knows.
 *
 * This is asserted on the source rather than on a rendered page because **no fixture draws a group
 * heading**: it needs a run of senses sharing a classification, and Reference's balanced
 * presentation does not group at all. That is a coverage hole worth naming rather than papering
 * over — the shared token is what makes the three agree, so the token is what is checked.
 */
test("the two hairline halves are named, not hand-computed", () => {
  assert.match(theme, /--entry-rule:\s*2px;/);
  assert.match(theme, /--entry-gap:\s*0\.7rem;/);
  // The sum exists so no caller has to write `calc(0.7rem + 2px)` for itself.
  assert.match(theme, /--entry-inset:\s*calc\(var\(--entry-rule\) \+ var\(--entry-gap\)\);/);
});

test("an example takes the gap, because its rule already takes the space", () => {
  assert.match(example, /border-left: var\(--entry-rule\) solid/);
  assert.match(example, /padding: [^;]*var\(--entry-gap\)/);
  assert.doesNotMatch(example, /padding: [^;]*0\.7rem/);
});

test("a sense takes the sum, because an inset shadow takes no space", () => {
  assert.match(sense, /\.sense \{\s*padding-left: var\(--entry-inset\);/);
  assert.match(sense, /box-shadow: inset var\(--entry-rule\) 0 var\(--kind\)/);
  assert.doesNotMatch(sense, /padding-left: calc\(0\.7rem/);
});

test("a group heading takes the same sum, and takes it whether or not it is classified", () => {
  assert.match(heading, /\.heading \{[^}]*padding-left: var\(--entry-inset\);/s);
  // The padding must not live on `.classified`: where a heading sits is not a question about
  // whether its tags name a colour, and that is exactly how the two came apart.
  assert.doesNotMatch(heading, /\.heading\.classified \{[^}]*padding-left/s);
  assert.match(heading, /box-shadow: inset var\(--entry-rule\) 0 var\(--kind\)/);
});

test("nothing in the three still writes the old literals", () => {
  for (const [name, source] of [["ExampleText", example], ["SenseRow", sense], ["SenseGroupHeading", heading]]) {
    assert.doesNotMatch(source, /padding-left: var\(--gap-sm\)/, `${name} left on the old inset`);
  }
});
