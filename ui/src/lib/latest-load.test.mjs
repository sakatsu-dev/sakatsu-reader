import assert from "node:assert/strict";
import test from "node:test";

import { latestLoad } from "./latest-load.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("an older resource response cannot replace a newer one", async () => {
  const first = deferred();
  const second = deferred();
  const pending = [first, second];
  const accepted = [];
  const loader = latestLoad(() => pending.shift().promise, (value) => accepted.push(value));

  const a = loader.run();
  const b = loader.run();
  second.resolve("new dictionaries");
  await b;
  first.resolve("old dictionaries");
  await a;

  assert.deepEqual(accepted, ["new dictionaries"]);
});

test("an obsolete rejection cannot replace the current result", async () => {
  const first = deferred();
  const second = deferred();
  const pending = [first, second];
  const accepted = [];
  const rejected = [];
  const loader = latestLoad(
    () => pending.shift().promise,
    (value) => accepted.push(value),
    (error) => rejected.push(error.message),
  );

  const a = loader.run();
  const b = loader.run();
  second.resolve("current");
  await b;
  first.reject(new Error("obsolete"));
  await a;

  assert.deepEqual(accepted, ["current"]);
  assert.deepEqual(rejected, []);
});
