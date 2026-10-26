import assert from "node:assert/strict";
import test from "node:test";

import {
  isPersistenceError,
  errorMessage,
  saveFailure,
  SETTINGS_NOT_SAVED,
  STATUS_NOT_SAVED,
} from "./save-notice.js";

const SENTENCE =
  "Nothing can be saved this session: Cannot create a file when that file already exists. (os error 183)";

/** A rejection as `PersistenceError` crosses the IPC boundary: plain JSON, no class. */
const rejection = (code, message = SENTENCE) => ({ code, message });

test("a failure for the standing reason is answered briefly, not repeated", () => {
  const outcome = saveFailure(rejection("store_unavailable"), STATUS_NOT_SAVED);

  assert.equal(outcome.notice, STATUS_NOT_SAVED);
  assert.equal(outcome.error, null, "the sentence is already on screen once");
});

/**
 * The distinction the typed code exists to draw.
 *
 * `settings_read_only` means the store opened and declined this one file — a settings file from a
 * newer build, or one whose migration backup could not be written. Nothing on screen says that,
 * so answering it with "Settings weren't saved" would hide the only account of it.
 */
test("a store that opened but refused this write keeps its own message", () => {
  const refusal =
    "the settings file was written by a newer version of this app, so saving would discard settings this build cannot see";
  const outcome = saveFailure(rejection("settings_read_only", refusal), SETTINGS_NOT_SAVED);

  assert.equal(outcome.notice, null);
  assert.equal(outcome.error, refusal);
});

test("a write that went wrong keeps its own message too", () => {
  const outcome = saveFailure(rejection("write_failed", "Access is denied. (os error 5)"), STATUS_NOT_SAVED);

  assert.equal(outcome.notice, null);
  assert.equal(outcome.error, "Access is denied. (os error 5)");
});

test("an unrelated rejection is shown in full, whatever shape it is", () => {
  // Any other command, or a transport failure: not a PersistenceError, so not this function's
  // business beyond passing it through legibly.
  const outcome = saveFailure("the deck could not be reached", STATUS_NOT_SAVED);

  assert.equal(outcome.notice, null);
  assert.equal(outcome.error, "the deck could not be reached");
});

/**
 * The regression the typed error replaced, kept as a test.
 *
 * The first version compared the rejection text against the standing warning with `===`. That
 * made behaviour depend on prose — reword the sentence in Rust and the notice silently became a
 * full repeat. A code cannot be reworded by accident.
 */
test("the message is not consulted for the decision", () => {
  const reworded = rejection("store_unavailable", "Persistence is unavailable for this session.");
  const outcome = saveFailure(reworded, STATUS_NOT_SAVED);

  assert.equal(outcome.notice, STATUS_NOT_SAVED, "the code decided, not the wording");
});

/** And the converse: the standing sentence with the wrong code is not treated as the standing case. */
test("the standing sentence under another code is not answered briefly", () => {
  const outcome = saveFailure(rejection("write_failed", SENTENCE), STATUS_NOT_SAVED);

  assert.equal(outcome.notice, null);
  assert.equal(outcome.error, SENTENCE);
});

test("a typed rejection never reaches the screen as [object Object]", () => {
  const outcome = saveFailure(rejection("write_failed", "disk full"), STATUS_NOT_SAVED);
  assert.equal(outcome.error, "disk full");
  assert.notEqual(outcome.error, String(rejection("write_failed", "disk full")));
});

test("a typed settings rejection remains readable when another action prefixes it", () => {
  const thrown = rejection("settings_read_only", "this settings file belongs to a newer build");
  const shown = `Could not restart Sakatsu Reader: ${errorMessage(thrown)}`;

  assert.equal(
    shown,
    "Could not restart Sakatsu Reader: this settings file belongs to a newer build",
  );
  assert.ok(!shown.includes("[object Object]"));
});

test("repeated failures produce one message, because there is one slot", () => {
  const notices = [0, 1, 2].map(
    () => saveFailure(rejection("store_unavailable"), STATUS_NOT_SAVED).notice,
  );

  assert.deepEqual(notices, [STATUS_NOT_SAVED, STATUS_NOT_SAVED, STATUS_NOT_SAVED]);
  assert.equal(new Set(notices).size, 1, "identical, so assigning one slot collapses them");
});

test("the two actions say which one failed", () => {
  assert.notEqual(STATUS_NOT_SAVED, SETTINGS_NOT_SAVED);
  for (const said of [STATUS_NOT_SAVED, SETTINGS_NOT_SAVED]) {
    assert.ok(said.length < 40, `${said} is meant to be a line, not an explanation`);
    assert.ok(!said.includes(":"), `${said} should not read as a prefix onto something`);
  }
});

test("only the structured shape counts as one", () => {
  assert.ok(isPersistenceError({ code: "write_failed", message: "x" }));
  assert.ok(!isPersistenceError(null));
  assert.ok(!isPersistenceError("a string"));
  assert.ok(!isPersistenceError({ code: "write_failed" }), "a message is required");
  assert.ok(!isPersistenceError({ message: "x" }), "a code is required");
  assert.ok(!isPersistenceError({ code: 1, message: "x" }), "the code must be a string");
});
