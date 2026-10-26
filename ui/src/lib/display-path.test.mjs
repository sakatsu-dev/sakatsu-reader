import assert from "node:assert/strict";
import test from "node:test";

import { displayPath } from "./display-path.js";

test("hides a local Windows device-path prefix", () => {
  assert.equal(displayPath("\\\\?\\C:\\reader\\resources"), "C:\\reader\\resources");
});

test("renders a device UNC path as an ordinary UNC path", () => {
  assert.equal(
    displayPath("\\\\?\\UNC\\server\\share\\resources"),
    "\\\\server\\share\\resources",
  );
});

test("leaves ordinary and non-Windows paths unchanged", () => {
  assert.equal(displayPath("C:\\reader\\resources"), "C:\\reader\\resources");
  assert.equal(displayPath("/opt/reader/resources"), "/opt/reader/resources");
});
