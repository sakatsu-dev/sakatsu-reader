import assert from "node:assert/strict";
import test from "node:test";

import { orderGrammarBlocks } from "./grammar-order.js";

const step = (form, component = null) => ({ form, component });
const auxiliary = (pattern_id, component) => ({ pattern_id, component });
const match = (pattern_id, extra = {}) => ({ pattern_id, ...extra });
const sense = (id, score = 0) => ({ id, score });

const labels = (blocks) =>
  blocks.flatMap((block) => {
    switch (block.kind) {
      case "prioritized-readings":
        return block.readings.map((reading) => `${reading.role}:${reading.sense.id}`);
      case "steps":
        return block.steps.map((item, index) => `step:${block.start + index}:${item.form}`);
      case "patterns":
        return block.items.map((item) =>
          item.kind === "auxiliary"
            ? `auxiliary:${item.auxiliary.pattern_id}`
            : `match:${item.match.pattern_id}`,
        );
      case "other-readings":
        return block.senses.map((item) => `other:${item.id}`);
    }
  });

test("a simple conjugation keeps its established derivational order", () => {
  const blocks = orderGrammarBlocks(
    { steps: [step("causative-passive"), step("conjunctive")], auxiliaries: [] },
    [match("noni")],
    null,
  );

  assert.deepEqual(labels(blocks), [
    "step:1:causative-passive",
    "step:2:conjunctive",
    "match:noni",
  ]);
});

test("伝わってきます places the compound before the auxiliary component's inflection", () => {
  const blocks = orderGrammarBlocks(
    {
      steps: [step("conjunctive", 0), step("non-past", 1)],
      auxiliaries: [auxiliary("te-kuru", 1)],
    },
    [],
    null,
  );

  assert.deepEqual(labels(blocks), [
    "step:1:conjunctive",
    "auxiliary:te-kuru",
    "step:2:non-past",
  ]);
  assert.deepEqual(
    blocks.filter((block) => block.kind === "steps").map((block) => block.start),
    [1, 2],
    "split ordered lists continue their visible and accessible numbering",
  );
});

test("chained auxiliaries are inserted at their own component boundaries", () => {
  const blocks = orderGrammarBlocks(
    {
      steps: [step("conjunctive", 0), step("conjunctive", 1), step("past", 2)],
      auxiliaries: [auxiliary("te-miru", 1), auxiliary("te-iru", 2)],
    },
    [],
    null,
  );

  assert.deepEqual(labels(blocks), [
    "step:1:conjunctive",
    "auxiliary:te-miru",
    "step:2:conjunctive",
    "auxiliary:te-iru",
    "step:3:past",
  ]);
});

test("one supported reading retains the established promoted presentation", () => {
  const blocks = orderGrammarBlocks(
    { steps: [], auxiliaries: [] },
    [match("particle-de")],
    { senses: [sense("means", 3), sense("place")], favoured: 0 },
  );

  assert.deepEqual(labels(blocks), ["favoured:means", "match:particle-de", "other:place"]);
  assert.equal(blocks.at(-1).ambiguous, false);
});

test("every supported reading is promoted in rank order and zero-score alternatives follow", () => {
  const blocks = orderGrammarBlocks(
    { steps: [], auxiliaries: [] },
    [match("particle-ni")],
    {
      senses: [sense("recipient", 5), sense("existence", 2), sense("time")],
      favoured: 0,
    },
  );

  assert.deepEqual(labels(blocks), [
    "favoured:recipient",
    "supported:existence",
    "match:particle-ni",
    "other:time",
  ]);
  assert.deepEqual(
    blocks[0].readings.map((reading) => reading.role),
    ["favoured", "supported"],
    "the renderer receives the prefix/accent role instead of reinterpreting scores",
  );
});

test("a positive-score tie promotes nothing and keeps every reading after specific grammar", () => {
  const blocks = orderGrammarBlocks(
    { steps: [], auxiliaries: [] },
    [match("particle-de")],
    { senses: [sense("place", 3), sense("means", 3)], favoured: null },
  );

  assert.deepEqual(labels(blocks), ["match:particle-de", "other:place", "other:means"]);
  assert.equal(blocks.at(-1).ambiguous, true);
});

test("an all-zero result remains ambiguous and keeps authored order", () => {
  const blocks = orderGrammarBlocks(null, [], {
    senses: [sense("subject"), sense("object")],
    favoured: null,
  });

  assert.deepEqual(labels(blocks), ["other:subject", "other:object"]);
  assert.equal(blocks[0].ambiguous, true);
});

test("JLPT-like metadata cannot affect the dependency order", () => {
  const base = [match("first"), match("second")];
  const labelled = [match("first", { jlpt: "N1" }), match("second", { jlpt: "N5" })];

  assert.deepEqual(
    labels(orderGrammarBlocks(null, labelled, null)),
    labels(orderGrammarBlocks(null, base, null)),
  );
});

test("the ordering is lossless even when a component has no reported form", () => {
  const blocks = orderGrammarBlocks(
    {
      steps: [step("conjunctive", 0), step("past", 2)],
      auxiliaries: [auxiliary("te-miru", 1), auxiliary("te-iru", 2)],
    },
    [match("sentence-one"), match("sentence-two")],
    { senses: [sense("a", 5), sense("b", 2), sense("c")], favoured: 0 },
  );

  assert.deepEqual(labels(blocks), [
    "favoured:a",
    "supported:b",
    "step:1:conjunctive",
    "auxiliary:te-miru",
    "auxiliary:te-iru",
    "step:2:past",
    "match:sentence-one",
    "match:sentence-two",
    "other:c",
  ]);
});
