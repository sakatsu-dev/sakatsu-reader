import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { verifyQaBoundary } from "../../scripts/qa-build-boundary.mjs";

test("QA build guard requires fixtures only in layout assets", async () => {
  const root = await mkdtemp(join(tmpdir(), "sakatsu-build-guard-"));
  try {
    await mkdir(join(root, "assets"));
    await assert.rejects(verifyQaBoundary(root, "layout"), /no JavaScript/);
    await writeFile(join(root, "assets", "app.js"), "console.log('reader');");
    await verifyQaBoundary(root, "production");
    await assert.rejects(verifyQaBoundary(root, "layout"), /missing QA/);
    await writeFile(join(root, "assets", "qa.js"), "window.__SAKATSU_QA__ = {};");
    await verifyQaBoundary(root, "layout");
    await assert.rejects(verifyQaBoundary(root, "production"), /leaked/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
