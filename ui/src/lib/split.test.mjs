import assert from "node:assert/strict";
import { test } from "node:test";

import { clampRatio, floorShare, snapToCentre } from "./split.js";

// The numbers below are the real ones, measured in the running app at the default reading scale:
// one line of Japanese with furigana is 85px, the translation band under it is 37px, and the
// container the divider moves in is 351px tall in a 640px window.
const LINE = 85;
const BAND = 37;
const FLOOR = LINE + BAND;
const SPAN = 351;
const BOUNDS = { min: 0.2, max: 0.8 };

test("with no floor it is the old clamp, exactly", () => {
  assert.equal(clampRatio(0.45, BOUNDS), 0.45);
  assert.equal(clampRatio(0.05, BOUNDS), 0.2);
  assert.equal(clampRatio(0.95, BOUNDS), 0.8);
});

test("a pane's floor outranks the share minimum", () => {
  // 0.2 of 351px is 70px. The reader needs 122. The share minimum is simply not the binding one.
  const parked = clampRatio(0.2, { ...BOUNDS, span: SPAN, floor: FLOOR });
  assert.ok(parked * SPAN >= FLOOR, `${parked * SPAN}px is under the ${FLOOR}px floor`);
  assert.ok(parked > 0.2, "the floor should have raised it above the share minimum");
});

test("the floor is a floor, not a size — a comfortable divider is left alone", () => {
  assert.equal(clampRatio(0.6, { ...BOUNDS, span: SPAN, floor: FLOOR }), 0.6);
});

test("dragging to the very top lands on the floor and not below it", () => {
  const dragged = clampRatio(-0.4, { ...BOUNDS, span: SPAN, floor: FLOOR });
  assert.ok(Math.abs(dragged * SPAN - FLOOR) < 0.001, `${dragged * SPAN}px, wanted ${FLOOR}px`);
});

test("when the container cannot honour both, the pane's contents win over the ceiling", () => {
  // A container barely taller than the floor: every share up to 0.8 is too small.
  const cramped = clampRatio(0.5, { ...BOUNDS, span: 130, floor: FLOOR });
  assert.ok(cramped > BOUNDS.max, "the floor must be allowed to push past max");
  assert.ok(cramped * 130 >= FLOOR - 0.001);
});

test("a container shorter than the floor asks for all of it, never more", () => {
  assert.equal(floorShare(60, FLOOR), 1);
  assert.equal(clampRatio(0.45, { ...BOUNDS, span: 60, floor: FLOOR }), 1);
});

test("no floor is known, so no floor is applied", () => {
  // `parseFloat` of a CSS `min-height` of `auto` or `none` is NaN, and a container that has not
  // been laid out reports 0. Both have to mean *unconstrained*, not *infinitely constrained*.
  assert.equal(floorShare(SPAN, Number.NaN), 0);
  assert.equal(floorShare(0, FLOOR), 0);
  assert.equal(floorShare(Number.NaN, FLOOR), 0);
  assert.equal(clampRatio(0.45, { ...BOUNDS, span: 0, floor: FLOOR }), 0.45);
  assert.equal(clampRatio(0.45, { ...BOUNDS, span: SPAN, floor: Number.NaN }), 0.45);
});

test("a ratio that came out NaN does not get stored", () => {
  // Dividing by a container of zero width is how this happens, and a NaN ratio poisons every
  // load afterwards rather than just the drag it came from.
  assert.equal(clampRatio(Number.NaN, BOUNDS), 0.2);
  assert.ok(Number.isFinite(clampRatio(Number.NaN, { ...BOUNDS, span: SPAN, floor: FLOOR })));
});

test("negative and absurd floors are ignored rather than obeyed", () => {
  assert.equal(floorShare(SPAN, -50), 0);
  assert.equal(clampRatio(0.45, { ...BOUNDS, span: SPAN, floor: -50 }), 0.45);
});

test("the floor tracks the reading scale, because it is measured not assumed", () => {
  // At twice the reading scale a line is twice as tall, and the share it needs doubles with it.
  const single = floorShare(SPAN, LINE);
  const doubled = floorShare(SPAN, LINE * 2);
  assert.ok(Math.abs(doubled - single * 2) < 1e-9);
});

/**
 * **The snap zone is measured in pixels, and these tests exist to keep it that way.**
 *
 * A share threshold is the obvious simplification and it is wrong: the zone would be physically
 * wider on a wide window than a narrow one, so the same gesture would behave differently
 * depending on the window. `sameZoneAtTwoSizes` below is the assertion that fails if anybody
 * makes that change.
 */
test("a divider dropped near the centre lands exactly on it", () => {
  const snap = { span: 800, within: 14 };
  // 14px of 800 is a share of 0.0175, so these are 8px and 4px off centre.
  assert.equal(snapToCentre(0.51, snap), 0.5);
  assert.equal(snapToCentre(0.495, snap), 0.5);
  assert.equal(snapToCentre(0.5, snap), 0.5);
});

test("a divider parked deliberately off centre is left exactly where it was put", () => {
  const snap = { span: 800, within: 14 };
  // A third along is a position somebody chose. A snap that reached this far would be a magnet.
  assert.equal(snapToCentre(0.3333, snap), 0.3333);
  assert.equal(snapToCentre(0.6, snap), 0.6);
  // Just outside the zone, to the digit: 0.0176 > 0.0175.
  assert.equal(snapToCentre(0.5176, snap), 0.5176);
});

test("the zone is the same physical width whatever the container is", () => {
  // The same 14px, on containers of 400 and 1600. In share terms that is 0.035 and 0.00875, so a
  // divider 0.02 off centre is inside the zone on the narrow one and outside it on the wide one —
  // which is exactly what "fourteen pixels" means and what a share threshold could not express.
  assert.equal(snapToCentre(0.52, { span: 400, within: 14 }), 0.5);
  assert.equal(snapToCentre(0.52, { span: 1600, within: 14 }), 0.52);
});

test("nothing that cannot be measured is snapped", () => {
  // A container that has not been laid out yet, and a caller that did not ask for snapping.
  // Both have to mean *no zone* rather than a zone of unknown size.
  assert.equal(snapToCentre(0.51, { span: 0, within: 14 }), 0.51);
  assert.equal(snapToCentre(0.51, { span: 800, within: 0 }), 0.51);
  assert.equal(snapToCentre(0.51, {}), 0.51);
  assert.equal(snapToCentre(0.51, { span: Number.NaN, within: 14 }), 0.51);
});

test("the clamp still has the last word over the snap", () => {
  // Contrived here, but the reason the caller snaps *before* clamping: the centre is not
  // guaranteed to be a position the layout allows, and `clampRatio` is what decides that.
  const snapped = snapToCentre(0.51, { span: 800, within: 14 });
  assert.equal(snapped, 0.5);
  assert.equal(clampRatio(snapped, { min: 0.6, max: 0.9 }), 0.6);
});

/**
 * **Why `Split.svelte`'s `key()` clamps without snapping.**
 *
 * A snap zone wider than the arrow-key step swallows every step: the divider reaches the centre
 * and cannot leave it, in either direction, ever. That is not hypothetical arithmetic — it is what
 * `snapToCentre` does when asked, and the only reason the app does not show it today is that at
 * the widths where the split exists the zone happens to come out narrower than the step.
 *
 * **Which is luck, not design.** Raise `snapWithin`, or put the split in a narrower container, and
 * the trap appears. So the rule is "the keyboard does not snap" rather than "the numbers work out",
 * and this is the test that says why.
 */
test("a snap zone wider than a keyboard step would trap a stepped divider", () => {
  const step = 0.02;
  const down = 0.5 - step;
  const up = 0.5 + step;

  // 14px of a 600px split is a share of 0.0233 — wider than the step. Both directions come back.
  const narrow = { span: 600, within: 14 };
  assert.equal(snapToCentre(down, narrow), 0.5);
  assert.equal(snapToCentre(up, narrow), 0.5);

  // 14px of 900px is 0.0156, and the step clears it. The app is here today, by coincidence.
  const wide = { span: 900, within: 14 };
  assert.equal(snapToCentre(down, wide), down);
  assert.equal(snapToCentre(up, wide), up);
});
