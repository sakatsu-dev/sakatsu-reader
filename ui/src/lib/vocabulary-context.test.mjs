import assert from "node:assert/strict";
import test from "node:test";

import { vocabularyPresentation } from "./vocabulary-context.js";

const ranking = (scores, favoured, reasons = []) => ({
  senses: scores.map((score, original_index) => ({
    original_index,
    score,
    reasons: original_index === favoured ? reasons : [],
  })),
  favoured,
});

test("a unique positive favourite is the one authoritative leading sense", () => {
  assert.deepEqual(vocabularyPresentation(3, ranking([4, 1, 0], 0, ["nearby evidence"])), {
    leadCount: 1,
    favourite: 0,
    evidence: ["nearby evidence"],
    ambiguous: false,
  });
});

test("ties and all-zero rankings lead with two neutral source-order senses", () => {
  assert.deepEqual(vocabularyPresentation(3, ranking([2, 2, 0], null)), {
    leadCount: 2, favourite: null, evidence: [], ambiguous: true,
  });
  assert.deepEqual(vocabularyPresentation(3, ranking([0, 0, 0], null)), {
    leadCount: 2, favourite: null, evidence: [], ambiguous: true,
  });
});

test("single-sense words do not claim ambiguity when the scorer abstains", () => {
  assert.deepEqual(vocabularyPresentation(1, ranking([0], null)), {
    leadCount: 1, favourite: null, evidence: [], ambiguous: false,
  });
});

test("missing rankings retain the ordinary one-sense balanced presentation", () => {
  assert.deepEqual(vocabularyPresentation(4, null), {
    leadCount: 1, favourite: null, evidence: [], ambiguous: false,
  });
  assert.deepEqual(vocabularyPresentation(0, undefined), {
    leadCount: 0, favourite: null, evidence: [], ambiguous: false,
  });
});

test("malformed or non-positive favourites are treated as abstentions", () => {
  assert.equal(vocabularyPresentation(2, ranking([3, 0], 7)).ambiguous, true);
  assert.equal(vocabularyPresentation(2, ranking([0, 3], 1)).ambiguous, true);
  assert.equal(vocabularyPresentation(2, ranking([0, 0], 0)).ambiguous, true);
});
