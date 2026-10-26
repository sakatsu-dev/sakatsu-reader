import assert from "node:assert/strict";
import test from "node:test";

import { sharedOperation } from "./shared-operation.js";

/** A promise this test settles by hand, so overlap is exact rather than timed. */
function deferred() {
  let settle;
  let fail;
  const promise = new Promise((resolve, reject) => {
    settle = resolve;
    fail = reject;
  });
  return { promise, settle, fail };
}

test("a second caller joins the run already going instead of starting another", async () => {
  const gate = deferred();
  let starts = 0;
  const operation = sharedOperation();
  const start = () => {
    starts += 1;
    return gate.promise;
  };

  const first = operation.run(start);
  const second = operation.run(start);
  assert.equal(starts, 1, "the work is started once");
  assert.equal(first, second, "and both callers hold the same promise");

  gate.settle("built");
  assert.equal(await first, "built");
  assert.equal(await second, "built", "the joiner gets the same answer, not a stale one");
});

test("a failure reaches every caller and does not outlive the operation", async () => {
  const gate = deferred();
  const operation = sharedOperation();
  const first = operation.run(() => gate.promise);
  const second = operation.run(() => gate.promise);

  gate.fail(new Error("another resource operation is using the dictionaries"));
  await assert.rejects(first, /another resource operation/);
  await assert.rejects(second, /another resource operation/);

  // **The slot is cleared by a failure too.** Left occupied, every later attempt would return
  // this same rejection — the refusal would become permanent, and retrying would be impossible.
  assert.equal(operation.busy, false);
  const after = operation.run(async () => "built");
  assert.equal(await after, "built", "a later attempt runs afresh");
});

test("an operation that has settled does not capture the next one", async () => {
  const operation = sharedOperation();
  assert.equal(await operation.run(async () => "first"), "first");
  assert.equal(operation.busy, false);
  assert.equal(await operation.run(async () => "second"), "second");
});

test("busy reports the run, not the request", async () => {
  const gate = deferred();
  const operation = sharedOperation();
  assert.equal(operation.busy, false, "nothing is running yet");
  const running = operation.run(() => gate.promise);
  assert.equal(operation.busy, true);
  gate.settle("done");
  await running;
  assert.equal(operation.busy, false);
});
