import assert from "node:assert/strict";
import { test } from "node:test";
import {
  accelerationPhrase,
  engineRow,
  isLoopback,
  resultPhrase,
  statusLabel,
} from "./runtime-status.js";

const label = (engine) =>
  ({ "manga-ocr": "manga-ocr", "pp-ocrv6": "PP-OCRv6", vlm: "PaddleOCR-VL", auto: "Auto" })[
    engine
  ] ?? engine;

const request = { owner: "w", operation: "capture", generation: 3, priority: "foreground" };

/** @returns {import("../model").OperationResult} */
function result(overrides = {}) {
  return {
    role: "recognition",
    request,
    engine: "manga-ocr",
    fallback: null,
    cache: "fresh",
    outcome: { kind: "succeeded" },
    acceleration: { kind: "cpu" },
    ...overrides,
  };
}

test("acceleration is said only as reported, and never guessed", () => {
  assert.equal(accelerationPhrase({ kind: "cpu" }), "CPU");
  assert.equal(
    accelerationPhrase({ kind: "gpu_layers", offloaded: 29, total: 29, device: null }),
    "GPU acceleration active (29/29 layers)",
  );
  assert.equal(accelerationPhrase({ kind: "unknown" }), "");
  assert.equal(accelerationPhrase({ kind: "not_running" }), "");
  assert.equal(accelerationPhrase(undefined), "");
});

test("an engine row says what is asked for, what is loaded, where and on what", () => {
  assert.deepEqual(
    engineRow(
      {
        role: "recognition",
        configured: "auto",
        loaded: "auto",
        hosting: { kind: "in_process" },
        state: "ready",
        acceleration: { kind: "cpu" },
      },
      label,
    ),
    { role: "Recognition", name: "Auto", detail: "ready · in the app · CPU" },
  );
  assert.deepEqual(
    engineRow(
      {
        role: "analysis",
        configured: "automatic",
        loaded: "lindera",
        hosting: { kind: "in_process" },
        state: "ready",
        acceleration: { kind: "cpu" },
      },
      label,
    ),
    { role: "Analysis", name: "automatic", detail: "ready, using lindera · in the app · CPU" },
  );
  assert.equal(
    engineRow(
      {
        role: "recognition",
        configured: "vlm",
        loaded: null,
        hosting: { kind: "endpoint", external: true },
        state: "not_loaded",
        acceleration: { kind: "unknown" },
      },
      label,
    ).detail,
    "not loaded yet · an external endpoint",
  );
});

test("a cached result never claims a device ran it", () => {
  assert.equal(
    resultPhrase(result({ role: "translation", engine: "fugumt", cache: "cached" }), label),
    "fugumt · from cache",
  );
  assert.equal(
    resultPhrase(result({ role: "translation", engine: "fugumt", cache: "mixed" }), label),
    "fugumt · partly from cache · CPU",
  );
  assert.equal(resultPhrase(result(), label), "manga-ocr · CPU");
});

test("a failure is said as its code", () => {
  assert.equal(
    resultPhrase(
      result({ engine: "", outcome: { kind: "failed", code: "engine_unavailable" } }),
      label,
    ),
    "failed (engine unavailable)",
  );
  assert.equal(resultPhrase(null, label), null);
});

test("status names cover the analyzers and the translator, and defer for recognition", () => {
  const named = statusLabel(label);
  assert.equal(named("fugumt"), "FuguMT");
  assert.equal(named("automatic"), "Automatic");
  assert.equal(named("vlm"), "PaddleOCR-VL");
});

test("loopback is this computer only, with the same cases as the Rust rule", () => {
  for (const url of [
    "http://127.0.0.1:8140",
    "http://127.4.5.6/v1",
    "http://localhost:8080",
    "http://LOCALHOST",
    "http://[::1]:8000/v1",
  ]) {
    assert.equal(isLoopback(url), true, `${url} is this computer`);
  }
  for (const url of [
    "http://192.168.1.10:8140",
    "http://10.0.0.2",
    "http://172.16.0.1",
    "https://api.example.com/v1",
    "http://localhost.example.com",
    "http://127.0.0.1.example.com",
    "",
    "not a url",
  ]) {
    assert.equal(isLoopback(url), false, `${url} is not this computer`);
  }
});

test("an engine with nothing installed says so once", () => {
  assert.deepEqual(
    engineRow(
      {
        role: "translation",
        configured: "none",
        loaded: null,
        hosting: { kind: "in_process" },
        state: "not_installed",
        acceleration: { kind: "not_running" },
      },
      label,
    ),
    { role: "Translation", name: "Not installed", detail: "" },
  );
});
