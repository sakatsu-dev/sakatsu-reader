import { strict as assert } from "node:assert";
import test from "node:test";
import { DRAG_SLOP, travelled } from "./card-click.js";

test("a press that does not move is a click", () => {
  assert.equal(travelled({ x: 100, y: 100 }, { x: 100, y: 100 }), false);
});

test("a hand that drifts within the slop still clicks", () => {
  assert.equal(travelled({ x: 100, y: 100 }, { x: 100 + DRAG_SLOP, y: 100 - DRAG_SLOP }), false);
});

test("travel past the slop on either axis is a drag", () => {
  assert.equal(travelled({ x: 100, y: 100 }, { x: 100 + DRAG_SLOP + 1, y: 100 }), true);
  assert.equal(travelled({ x: 100, y: 100 }, { x: 100, y: 100 + DRAG_SLOP + 1 }), true);
});

test("direction does not matter — a drag up and left is still a drag", () => {
  assert.equal(travelled({ x: 100, y: 100 }, { x: 20, y: 20 }), true);
});

/**
 * The case that broke the card: a double-click selects a word without moving the pointer, so the
 * second click has to keep counting as a click. Asking "is anything selected?" answered yes here
 * and left the card stuck.
 */
test("a second click in the same place is still a click, whatever it selected", () => {
  assert.equal(travelled({ x: 240, y: 310 }, { x: 240, y: 310 }), false);
});

test("a click with no press behind it acts", () => {
  assert.equal(travelled(null, { x: 0, y: 0 }), false);
});
