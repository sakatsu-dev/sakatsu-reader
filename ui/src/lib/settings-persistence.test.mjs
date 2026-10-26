import assert from "node:assert/strict";
import test from "node:test";
import { settingsPersistence } from "./settings-persistence.js";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("writes stay ordered and only the newest response may normalize the interface", async () => {
  let value = "A";
  const calls = [];
  const applied = [];
  const first = deferred();
  const second = deferred();
  const controller = settingsPersistence({
    read: () => value,
    signature: () => value,
    save: (settings) => {
      calls.push(settings);
      return calls.length === 1 ? first.promise : second.promise;
    },
    apply: (kept) => applied.push(kept),
    failed: () => {},
    restart: async () => {},
  });
  controller.loaded("disk");
  const a = controller.persist("A");
  value = "B";
  const b = controller.persist("B");
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(calls, ["A"], "B waits for A rather than racing it");
  first.resolve("normalized-A");
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(calls, ["A", "B"]);
  second.resolve("normalized-B");
  await Promise.all([a, b]);
  assert.deepEqual(applied, ["normalized-B"]);
});

test("restart waits for the newest save and retries a failed current write", async () => {
  let value = "current";
  const first = deferred();
  const events = [];
  let saves = 0;
  const controller = settingsPersistence({
    read: () => value,
    signature: () => value,
    save: async () => {
      saves += 1;
      if (saves === 1) return first.promise;
      events.push("retry");
      return value;
    },
    apply: () => {},
    failed: () => {},
    restart: async () => events.push("restart"),
  });
  controller.loaded("old");
  const save = controller.persist("current");
  const restart = controller.restart();
  first.reject({ code: "settings_read_only", message: "read only" });
  await assert.rejects(save);
  await restart;
  assert.deepEqual(events, ["retry", "restart"]);
});

test("dispose prevents late normalization and clears scheduled work", async () => {
  const response = deferred();
  const applied = [];
  const controller = settingsPersistence({
    read: () => "current",
    signature: () => "current",
    save: () => response.promise,
    apply: (kept) => applied.push(kept),
    failed: () => {},
    restart: async () => {},
    delay: 0,
  });
  controller.loaded("old");
  const save = controller.persist("current");
  controller.dispose();
  response.resolve("late");
  await save;
  assert.deepEqual(applied, []);
});

test("before-exit saves pending settings and refuses installation when saving fails", async () => {
  let installed = false;
  const controller = settingsPersistence({
    read: () => "changed",
    signature: () => "changed",
    save: async () => {
      throw new Error("disk unavailable");
    },
    apply: () => {},
    failed: () => {},
    restart: async () => {},
  });
  controller.loaded("old");
  await assert.rejects(
    controller.beforeExit(async () => {
      installed = true;
    }),
    /disk unavailable/,
  );
  assert.equal(installed, false);
});
