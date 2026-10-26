import { expect, test } from "./fixtures";
import { gear, menu } from "./menu";

/**
 * **The interface size scales the interface, and anything drawn in device pixels does not.**
 *
 * `font_size` is the root's `font-size`, so every `rem` in the app follows it — the rail is
 * `2.9rem`, its buttons `2rem`, the header toggles the same. Two things kept being written in
 * device pixels instead, and both only show up once somebody drags that setting:
 *
 * - **The glyphs**, as `width="18"` attributes. At the default an icon is 60% of its button; at the
 *   smallest size the button shrinks to 24px and the icon does not, so it fills three quarters of
 *   it and the rail reads as broken. At the largest it is lost in the middle of a 48px button.
 * - **The browser's own `button` padding**, which is `1px 6px` and was never reset. With
 *   `box-sizing: border-box` a 24px button then offers its content 10px, the icon overflows the
 *   track it is meant to be centred in, and `place-items: center` centres it against a box that
 *   starts 7px in — so it sits right of centre, by more the smaller the interface gets. At 12px the
 *   header toggles were 7px out of 24 and their icons touched the right edge.
 *
 * Both are asserted as *proportions* rather than sizes, because the proportion is the thing that
 * must not move.
 */

/** Every fixed-size icon button the interface scales: the rail's views and the header's toggles. */
const ICON_BUTTONS = ".rail .view, .toggle";

async function measure(page: import("@playwright/test").Page) {
  return page.evaluate((selector) => {
    return [...document.querySelectorAll(selector)]
      .filter((el) => el.querySelector("svg") && el.getBoundingClientRect().width > 0)
      .map((el) => {
        const b = el.getBoundingClientRect();
        const s = el.querySelector("svg")!.getBoundingClientRect();
        return {
          what: `${(el.className + "").split(" ").slice(0, 2).join(".")}`,
          button: Math.round(b.width),
          ratio: Number((s.width / b.width).toFixed(2)),
          // Positive means the icon sits right of centre.
          skew: Number((s.left - b.left - (b.right - s.right)).toFixed(2)),
        };
      });
  }, ICON_BUTTONS);
}

test("icon buttons keep their proportion and their centre at every interface size", async ({
  page,
}) => {
  await page.goto("/?qa=analysis");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  // The rail and the header toggles both have to be drawn before anything is measured. Without
  // this the first `evaluate` ran against an empty document under parallel load and threw on a
  // null, which is a flake rather than a finding — it failed in the full suite and passed alone.
  await expect(page.locator(".rail .view").first()).toBeVisible();
  await expect(page.locator(".toggle").first()).toBeVisible();

  const settled = await measure(page);
  expect(settled.length, "there are icon buttons to measure").toBeGreaterThan(3);

  // The two ends of what the Interface setting offers, and one beyond the middle.
  for (const size of [12, 18, 24]) {
    await page.evaluate((px) => {
      document.documentElement.style.fontSize = `${px}px`;
    }, size);
    const now = await measure(page);

    for (const [index, was] of settled.entries()) {
      const is = now[index];
      expect(is.button, `${is.what} follows the interface at ${size}px`).not.toBe(was.button);
      expect(is.ratio, `${is.what} keeps its share of its button at ${size}px`).toBe(was.ratio);
      // A subpixel is a rounding artefact; anything above it is a glyph off its centre.
      expect(Math.abs(is.skew), `${is.what} stays centred at ${size}px`).toBeLessThan(0.2);
    }
  }
});

/**
 * **The open gear and its menu are one shape, and the join has to be exactly one button wide.**
 *
 * `.menu::before` paints over the menu's top border so the two read as a single surface. `right` on
 * an absolutely positioned box is measured from the containing block's *padding* box — inside
 * `.menu`'s own 1px border — and the offset was written as though it were measured from the border
 * box. One pixel out in each direction: a tick of un-erased border poking from the menu's right
 * edge, and a notch where the gear's left border should meet the menu's top. Both were visible, and
 * both were reported.
 *
 * Asserted against the gear's own rectangle rather than a literal, so the two cannot drift apart.
 */
test("the join under the open gear covers exactly the gear's interior", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await gear(page).click();
  await expect(menu(page)).toBeVisible();

  const seam = await page.evaluate(() => {
    const g = document.querySelector(".cap.gear")!.getBoundingClientRect();
    const m = document.querySelector(".menu")!;
    const box = m.getBoundingClientRect();
    const style = getComputedStyle(m);
    const before = getComputedStyle(m, "::before");
    // `right` counts from the padding box, so the menu's own right border comes off first.
    const right = box.right - parseFloat(style.borderRightWidth) - parseFloat(before.right);
    return {
      eraseLeft: right - parseFloat(before.width),
      eraseRight: right,
      gearLeft: g.left,
      gearRight: g.right,
      height: parseFloat(before.height),
      // `top` counts from the padding box too, so the menu's top border is added back on.
      top: box.top + parseFloat(style.borderTopWidth) + parseFloat(before.top),
      gearBottom: g.bottom,
    };
  });

  // The gear's own side borders are 1px each and must survive; everything between them goes.
  expect(seam.eraseLeft, "the join starts inside the gear's left border").toBe(seam.gearLeft + 1);
  expect(seam.eraseRight, "and ends inside its right border").toBe(seam.gearRight - 1);
  expect(seam.height, "it is exactly the border it paints over").toBe(1);
  expect(seam.top, "and sits on the menu's top edge").toBe(seam.gearBottom);
});
