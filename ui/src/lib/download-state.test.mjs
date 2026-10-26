import assert from "node:assert/strict";
import { test } from "node:test";
import { hasSavedPartial, savedPhrase } from "./download-state.js";

const mb = (bytes) => `${Math.round(bytes / 1_048_576)} MB`;

test("a saved partial says how much of how much", () => {
  assert.equal(savedPhrase(40 * 1_048_576, 90 * 1_048_576, mb), "40 MB of 90 MB saved");
});

test("an unknown total is left out, not invented", () => {
  assert.equal(savedPhrase(40 * 1_048_576, 0, mb), "40 MB saved");
  assert.equal(savedPhrase(40 * 1_048_576, null, mb), "40 MB saved");
});

test("nothing saved says nothing", () => {
  assert.equal(savedPhrase(0, 90, mb), null);
});

test("saved bytes are offered only for an uninstalled component", () => {
  assert.equal(hasSavedPartial({ downloaded_bytes: 10, installed: false }), true);
  assert.equal(hasSavedPartial({ downloaded_bytes: 10, installed: true }), false);
  assert.equal(hasSavedPartial({ downloaded_bytes: 0, installed: false }), false);
  assert.equal(hasSavedPartial(undefined), false);
});

test("a saved Windows job remains resumable without a readable partial file", () => {
  const component = {
    downloaded_bytes: 0,
    installed: false,
    artifacts: [{ transfer_job: "saved-job" }],
  };
  assert.equal(hasSavedPartial(component), true);
  assert.equal(hasSavedPartial({ ...component, installed: true }), false);
  assert.equal(hasSavedPartial({ ...component, artifacts: [{ transfer_job: " " }] }), false);
  assert.equal(savedPhrase(0, 90, mb), null, "do not invent a saved byte count");
});
