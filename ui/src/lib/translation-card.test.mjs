import assert from "node:assert/strict";
import test from "node:test";

import {
  alignedSentenceFlow,
  comparisonFollowup,
  contextCheckEligible,
} from "./translation-card.js";
import { latestRequest } from "./latest-request.js";

const sentences = [
  { first_token: 0, last_token_end: 4, japanese: "前の文です。", english: "This comes first." },
  {
    first_token: 4,
    last_token_end: 9,
    japanese: "気持ちが伝わります。",
    english: "My feelings come through.",
  },
];
const comparison = {
  japanese: { full: "気持ちが伝わります。", runs: ["気持ち"], without: "が伝わります。" },
  full: "My feelings come through.",
  without: "They come through.",
  pieces: [{ text: "My feelings", gone: true }, { text: "come through.", gone: false }],
  result_says_more: true,
};

test("aligned flow preserves every sentence exactly once and in source order", () => {
  const flow = alignedSentenceFlow(sentences);
  assert.deepEqual(flow.map(({ sentence }) => sentence), sentences);
  assert.equal(flow.filter(({ active }) => active).length, 0);
});

test("only the active sentence is transformed by an exact matching comparison", () => {
  const flow = alignedSentenceFlow(sentences, {
    comparing: true,
    activeToken: 5,
    comparison,
  });
  assert.equal(flow[0].comparison, null);
  assert.equal(flow[1].comparison, comparison);
  assert.equal(flow.filter(({ comparison: result }) => result).length, 1);
});

test("a stale comparison cannot replace a sentence with different Japanese", () => {
  const stale = { ...comparison, japanese: { ...comparison.japanese, full: "別の文。" } };
  const [first, second] = alignedSentenceFlow(sentences, {
    comparing: true,
    activeToken: 5,
    comparison: stale,
    comparisonBusy: true,
    comparisonError: "retry locally",
  });
  assert.equal(first.active, false);
  assert.equal(second.active, true);
  assert.equal(second.comparison, null);
  assert.equal(second.comparisonBusy, true);
  assert.equal(second.comparisonError, "retry locally");
});

test("context check is offered only for more than one canonical sentence", () => {
  assert.equal(contextCheckEligible(sentences.slice(0, 1)), false);
  assert.equal(contextCheckEligible(sentences), true);
});

test("all-struck and meaningful ablation outputs are distinct", () => {
  assert.deepEqual(comparisonFollowup({ ...comparison, without: "" }), { kind: "all-struck" });
  assert.deepEqual(comparisonFollowup(comparison), {
    kind: "result",
    text: "They come through.",
  });
  assert.deepEqual(comparisonFollowup({ ...comparison, result_says_more: false }), {
    kind: "baseline-only",
  });
});

test("stale aligned and context responses can both be rejected", () => {
  for (const requests of [latestRequest(), latestRequest()]) {
    const previousPassage = requests.begin();
    const currentPassage = requests.begin();
    assert.equal(requests.isCurrent(previousPassage), false);
    assert.equal(requests.isCurrent(currentPassage), true);
    requests.cancel();
    assert.equal(requests.isCurrent(currentPassage), false);
  }
});
