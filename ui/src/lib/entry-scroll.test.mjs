import test from "node:test";
import assert from "node:assert/strict";
import { revealEntryHeading } from "./entry-scroll.js";

test("revealing a heading moves only its list and only as far as needed", () => {
  const scroller = {
    scrollTop: 100,
    getBoundingClientRect: () => ({ top: 20, bottom: 220, height: 200 }),
  };
  const heading = (top, bottom) => ({
    getBoundingClientRect: () => ({ top, bottom, height: bottom - top }),
  });
  revealEntryHeading(scroller, heading(40, 90));
  assert.equal(scroller.scrollTop, 100);
  revealEntryHeading(scroller, heading(0, 50));
  assert.equal(scroller.scrollTop, 80);
  revealEntryHeading(scroller, heading(200, 250));
  assert.equal(scroller.scrollTop, 110);
  revealEntryHeading(scroller, heading(50, 400));
  assert.equal(scroller.scrollTop, 140);
});
