import assert from "node:assert/strict";
import test from "node:test";

import { latestRequest } from "./latest-request.js";

test("only the newest request may update its surface", () => {
  const requests = latestRequest();
  const first = requests.begin();
  const second = requests.begin();

  assert.equal(requests.isCurrent(first), false);
  assert.equal(requests.isCurrent(second), true);
});

test("cancelling invalidates work without starting a replacement", () => {
  const requests = latestRequest();
  const running = requests.begin();
  requests.cancel();

  assert.equal(requests.isCurrent(running), false);
});
