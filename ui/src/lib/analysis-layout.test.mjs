import assert from "node:assert/strict";
import test from "node:test";

import { fitPassageRatio, isConstrainedStudy, passageFitContent } from "./analysis-layout.js";

test("one line fits to its measured content instead of reserving the old percentage", () => {
  assert.equal(fitPassageRatio({ workspace: 800, content: 96, floor: 72 }), 0.12);
});

test("long content stops at forty percent", () => {
  assert.equal(fitPassageRatio({ workspace: 800, content: 900, floor: 72 }), 0.4);
});

test("the reading floor outranks the percentage cap", () => {
  assert.equal(fitPassageRatio({ workspace: 200, content: 400, floor: 110 }), 0.55);
});

test("the reading floor also outranks the ordinary maximum in a tiny workspace", () => {
  assert.equal(fitPassageRatio({ workspace: 100, content: 400, floor: 90 }), 0.9);
  assert.equal(fitPassageRatio({ workspace: 60, content: 400, floor: 90 }), 1);
});

test("translation never changes the Japanese pane's fitted ratio", () => {
  const japanese = 90;
  const beforeTranslation = fitPassageRatio({
    workspace: 600,
    content: passageFitContent({ japanese, translation: 0 }),
    floor: 72,
  });
  const afterTranslation = fitPassageRatio({
    workspace: 600,
    content: passageFitContent({ japanese, translation: 700 }),
    floor: 72,
  });
  assert.equal(beforeTranslation, 0.15);
  assert.equal(afterTranslation, beforeTranslation);
});

test("unlaid geometry cannot publish NaN", () => {
  assert.equal(fitPassageRatio({ workspace: 0, content: Number.NaN, floor: Number.NaN }), 0);
});

test("the responsive boundary is based on usable study width", () => {
  assert.equal(isConstrainedStudy(629, 15), true);
  assert.equal(isConstrainedStudy(630, 15), false);
});
