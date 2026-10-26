import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { assertContained } from "./contained";

/**
 * **Two size controls, and the translation used to answer only one of them.**
 *
 * - **Interface size** sets the root `font-size`, so every `rem` in the app follows it.
 * - **Definitions size** supplies `--panel-scale`, which each panel component multiplies its own
 *   `rem` sizes by.
 *
 * `.translation-card` declared `font-size: calc(var(--panel-font) * 1rem)`. There is no
 * `--panel-font` in this project, so the declaration was invalid at computed-value time and the
 * browser dropped it. The consequence was narrower than it first looked: the body text still
 * followed Interface size, because it inherited a plain `1rem` — what it never did was respond to
 * Definitions size, alone among the panels.
 *
 * These tests hold each control while moving the other, which is the only way to tell the two
 * apart. They are set as properties rather than driven through the settings UI, the same way
 * `interface-size.spec.ts` does it: what is under test is what the CSS does with the value.
 */

const passage = "気持ちが伝わってきます。";

/** The two ends this checks, and a ratio big enough that no rounding can produce it by accident. */
const SMALL = 1;
const LARGE = 1.5;

async function analyze(page: Page, query = "", text = passage) {
  await page.goto(`/?qa=analysis${query}`);
  await page.getByPlaceholder("Paste Japanese text").fill(text);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  await page.getByRole("button", { name: "Translation", exact: true }).click();
  await expect(page.getByTestId("translation-card")).toBeVisible();
}

async function setDefinitionsSize(page: Page, scale: number) {
  await page.evaluate((value) => {
    document.documentElement.style.setProperty("--panel-scale", String(value));
  }, scale);
  // Layout follows the property on the next frame. Waiting for two settles it.
  await page.evaluate(
    () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))),
  );
}

/**
 * The rendered text, not the container.
 *
 * `selector` is the paragraph a reader actually reads — the aligned view's sentence or the whole
 * passage — because a container can be the right size while the text inside it inherits something
 * else entirely.
 */
async function rendered(page: Page, selector: string) {
  return page.evaluate((target) => {
    const body = document.querySelector<HTMLElement>("[data-testid=translation-body]")!;
    const text = document.querySelector<HTMLElement>(target)!;
    return {
      text: Number.parseFloat(getComputedStyle(text).fontSize),
      body: Number.parseFloat(getComputedStyle(body).fontSize),
      lineHeight: Number.parseFloat(getComputedStyle(body).lineHeight),
      height: body.getBoundingClientRect().height,
      scrollHeight: body.scrollHeight,
      maxHeight: Number.parseFloat(getComputedStyle(body).maxHeight),
      root: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
    };
  }, selector);
}

/**
 * Each view is reached by the passage rather than by a click.
 *
 * **Whole passage is the default** wherever it is offered, and it is offered only for more than
 * one sentence (`contextCheckAvailable`). So two sentences land in the whole-passage view and one
 * lands line by line, with no control touched in either — which is what a reader gets too.
 */
const VIEWS = [
  { name: "line by line", selector: "[data-testid=aligned-translation] p", text: passage },
  {
    name: "whole passage",
    selector: "[data-testid=context-translation]",
    text: `前の文です。${passage}`,
  },
] as const;

for (const view of VIEWS) {
  test(`the ${view.name} translation follows Definitions size, and shows in full at both`, async ({
    page,
  }) => {
    await analyze(page, "&fixture=long-translation", view.text);
    await expect(page.locator(view.selector)).toBeVisible();

    await setDefinitionsSize(page, SMALL);
    const small = await rendered(page, view.selector);

    await setDefinitionsSize(page, LARGE);
    const large = await rendered(page, view.selector);

    // **The text itself grew**, in the ratio the setting asked for.
    expect(large.text / small.text).toBeCloseTo(LARGE / SMALL, 1);
    expect(large.text, "the paragraph takes the body's size, not an inherited one").toBeCloseTo(
      large.body,
      1,
    );

    // **And it is shown whole at both sizes**: as tall as the text, up to the card's bound, and
    // never behind a disclosure.
    for (const at of [small, large]) {
      expect(at.height).toBeGreaterThanOrEqual(Math.min(at.scrollHeight, at.maxHeight) - 1);
    }
    await expect(page.getByRole("button", { name: "Show full translation" })).toHaveCount(0);
    await assertContained(page);
  });
}

test("Interface size still scales the translation, independently of Definitions size", async ({
  page,
}) => {
  // **The part the invalid declaration never broke**, and the part a naive fix could: replacing
  // `1rem` with a fixed `px` would satisfy Definitions size and quietly detach the card from the
  // Interface setting. Definitions size is held at one value across the whole test.
  await analyze(page, "&fixture=long-translation");
  await setDefinitionsSize(page, LARGE);

  const sizes = [];
  for (const root of [12, 24]) {
    await page.evaluate((px) => {
      document.documentElement.style.fontSize = `${px}px`;
    }, root);
    await page.evaluate(
      () => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))),
    );
    sizes.push(await rendered(page, "[data-testid=aligned-translation] p"));
  }

  const [small, large] = sizes;
  expect(small.root).toBe(12);
  expect(large.root).toBe(24);
  // **This one held before the fix and has to keep holding.** With the declaration dropped the
  // body inherited a plain `1rem`, so it followed the root then too — which is why "the card
  // ignores Interface size" was the wrong diagnosis. It is here as a guard, not as a repair:
  // writing the size in `px` to satisfy Definitions size would break exactly this.
  expect(large.text / small.text, "the body follows the root").toBeCloseTo(2, 1);
  // **This one is what the fix adds.** The text is half again the root at `--panel-scale: 1.5`,
  // at both root sizes, so the two controls compose instead of one replacing the other.
  expect(small.text / small.root).toBeCloseTo(LARGE, 1);
  expect(large.text / large.root).toBeCloseTo(LARGE, 1);
});
