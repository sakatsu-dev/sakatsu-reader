import { expect, test } from "./fixtures";

/**
 * The rail's seal, which is chrome rather than a page and so has no spec of its own until now.
 *
 * The one thing asserted here is that the artwork fills the box the rail gives it. That sounds
 * too obvious to test, and it is exactly what broke: a `.mark` rule promoted into `theme.css`
 * for the word-status chips became global, landed on `Mark.svelte`'s `<span class="mark">` as
 * well, and its `padding: 0.14rem 0.48rem` squeezed the seal's content box to 6.6px wide. The
 * `<svg>` is `width: 100%`, so it obediently shrank to a red smudge inside a box still measuring
 * 21px — nothing overflowed, nothing was unused, and every other check stayed green. See
 * `local documentation`.
 */
test("the rail's seal fills the box the rail gives it", async ({ page }) => {
  await page.goto("/?qa=analysis");
  const seal = page.locator("nav.rail > span.mark");
  await expect(seal).toBeAttached();
  const fit = await seal.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const art = element.querySelector("svg")!.getBoundingClientRect();
    return { box: { w: box.width, h: box.height }, art: { w: art.width, h: art.height } };
  });
  // Not a ratio: any padding, border or box-sizing change on this element shows up as the
  // artwork being *smaller* than its box, whatever the box then measures.
  expect(fit.box.w).toBeGreaterThan(0);
  expect(fit.art.w).toBeCloseTo(fit.box.w, 1);
  expect(fit.art.h).toBeCloseTo(fit.box.h, 1);
});
