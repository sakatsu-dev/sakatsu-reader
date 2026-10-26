import assert from "node:assert/strict";
import test from "node:test";

import { cancelSourceDraft, compactSource, draftSource } from "./source-state.js";

test("a successful analysis becomes the compact canonical source", () => {
  assert.deepEqual(compactSource("気持ちが伝わる。"), {
    kind: "compact",
    canonical: "気持ちが伝わる。",
  });
});

test("editing remembers the successful passage while the draft changes", () => {
  assert.deepEqual(draftSource("前の文。"), { kind: "draft", previous: "前の文。" });
});

test("Escape restores the previous canonical source", () => {
  assert.deepEqual(cancelSourceDraft(draftSource("前の文。")), {
    text: "前の文。",
    source: compactSource("前の文。"),
  });
});

test("a first-run draft has nothing to cancel back to", () => {
  assert.equal(cancelSourceDraft(draftSource()), null);
});
