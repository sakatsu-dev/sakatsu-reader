import assert from "node:assert/strict";
import test from "node:test";

import { listenerGroup } from "./listener-group.js";

function deferred() {
  let resolve;
  const promise = new Promise((yes) => (resolve = yes));
  return { promise, resolve };
}

test("disposing removes every listener already owned", async () => {
  const removed = [];
  const listeners = listenerGroup();
  await listeners.add(async () => () => removed.push("first"));
  await listeners.add(async () => () => removed.push("second"));

  listeners.dispose();

  assert.deepEqual(removed, ["first", "second"]);
});

test("a subscription that resolves after disposal immediately removes itself", async () => {
  const pending = deferred();
  let removed = 0;
  const listeners = listenerGroup();
  const adding = listeners.add(() => pending.promise);

  listeners.dispose();
  pending.resolve(() => (removed += 1));

  assert.equal(await adding, false);
  assert.equal(removed, 1);
});

test("a new generation can subscribe after the old one is disposed", async () => {
  let removed = 0;
  const listeners = listenerGroup();
  listeners.dispose();

  assert.equal(await listeners.add(async () => () => (removed += 1)), true);
  assert.equal(removed, 0);
  listeners.dispose();
  assert.equal(removed, 1);
});
