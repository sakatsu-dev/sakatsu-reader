import assert from "node:assert/strict";
import test from "node:test";
import { requestOwner } from "./request-owner.js";

test("operations and owners have independent monotonic identities", () => {
  const owner = requestOwner("window-a", async () => {});
  assert.deepEqual(owner.begin("study"), {
    owner: "window-a",
    operation: "study",
    generation: 1,
    priority: "foreground",
  });
  assert.equal(owner.begin("translation").generation, 1);
  assert.equal(owner.begin("study").generation, 2);
});

test("replacement, clear and disposal withdraw only work this owner started", async () => {
  const withdrawn = [];
  const owner = requestOwner("window", async (request) => withdrawn.push(request));
  owner.begin("study");
  const translation = owner.begin("translation");
  owner.begin("study");
  owner.cancel("translation");
  owner.dispose();
  await Promise.resolve();

  assert.deepEqual(
    withdrawn.map(({ operation, generation }) => [operation, generation]),
    [
      ["study", 1],
      ["translation", 1],
      ["study", 2],
    ],
  );
  assert.equal(translation.operation, "translation");
});

test("completion cannot clear a replacement", async () => {
  const withdrawn = [];
  const owner = requestOwner("window", async (request) => withdrawn.push(request));
  const old = owner.begin("study");
  const current = owner.begin("study");
  owner.complete(old);
  owner.dispose();
  await Promise.resolve();
  assert.deepEqual(withdrawn, [old, current]);
});
