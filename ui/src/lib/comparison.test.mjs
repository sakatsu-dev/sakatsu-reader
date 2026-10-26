import assert from "node:assert/strict";
import test from "node:test";

import { toggleComparisonSelection } from "./comparison.js";

const sentences = [
  { first_token: 0, last_token_end: 3 },
  { first_token: 3, last_token_end: 6 },
];

test("adding in another sentence starts a new comparison there", () => {
  assert.deepEqual(
    toggleComparisonSelection(new Set([0, 2]), 4, sentences),
    new Set([4]),
  );
});

test("adding and removing within one sentence preserves the other words", () => {
  const original = new Set([0, 2]);
  assert.deepEqual(toggleComparisonSelection(original, 1, sentences), new Set([0, 1, 2]));
  assert.deepEqual(toggleComparisonSelection(original, 2, sentences), new Set([0]));
  assert.deepEqual(original, new Set([0, 2]), "the transition never mutates its input");
});

test("every transition closes over the clicked word's sentence", () => {
  // Exhaust every possible starting set, including illegal cross-sentence sets. The result of an
  // add is always a valid one-sentence state; this is the invariant the Rust builder defends too.
  for (let mask = 0; mask < 1 << 6; mask += 1) {
    const starting = new Set(
      Array.from({ length: 6 }, (_, index) => index).filter((index) => mask & (1 << index)),
    );
    for (let clicked = 0; clicked < 6; clicked += 1) {
      if (starting.has(clicked)) continue;
      const result = toggleComparisonSelection(starting, clicked, sentences);
      const range = sentences.find(
        (sentence) => clicked >= sentence.first_token && clicked < sentence.last_token_end,
      );
      assert.ok(range);
      assert.ok(result.has(clicked));
      assert.ok(
        [...result].every(
          (index) => index >= range.first_token && index < range.last_token_end,
        ),
        `mask ${mask.toString(2)}, clicked ${clicked}`,
      );
    }
  }
});

test("an index outside the supplied partition leaves state alone", () => {
  const original = new Set([1]);
  assert.deepEqual(toggleComparisonSelection(original, 99, sentences), original);
  assert.notEqual(toggleComparisonSelection(original, 99, sentences), original);
});
