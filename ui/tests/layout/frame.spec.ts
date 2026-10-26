import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";

/**
 * The capture overlay's edges, which are the one surface where drag and resize compete.
 *
 * **The main window barely has to settle this, and the reason is measurable.** Its window rect
 * is 916px wide against 902px visible: `DWMWA_EXTENDED_FRAME_BOUNDS` reports a 7px invisible
 * margin left, right and bottom, and none at the top. Windows hit-tests the resize border from
 * the *window* rect, so on the sides that border is spent almost entirely on margin nobody can
 * see — and every visible pixel of the title bar drags.
 *
 * The overlay is transparent with no shadow, so it has no margin at all, and `resizable(false)`
 * leaves it no native border either (the OS one bypassed `clamp_frame_size` and broke the
 * minimum size). Its handles are drawn over its own chrome, out of the same pixels the strip is
 * made of. **What these measure is therefore how much of the visible strip survives**: the rim
 * no deeper than the main window's 8px, the corners no deeper than the rim, and the sides kept
 * off the strip entirely.
 *
 * Geometry only: no screenshot, and nothing that depends on a font.
 */

/** The frame renders standalone at `?frame=1`; the QA mock supplies the IPC it asks for on mount. */
async function frame(page: Page) {
  await page.goto("/?frame=1&qa=analysis");
  await expect(page.locator(".grip")).toBeVisible();
}

type Box = { x: number; y: number; width: number; height: number };

async function boxes(page: Page) {
  return page.evaluate(() => {
    const rect = (element: Element) => {
      const { x, y, width, height } = element.getBoundingClientRect();
      return { x, y, width, height };
    };
    const grip = document.querySelector(".grip")!;
    return {
      grip: rect(grip),
      handles: Object.fromEntries(
        [...document.querySelectorAll(".handle")].map((element) => [
          [...element.classList].find((name) => /^(n|s|e|w|ne|nw|se|sw)$/.test(name)) ?? "?",
          rect(element),
        ]),
      ),
    };
  });
}

/**
 * The rim is the main window's rim.
 *
 * `SM_CXSIZEFRAME + SM_CXPADDEDBORDER` measures 8 on Windows, and matching it is the point: the
 * same gesture should want the same aim in either window. Asserted against the drawn handle rather
 * than the constant, because the constant being right is worth nothing if the CSS disagrees.
 */
test("the resize rim is the same 8px the main window resizes from", async ({ page }) => {
  await frame(page);
  const { handles } = await boxes(page);
  expect((handles.n as Box).height).toBeCloseTo(8, 1);
  expect((handles.s as Box).height).toBeCloseTo(8, 1);
  expect((handles.w as Box).width).toBeCloseTo(8, 1);
  expect((handles.e as Box).width).toBeCloseTo(8, 1);
});

/**
 * **No corner may reach deeper than the rim**, which is the half that was actually broken.
 *
 * Square 20px corners over a 28px strip took its top fifth at both ends while the `n` handle took
 * 14px across the middle, leaving a thin inset band to grab. Corners are a width now and
 * `--thickness` tall, so the strip keeps everything below 8px.
 */
test("no corner dips deeper into the grip than the rim does", async ({ page }) => {
  await frame(page);
  const { handles } = await boxes(page);
  const rim = (handles.n as Box).height;
  for (const edge of ["nw", "ne"]) {
    expect((handles[edge] as Box).height, `${edge} is no deeper than the rim`).toBeCloseTo(rim, 1);
  }
  // Wide, though — that is the axis that makes a corner easy to find along an edge.
  expect((handles.nw as Box).width).toBeGreaterThan(rim * 2);
});

/**
 * What the reader actually cares about: how much of the strip is left to grab.
 *
 * **The width half is the one that was still wrong after the rim was fixed.** Measured on the
 * running reader, its window rect is 916px wide against 902px visible: `DWMWA_EXTENDED_FRAME_BOUNDS`
 * reports a 7px invisible margin left, right and bottom and none at the top. Windows hit-tests the
 * resize border from the *window* rect, so on the sides that border is spent almost entirely on
 * margin nobody sees, and every visible pixel of the title bar drags.
 *
 * The frame has no margin — transparent, no shadow — so a side handle at strip height is taken out
 * of the strip. These assert it is not: the strip drags end to end, and keeps most of its height.
 */
test("the drag strip runs the full width of the frame", async ({ page }) => {
  await frame(page);
  const { grip, handles } = await boxes(page);
  const strip = grip as Box;

  for (const edge of ["e", "w"]) {
    const side = handles[edge] as Box;
    expect(side.y, `${edge} starts below the strip`).toBeGreaterThanOrEqual(
      strip.y + strip.height - 0.5,
    );
  }
});

test("the drag strip keeps most of its height", async ({ page }) => {
  await frame(page);
  const { grip, handles } = await boxes(page);
  const strip = grip as Box;
  const north = handles.n as Box;
  const northwest = handles.nw as Box;

  const top = Math.max(north.y + north.height, northwest.y + northwest.height);
  expect(strip.y + strip.height - top).toBeGreaterThanOrEqual(strip.height * 0.6);
});
