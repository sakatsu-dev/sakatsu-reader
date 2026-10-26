import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { groupSenseSections, orderSensesForDisplay } from "./sense-groups.js";

const sense = (tags, credit = null) => ({ sense: { tags }, credit });
const groupShape = (groups) =>
  groups.map(({ items, start, sharedTags }) => [items.length, start, sharedTags]);

test("one classification shared by every sense is printed once", () => {
  const [section] = groupSenseSections([
    sense(["い-adjective"]),
    sense(["い-adjective"]),
    sense(["い-adjective"]),
  ]);

  assert.deepEqual(section.sharedTags, ["い-adjective"]);
  assert.deepEqual(groupShape(section.groups), [[3, 0, []]]);
});

test("several parts of speech get one stable umbrella each", () => {
  const [section] = groupSenseSections([
    sense(["particle"]),
    sense(["particle"]),
    sense(["particle"]),
    sense(["conjunction"]),
    sense(["conjunction"]),
  ]);

  assert.deepEqual(section.sharedTags, []);
  assert.deepEqual(groupShape(section.groups), [
    [3, 0, ["particle"]],
    [2, 3, ["conjunction"]],
  ]);
});

test("a one-off interjection cannot split one particle classification", () => {
  const first = sense(["particle"]);
  const second = sense(["particle"]);
  const interjection = sense(["interjection"]);
  const third = sense(["particle"]);
  const fourth = sense(["particle"]);
  const [section] = groupSenseSections([first, second, interjection, third, fourth]);

  assert.deepEqual(groupShape(section.groups), [
    [4, 0, ["particle"]],
    [1, 4, []],
  ]);
  assert.deepEqual(section.items, [first, second, third, fourth, interjection]);
  assert.equal(section.groups[1].items[0].sense.tags[0], "interjection");
});

test("a secondary tag stays on a row inside its primary classification", () => {
  const both = sense(["particle", "interjection"]);
  const [section] = groupSenseSections([sense(["particle"]), both]);

  assert.deepEqual(section.sharedTags, ["particle"]);
  assert.deepEqual(groupShape(section.groups), [
    [1, 0, []],
    [1, 1, []],
  ]);
  assert.deepEqual(
    section.groups[1].items[0].sense.tags.filter(
      (tag) => !section.sharedTags.includes(tag),
    ),
    ["interjection"],
  );
});

test("a whole-entry verb class and a narrower repeated role are both printed once", () => {
  const godan = "godan verb ending in る (irregular)";
  const [section] = groupSenseSections([
    sense([godan, "intransitive"]),
    sense([godan, "intransitive"]),
    sense([godan, "intransitive"]),
    sense([godan, "intransitive"]),
    sense([godan, "intransitive"]),
    sense([godan, "auxiliary verb"]),
  ]);

  assert.deepEqual(section.sharedTags, [godan]);
  assert.deepEqual(groupShape(section.groups), [
    [5, 0, ["intransitive"]],
    [1, 5, []],
  ]);
  assert.deepEqual(
    section.groups[1].items[0].sense.tags.filter(
      (tag) =>
        !section.sharedTags.includes(tag) && !section.groups[1].sharedTags.includes(tag),
    ),
    ["auxiliary verb"],
  );
});

test("shared base tags remain above senses with different qualifiers", () => {
  const [section] = groupSenseSections([
    sense(["godan verb", "intransitive"]),
    sense(["godan verb", "transitive"]),
    sense(["godan verb", "archaic"]),
  ]);

  assert.deepEqual(section.sharedTags, ["godan verb"]);
  assert.deepEqual(groupShape(section.groups), [
    [1, 0, []],
    [1, 1, []],
    [1, 2, []],
  ]);
});

test("unrelated uncredited senses keep their own tags", () => {
  const [section] = groupSenseSections([sense(["noun"]), sense(["adverb"])]);

  assert.deepEqual(section.sharedTags, []);
  assert.deepEqual(groupShape(section.groups), [
    [1, 0, []],
    [1, 1, []],
  ]);
});

test("source credits make separate sections and preserve numbering offsets", () => {
  const sections = groupSenseSections(
    [
      sense(["noun"], "Jitendex"),
      sense(["expression"], "Jitendex"),
      sense(["noun"], "JMnedict"),
    ],
    4,
  );

  assert.deepEqual(
    sections.map(({ items, start, sharedTags, credit }) => [
      items.length,
      start,
      sharedTags,
      credit,
    ]),
    [
      [2, 4, [], "Jitendex"],
      [1, 6, [], "JMnedict"],
    ],
  );
  assert.deepEqual(groupShape(sections[0].groups), [
    [1, 4, []],
    [1, 5, []],
  ]);
});

test("stable grouping never crosses a dictionary credit boundary", () => {
  const sections = groupSenseSections([
    sense(["particle"], "Jitendex"),
    sense(["interjection"], "Jitendex"),
    sense(["particle"], "Jitendex"),
    sense(["particle"], "JMdict"),
  ]);

  assert.deepEqual(groupShape(sections[0].groups), [
    [2, 0, ["particle"]],
    [1, 2, []],
  ]);
  assert.deepEqual(groupShape(sections[1].groups), [[1, 3, []]]);
});

test("stable grouping never crosses a retrieval-scope boundary", () => {
  const whole = { sense: { tags: ["noun"] }, credit: null, component: null };
  const component = { sense: { tags: ["noun"] }, credit: null, component: "神" };
  const sections = groupSenseSections([whole, component]);

  assert.equal(sections.length, 2);
  assert.equal(sections[0].component, null);
  assert.equal(sections[1].component, "神");
  assert.deepEqual(sections[0].items, [whole]);
  assert.deepEqual(sections[1].items, [component]);
});

test("the bare display order follows the same stable classification groups", () => {
  const first = { tags: ["particle"], glosses: ["first"] };
  const interruption = { tags: ["interjection"], glosses: ["interruption"] };
  const second = { tags: ["particle"], glosses: ["second"] };

  assert.deepEqual(orderSensesForDisplay([first, interruption, second]), [
    first,
    second,
    interruption,
  ]);
});

test("Analysis classification headings never rewrite contextual rank order", () => {
  const strongest = sense(["verb"]);
  const next = sense(["noun"]);
  const unsupported = sense(["verb"]);
  const [section] = groupSenseSections([strongest, next, unsupported], 0, false);

  assert.deepEqual(section.items, [strongest, next, unsupported]);
  assert.deepEqual(groupShape(section.groups), [
    [1, 0, []],
    [1, 1, []],
    [1, 2, []],
  ]);
});

test("every component that renders dictionary sense rows uses the shared grouping rule", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const consumers = readdirSync(here)
    .filter((name) => name.endsWith(".svelte"))
    .filter((name) => readFileSync(join(here, name), "utf8").includes("<SenseRow"))
    .sort();

  assert.deepEqual(consumers, ["WordBody.svelte"]);
  for (const name of consumers) {
    const source = readFileSync(join(here, name), "utf8");
    assert.match(source, /groupSenseSections/);
    assert.match(source, /SenseGroupHeading/);
  }
});
