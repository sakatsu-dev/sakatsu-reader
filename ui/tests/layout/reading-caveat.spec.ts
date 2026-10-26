import { expect, test } from "./fixtures";
import type { Page } from "@playwright/test";

/**
 * **A reading the app is not sure of has to look different from one it is.**
 *
 * Furigana is taken on trust — that is what it is for — and `core/tests/heteronym_readings.rs`
 * measures the contested ones at 69.2% correct. Until this mark existed the other 31% were drawn
 * exactly like a reading three dictionaries agree on, so a learner had no way to tell.
 *
 * The mark is on the `<rt>` and nowhere else: the word is on the page and is not in doubt, only the
 * kana above it. Every other channel on the token is already spoken for — the bottom border is word
 * status, the dashed strip above it is grammar, line-through is the comparison set, the wash is
 * selection.
 *
 * **These tests exist because the previous marker was invisible.** `.token.unverified rt` set
 * `color: var(--text-faint)`, which the base `rt` rule already sets, so it rendered identically to
 * a certain reading for as long as it shipped. The unused-selector gate cannot see that — the
 * selector matches, it just changes nothing — and no fixture set the flag, so nothing rendered it
 * either. Asserting the *computed difference* is what closes both holes.
 */

async function reader(page: Page) {
  await page.goto("/?qa=analysis");
  await page.getByPlaceholder("Paste Japanese text").fill("気持ちが伝わってきます。");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".reader")).toBeVisible();
}

test("a contested reading is marked and a settled one is not", async ({ page }) => {
  await reader(page);

  const marked = page.locator(".token.ambiguous rt").first();
  await expect(marked, "the fixture flags one token, so the mark has something to land on").toHaveCount(1);

  const paint = await page.evaluate(() => {
    const read = (el: Element) => {
      const style = getComputedStyle(el);
      return {
        line: style.textDecorationLine,
        pattern: style.textDecorationStyle,
        colour: style.textDecorationColor,
        fill: style.color,
      };
    };
    return {
      marked: read(document.querySelector(".token.ambiguous rt")!),
      plain: read(document.querySelector(".token:not(.ambiguous):not(.unverified) rt")!),
    };
  });

  expect(paint.marked.line, "the caveat is a rule under the reading").toBe("underline");
  expect(paint.marked.pattern).toBe("dotted");
  expect(paint.plain.line, "a settled reading carries nothing").toBe("none");

  // **The point of the whole exercise**: the two must not resolve to the same paint. This is the
  // assertion the dead `.unverified` rule would have failed from the day it shipped.
  expect(
    JSON.stringify(paint.marked) !== JSON.stringify(paint.plain),
    "a marked reading must render differently from an unmarked one",
  ).toBe(true);

  // Drawn in its own colour rather than the kana's, or a dotted rule at 11px disappears into it.
  expect(paint.marked.colour).not.toBe(paint.marked.fill);
});

/**
 * The reader's height is a floor other things are sized against — `--reader-line`, `<Split
 * firstMin>` and `tauri.conf.json`'s `minHeight` all depend on what one line costs. A decoration
 * adds no line box, and this is what says so rather than assuming it.
 */
test("the caveat costs the passage no height", async ({ page }) => {
  await reader(page);
  const sizes = await page.evaluate(() => {
    const marked = document.querySelector(".token.ambiguous")!.getBoundingClientRect();
    const plain = document.querySelector(".token:not(.ambiguous):not(.unverified)")!.getBoundingClientRect();
    return { marked: marked.height, plain: plain.height };
  });
  expect(sizes.marked).toBe(sizes.plain);
});

/** It is a caveat on the reading, so it goes when the readings do. */
test("no furigana means no caveat", async ({ page }) => {
  await reader(page);
  await expect(page.locator(".token.ambiguous rt")).toHaveCount(1);

  await page.getByRole("button", { name: /^Furigana/ }).click();
  await expect(page.locator(".reader rt")).toHaveCount(0);
  await expect(page.locator(".token.ambiguous rt")).toHaveCount(0);
});

/**
 * **The legend has to show the mark it describes, and it is drawn from a copy.**
 *
 * `Help.svelte` cannot borrow `Reader.svelte`'s styles — Svelte scopes them — so the legend
 * hand-duplicates the token treatment. The comment over those rules says the file "has lied
 * twice", meaning the copy has drifted from the real thing before. Comparing the two computed
 * styles is cheaper than a third occasion.
 *
 * This matters more than usual here: the mark carries no tooltip. `tooltips.md` rules out
 * per-token tips as the reader's hot path and the passage's scroller would crop them anyway, so
 * the legend is the *only* place the mark is explained. A legend that draws it differently
 * explains the wrong thing.
 */
test("the Help legend draws the caveat the way the reader does", async ({ page }) => {
  await reader(page);
  const inReader = await page.evaluate(() => {
    const style = getComputedStyle(document.querySelector(".token.ambiguous rt")!);
    return `${style.textDecorationLine} ${style.textDecorationStyle} ${style.textDecorationColor}`;
  });

  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "Help", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Help", exact: true })).toBeVisible();

  const legend = page.locator(".marks .token.ambiguous rt");
  await expect(legend, "the legend shows the mark it names").toHaveCount(1);
  const inLegend = await legend.evaluate((el) => {
    const style = getComputedStyle(el);
    return `${style.textDecorationLine} ${style.textDecorationStyle} ${style.textDecorationColor}`;
  });

  expect(inLegend, "the legend and the passage draw one mark").toBe(inReader);
});
