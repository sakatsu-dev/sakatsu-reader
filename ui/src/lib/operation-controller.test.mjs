import assert from "node:assert/strict";
import test from "node:test";
import { operationController } from "./operation-controller.js";
import { requestOwner } from "./request-owner.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("B may finish before A and every late handler from A is inert", async () => {
  const requests = requestOwner("window", async () => {});
  const states = [];
  const surface = operationController({
    operation: "study",
    requests,
    changed: (state) => states.push({ ...state }),
  });
  const a = surface.begin();
  const b = surface.begin();
  const painted = [];

  assert.equal(surface.accept(b, () => painted.push("B")), true);
  assert.equal(surface.finish(b), true);
  assert.equal(surface.accept(a, () => painted.push("A")), false);
  assert.equal(surface.reject(a, new Error("old")), false);
  assert.equal(surface.finish(a), false);
  assert.deepEqual(painted, ["B"]);
  assert.deepEqual(surface.snapshot(), { busy: false, error: "", disposed: false });
  assert.equal(states.at(-1).busy, false);
});

test("clear and disposal reject pending success, rejection and finally", () => {
  const requests = requestOwner("window", async () => {});
  for (const dispose of [false, true]) {
    const surface = operationController({ operation: `capture-${dispose}`, requests });
    const pending = surface.begin();
    if (dispose) surface.dispose();
    else surface.cancel();
    assert.equal(surface.accept(pending, () => assert.fail("stale success")), false);
    assert.equal(surface.reject(pending, "stale error"), false);
    assert.equal(surface.finish(pending), false);
    assert.equal(surface.snapshot().busy, false);
  }
});

test("a rejected current request owns its error until replacement", () => {
  const requests = requestOwner("window", async () => {});
  const surface = operationController({ operation: "translation", requests });
  const first = surface.begin();
  surface.reject(first, new Error("failed"));
  surface.finish(first);
  assert.equal(surface.snapshot().error, "Error: failed");
  surface.begin();
  assert.equal(surface.snapshot().error, "");
});

test("controlled promises exercise completion in reverse order", async () => {
  const requests = requestOwner("window", async () => {});
  const surface = operationController({ operation: "reader", requests });
  const first = deferred();
  const second = deferred();
  const results = [];
  const run = async (work) => {
    const ticket = surface.begin();
    try {
      const value = await work.promise;
      surface.accept(ticket, () => results.push(value));
    } finally {
      surface.finish(ticket);
    }
  };
  const a = run(first);
  const b = run(second);
  second.resolve("B");
  await b;
  first.resolve("A");
  await a;
  assert.deepEqual(results, ["B"]);
});
