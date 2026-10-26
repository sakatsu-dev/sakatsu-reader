import assert from "node:assert/strict";
import { test } from "node:test";

import { FAST_SECONDS, readSpeed } from "./vlm-speed.js";

/** The two machines this was written for, both real measurements. */
test("a graphics card is recommended and a processor is not", () => {
  const gpu = readSpeed(0.1);
  assert.equal(gpu.fast, true);
  assert.equal(gpu.perCapture, "0.1s");

  const cpu = readSpeed(49);
  assert.equal(cpu.timed, true, "a slow machine has still been measured");
  assert.equal(cpu.fast, false);
  assert.equal(cpu.perCapture, "49s");
});

/**
 * Untimed is its own state, not a slow one.
 *
 * Both withhold the recommendation, so a single `fast` boolean would look correct while
 * quietly telling someone their machine is slow when nothing has measured it.
 */
test("an unmeasured machine has nothing claimed about it", () => {
  for (const nothing of [null, undefined, Number.NaN, 0, -1]) {
    const speed = readSpeed(nothing);
    assert.deepEqual(
      speed,
      { timed: false, fast: false, perCapture: "" },
      `${String(nothing)} is not a measurement`,
    );
  }
});

/** The boundary is inclusive, and one step past it is not. */
test("the threshold includes its own value", () => {
  assert.equal(readSpeed(FAST_SECONDS).fast, true);
  assert.equal(readSpeed(FAST_SECONDS + 0.01).fast, false);
});

/**
 * The rounding switch. A tenth of a second matters at the fast end and is noise at the slow
 * end, so the format changes rather than the precision being a compromise between them.
 */
test("a decimal below ten seconds, none above", () => {
  assert.equal(readSpeed(0.14).perCapture, "0.1s");
  assert.equal(readSpeed(1.25).perCapture, "1.3s");
  assert.equal(readSpeed(9.94).perCapture, "9.9s");
  assert.equal(readSpeed(10).perCapture, "10s");
  assert.equal(readSpeed(47.3).perCapture, "47s");
});

/**
 * The real measurement from the machine this was developed on: 0.03 s. One decimal turns that
 * into "0.0s", which describes the best possible outcome as though nothing happened.
 */
test("a machine faster than a tenth of a second is not reported as taking none", () => {
  const measured = readSpeed(0.03);
  assert.equal(measured.timed, true);
  assert.equal(measured.fast, true);
  assert.equal(measured.perCapture, "under 0.1s");
});
