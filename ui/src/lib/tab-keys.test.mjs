import assert from "node:assert/strict";
import { test } from "node:test";

import { tabShortcut } from "./tab-keys.js";

/** A keystroke, spelled the way the browser reports one. */
const press = (code, extra = {}) => ({
  key: code.startsWith("Key") ? code.slice(3).toLowerCase() : code,
  code,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...extra,
});
const ctrl = (code, extra = {}) => press(code, { ctrlKey: true, ...extra });

test("the four commands a chord can mean", () => {
  assert.deepEqual(tabShortcut(ctrl("KeyT")), { kind: "new" });
  assert.deepEqual(tabShortcut(ctrl("KeyW")), { kind: "close" });
  assert.deepEqual(tabShortcut(ctrl("KeyT", { shiftKey: true })), { kind: "reopen" });
});

test("Tab cycles, and Shift turns it round", () => {
  assert.deepEqual(tabShortcut({ ...ctrl("Tab"), key: "Tab" }), { kind: "next" });
  assert.deepEqual(tabShortcut({ ...ctrl("Tab", { shiftKey: true }), key: "Tab" }), {
    kind: "previous",
  });
});

test("bare Tab is not ours — it is the focus-traversal key the app already swallows", () => {
  assert.equal(tabShortcut({ key: "Tab", code: "Tab" }), null);
  assert.equal(tabShortcut({ key: "Tab", code: "Tab", shiftKey: true }), null);
});

test("one through eight jump, and nine is the last tab rather than the ninth", () => {
  for (let nth = 1; nth <= 8; nth += 1) {
    assert.deepEqual(
      tabShortcut(ctrl(`Digit${nth}`)),
      { kind: "jump", index: nth - 1 },
      `Ctrl+${nth} lands on tab ${nth}`,
    );
  }
  assert.deepEqual(
    tabShortcut(ctrl("Digit9")),
    { kind: "jump", index: -1 },
    "nine is the end of the strip, whatever length it is",
  );
});

/**
 * **The layout trap.** On AZERTY the top row gives `&é"` and `event.key` for Ctrl+1 is `&`, so a
 * mapping written against `key` works on the author's keyboard and silently on nobody else's.
 * `code` is the physical key.
 */
test("a digit is read from the physical key, so a non-US layout still works", () => {
  assert.deepEqual(tabShortcut({ key: "&", code: "Digit1", ctrlKey: true }), {
    kind: "jump",
    index: 0,
  });
  // And a key that *prints* a digit without being one is not a jump.
  assert.equal(tabShortcut({ key: "1", code: "Numpad1", ctrlKey: true }), null);
});

/**
 * AltGr arrives as Ctrl+Alt on many European layouts, so `Ctrl+Alt+T` is somebody typing a
 * character. Claiming it would make the app eat their keyboard.
 */
test("Alt disqualifies everything, because AltGr is Ctrl+Alt", () => {
  for (const code of ["KeyT", "KeyW", "Digit1", "Tab"]) {
    assert.equal(tabShortcut(ctrl(code, { altKey: true })), null, `Ctrl+Alt+${code} is not ours`);
  }
});

test("nothing fires without a command key", () => {
  for (const code of ["KeyT", "KeyW", "Digit1"]) {
    assert.equal(tabShortcut(press(code)), null, `a bare ${code} is typing`);
    assert.equal(tabShortcut(press(code, { shiftKey: true })), null);
  }
});

test("Cmd works like Ctrl, so the mapping is not Windows-only", () => {
  assert.deepEqual(tabShortcut(press("KeyT", { metaKey: true })), { kind: "new" });
  assert.deepEqual(tabShortcut(press("Digit9", { metaKey: true })), { kind: "jump", index: -1 });
});

/**
 * `Ctrl+Shift+W` closes the whole window in a browser. There is one window here and it holds the
 * reading session — a chord that throws that away by being one key off `Ctrl+W` is a trap.
 */
test("the chords next door to ours do nothing", () => {
  assert.equal(tabShortcut(ctrl("KeyW", { shiftKey: true })), null, "Ctrl+Shift+W is not close");
  assert.equal(tabShortcut(ctrl("Digit1", { shiftKey: true })), null);
  assert.equal(tabShortcut(ctrl("KeyN")), null, "no new window here");
  assert.equal(tabShortcut(ctrl("KeyQ")), null, "and no quit");
});

test("an unrecognised key is not a command", () => {
  for (const code of ["KeyA", "KeyS", "Escape", "Enter", "ArrowLeft", "F5", "Digit0"]) {
    assert.equal(tabShortcut(ctrl(code)), null, `Ctrl+${code} means nothing here`);
  }
});
