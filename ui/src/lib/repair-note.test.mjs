import assert from "node:assert/strict";
import test from "node:test";

import { repairNoteFor } from "./repair-note.js";

const execution = (...repairs) => ({ requested: "automatic", engine: "ichiran", fallback: null, repairs });
const note = { start: 15, end: 30, family: "te_oru", lemma: "間違う" };

test("the repaired word is attributed by its exact span", () => {
  assert.equal(
    repairNoteFor(execution(note), { start: 15, end: 30 }),
    "Contracted ～ておる form of 間違う, rejoined using the dictionary.",
  );
  assert.equal(
    repairNoteFor(execution({ ...note, family: "negative_conditional", lemma: "怒る" }), { start: 15, end: 30 }),
    "Contracted ～なければ form of 怒る, rejoined using the dictionary.",
  );
});

test("a part of the word, a neighbour, or an unplaced token is not attributed", () => {
  assert.equal(repairNoteFor(execution(note), { start: 15, end: 24 }), null, "the part 間違っ");
  assert.equal(repairNoteFor(execution(note), { start: 30, end: 36 }), null, "the next word");
  assert.equal(repairNoteFor(execution(note), { start: null, end: null }), null);
});

test("no record, or a record from before repairs existed, says nothing", () => {
  assert.equal(repairNoteFor(null, { start: 15, end: 30 }), null);
  assert.equal(repairNoteFor(undefined, { start: 15, end: 30 }), null);
  assert.equal(repairNoteFor({ requested: "automatic", engine: "ichiran", fallback: null }, { start: 15, end: 30 }), null);
  assert.equal(repairNoteFor(execution(), { start: 15, end: 30 }), null);
  assert.equal(repairNoteFor(execution(note), null), null);
});

test("an unknown family still names the lemma rather than a form it cannot describe", () => {
  assert.equal(
    repairNoteFor(execution({ ...note, family: "something_new" }), { start: 15, end: 30 }),
    "A form of 間違う, rejoined using the dictionary.",
  );
});
