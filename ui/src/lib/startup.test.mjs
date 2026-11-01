import test from "node:test";
import assert from "node:assert/strict";
import { startupRows, startupPolling } from "./startup.js";

const engine = (role, state = "ready", loaded = role === "analysis" ? "ichiran" : "auto") => ({
  role,
  state,
  loaded,
  configured: role === "analysis" ? "automatic" : "auto",
});
const ready = {
  settings: true,
  resources: true,
  needsSetup: false,
  engines: [engine("analysis"), engine("recognition")],
};

test("progress counts actual terminal checks and never estimates elapsed time", () => {
  const rows = startupRows({
    ...ready,
    engines: [engine("analysis", "loading", null), engine("recognition")],
  });
  assert.equal(rows.filter((row) => row.state !== "loading").length, 3);
  assert.equal(rows.find((row) => row.id === "analysis").state, "loading");
});
test("deferred Ichiran and missing OCR are not described as initialized", () => {
  const rows = startupRows({
    ...ready,
    engines: [engine("analysis", "not_loaded", null), engine("recognition", "not_installed", null)],
  });
  assert.equal(rows[2].state, "deferred");
  assert.equal(rows[3].state, "attention");
});
test("setup-required startup can reach setup without waiting for missing services", () => {
  const rows = startupRows({ ...ready, needsSetup: true, engines: null });
  assert.equal(rows.filter((row) => row.state === "loading").length, 0);
  assert.equal(rows[1].state, "attention");
  assert.equal(rows[2].state, "deferred");
});
test("unavailable status and failed settings are separate from successful emptiness", () => {
  const rows = startupRows({
    ...ready,
    settingsFailed: true,
    resourcesFailed: true,
    engines: null,
    error: "offline",
  });
  assert.ok(rows.every((row) => row.state === "attention"));
});
test("built-in fallback is named rather than credited to Ichiran", () => {
  const rows = startupRows({
    ...ready,
    engines: [engine("analysis", "ready", "lindera"), engine("recognition")],
  });
  assert.equal(rows[2].detail, "Built-in fallback ready");
});
test("an unfamiliar engine status never claims readiness", () => {
  const rows = startupRows({
    ...ready,
    engines: [engine("analysis", "unrecognized", null), engine("recognition")],
  });
  assert.equal(rows[2].state, "attention");
});
test("a dismissed screen accepts no late result and schedules no next request", async () => {
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  let published = 0;
  let scheduled = 0;
  const poller = startupPolling(
    () => pending,
    () => published++,
    {
      schedule: () => {
        scheduled++;
        return 1;
      },
      cancel: () => {},
    },
  );
  const request = poller.refresh();
  poller.dispose();
  release({ engines: [] });
  await request;
  assert.equal(published, 0);
  assert.equal(scheduled, 0);
});
test("polls never overlap and failed status can be retried", async () => {
  let fail = true;
  let calls = 0;
  let scheduled = 0;
  const replies = [];
  const poller = startupPolling(
    async () => {
      calls++;
      if (fail) throw new Error("offline");
      return { engines: [] };
    },
    (status, error) => replies.push({ status, error }),
    {
      schedule: () => {
        scheduled++;
        return 1;
      },
      cancel: () => {},
    },
  );
  const first = poller.refresh();
  await poller.refresh();
  assert.equal(calls, 1);
  await first;
  assert.ok(replies[0].error);
  assert.equal(scheduled, 0, "failed checks wait for an explicit retry");
  fail = false;
  await poller.refresh();
  assert.equal(replies[1].error, "");
  assert.equal(scheduled, 1);
  poller.dispose();
});
