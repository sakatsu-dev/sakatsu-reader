import assert from "node:assert/strict";
import test from "node:test";

import { delayedWarmup, neighboringWordIndices } from "./intent-warmup.js";

test("nearby analysis words are forward-first, bounded and exclude the selection", () => {
  assert.deepEqual(neighboringWordIndices([0, 2, 4, 7, 9], 4), [7, 2, 9, 0]);
  assert.deepEqual(neighboringWordIndices([0, 2, 4], 0), [2, 4]);
  assert.deepEqual(neighboringWordIndices([0, 2, 4], 9), []);
});

test("a newer delayed intent replaces work that has not started", async () => {
  const prepared = [];
  const warmup = delayedWarmup(10, (value) => prepared.push(value));
  warmup.schedule("食");
  warmup.schedule("食べる");
  await new Promise((resolve) => setTimeout(resolve, 25));
  assert.deepEqual(prepared, ["食べる"]);
});

test("cancel leaves no delayed work behind", async () => {
  const prepared = [];
  const warmup = delayedWarmup(5, (value) => prepared.push(value));
  warmup.schedule("水");
  warmup.cancel();
  await new Promise((resolve) => setTimeout(resolve, 15));
  assert.deepEqual(prepared, []);
});
