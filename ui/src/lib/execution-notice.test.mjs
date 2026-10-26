import assert from "node:assert/strict";
import test from "node:test";

import { noticeFor } from "./execution-notice.js";

const record = (requested, fallback) => ({ requested, engine: fallback ? "lindera" : "ichiran", fallback });
const fallback = (reason, scope) => ({ reason, scope, detail: `technical: ${reason}` });

test("an analysis Ichiran produced says nothing", () => {
  assert.equal(noticeFor(record("automatic", null)), null);
  assert.equal(noticeFor(null), null, "an unknown execution says nothing either");
  assert.equal(noticeFor(undefined), null);
});

test("a passage Ichiran failed on is announced under every policy", () => {
  for (const requested of ["automatic", "ichiran"]) {
    for (const reason of ["unavailable", "timed_out", "malformed_output", "invalid_spans"]) {
      const notice = noticeFor(record(requested, fallback(reason, "passage")));
      assert.equal(notice?.tone, "prominent", `${requested} ${reason}`);
      assert.equal(notice?.text, "Using Lindera because Ichiran could not analyze this passage.");
      assert.equal(notice?.detail, `technical: ${reason}`, "the detail is kept, not shown instead");
    }
  }
});

test("Ichiran still starting says nothing, under either policy", () => {
  for (const requested of ["automatic", "ichiran"]) {
    assert.equal(noticeFor(record(requested, fallback("starting", "session"))), null, requested);
  }
});

test("no Ichiran this session is loud when it was asked for by name and quiet under Automatic", () => {
  assert.equal(noticeFor(record("ichiran", fallback("unavailable", "session")))?.tone, "prominent");
  const quiet = noticeFor(record("automatic", fallback("unavailable", "session")));
  assert.equal(quiet?.tone, "quiet", "provenance is still shown, just quietly");
  assert.equal(quiet?.text, "Built-in analyzer");
});
