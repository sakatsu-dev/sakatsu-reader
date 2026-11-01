import assert from "node:assert/strict";
import { test } from "node:test";

import { captureHint } from "./capture-hint.js";

/** The ordinary case: middle click bound, PaddleOCR-VL chosen, animation on. */
const state = {
  capturing: false,
  ready: true,
  engine: "PaddleOCR-VL",
  binding: "Middle click",
  cycling: true,
};

/**
 * The distinction the whole thing exists for.
 *
 * A cold PaddleOCR-VL read spends its first seconds starting a service. Before this, those
 * seconds and the inference that followed them both said "Reading…", so a reader could not tell
 * a model waking up from a capture that had gone wrong.
 */
test("a capture that is still loading says so, and names the engine", () => {
  const loading = captureHint({ ...state, capturing: true, ready: false });
  assert.equal(loading.phase, "loading");
  assert.equal(loading.text, "Loading PaddleOCR-VL…");
  assert.equal(loading.spinner, true);

  const reading = captureHint({ ...state, capturing: true, ready: true });
  assert.equal(reading.phase, "reading");
  assert.equal(reading.text, "Reading…");
  assert.equal(reading.spinner, true);
});

test("idle alternates the binding with the engine", () => {
  const idle = captureHint(state);
  assert.equal(idle.phase, "idle");
  assert.equal(idle.spinner, false);
  assert.deepEqual(idle.faces, ["Middle click reads", "PaddleOCR-VL"]);
});

/**
 * Turning animation off must lose the motion, not the answer.
 *
 * Dropping to just the binding would make the setting quietly remove information, and the engine
 * name is the reason this work happened.
 */
test("without motion the two faces become one line", () => {
  const still = captureHint({ ...state, cycling: false });
  assert.deepEqual(still.faces, [], "nothing to alternate");
  assert.equal(still.text, "Middle click reads · PaddleOCR-VL");
});

/**
 * `stub` is what Rust reports when the chosen engine's files are not on disk, and `engineLabel`
 * renders it "no model installed" — a phrase that reads correctly in the Settings row it was
 * written for and absurdly in a sentence: "Loading no model installed…".
 */
test("an engine with no name is never pasted into a sentence", () => {
  const loading = captureHint({
    ...state,
    capturing: true,
    ready: false,
    engine: "no model installed",
  });
  assert.equal(loading.text, "Loading…", "says less rather than saying nonsense");

  const idle = captureHint({ ...state, engine: "no model installed" });
  assert.deepEqual(idle.faces, [], "nothing worth alternating with");
  assert.equal(idle.text, "Middle click reads");
});

/** The same care for an engine name that has not arrived yet. */
test("an empty engine name behaves like a missing one", () => {
  assert.equal(captureHint({ ...state, capturing: true, ready: false, engine: "" }).text, "Loading…");
  assert.deepEqual(captureHint({ ...state, engine: "" }).faces, []);
});

/**
 * Readiness is only a question while a capture is running. An idle hint must not start
 * announcing a load nobody asked for.
 */
test("an idle hint ignores readiness", () => {
  const cold = captureHint({ ...state, ready: false });
  assert.equal(cold.phase, "idle");
  assert.equal(cold.spinner, false);
});

/** The binding is whatever the reader bound, not a hardcoded phrase. */
test("the binding is quoted as given", () => {
  const bound = captureHint({ ...state, binding: "Ctrl+Shift+Q" });
  assert.deepEqual(bound.faces, ["Ctrl+Shift+Q reads", "PaddleOCR-VL"]);
});
