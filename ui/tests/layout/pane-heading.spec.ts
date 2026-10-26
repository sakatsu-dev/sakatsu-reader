import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";

/**
 * The pane headings are the tab strip the narrow layout already uses.
 *
 * `.panel-switch` draws an uppercase label, an accent underline and a hairline under the row
 * whenever the card is under 42rem. The wide layout now draws the same thing, so the two differ
 * only where they must — which is the accent's meaning. There it marks the *selected* tab,
 * because one panel shows at a time. Here both show, so both are marked.
 *
 * **Asserted against the narrow layout's own computed values, not against a hex.** The palette is
 * generated and the reader can re-hue it, so a literal colour would be testing this machine's
 * theme. Comparing the two layouts is what actually holds them together.
 */

/** The split layout needs room; below 42rem the card switches to real tabs. */
const WIDE = { width: 1100, height: 700 };

async function analysed(page: Page, query = "") {
  await page.goto(`/?qa=analysis${query}`);
  await page.getByPlaceholder("Paste Japanese text").fill("気持ちが伝わってきます。");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
}

/** What the narrow layout paints on the tab that is currently showing. */
async function currentTabLook(page: Page) {
  return page.evaluate(() => {
    const tab = document.querySelector(".panel-switch button.current")!;
    const style = getComputedStyle(tab);
    return { color: style.color, underline: style.borderBottomColor, width: style.borderBottomWidth };
  });
}

test.describe("the wide headings", () => {
  test.use({ viewport: WIDE });

  test("both are marked, not just one", async ({ page }, info) => {
    test.skip(info.project.name !== "edge-900-100", "one project drives the wide layout");
    await analysed(page);
    const seen = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>(".pane-name")].map((el) => {
        const style = getComputedStyle(el);
        return {
          text: el.textContent?.trim(),
          color: style.color,
          underline: style.borderBottomColor,
          width: style.borderBottomWidth,
        };
      }),
    );
    expect(seen.map((h) => h.text)).toEqual(["Vocabulary", "Grammar"]);
    for (const heading of seen) {
      // The label and its underline are the same colour: a marked tab, not a heading with a rule.
      expect(heading.underline, `${heading.text} underline matches its label`).toBe(heading.color);
      expect(heading.width, `${heading.text} underline weight`).toBe("2px");
    }
  });

  test("the mark is the same one the narrow tabs use", async ({ page }, info) => {
    test.skip(info.project.name !== "edge-900-100", "this one compares the two layouts");
    await analysed(page);
    const wide = await page.evaluate(() => {
      const style = getComputedStyle(document.querySelector(".pane-name")!);
      return { color: style.color, underline: style.borderBottomColor, width: style.borderBottomWidth };
    });

    // Same page, narrowed until the card switches to the real tab strip.
    await page.setViewportSize({ width: 560, height: 700 });
    await expect(page.locator(".panel-switch button.current")).toBeVisible();
    const narrow = await currentTabLook(page);

    expect(wide, "the wide heading wears the narrow layout's current-tab mark").toEqual(narrow);
  });

  test("the row keeps a hairline under it", async ({ page }, info) => {
    test.skip(info.project.name !== "edge-900-100", "one project drives the wide layout");
    await analysed(page);
    const row = await page.evaluate(() => {
      const style = getComputedStyle(document.querySelector(".pane-head")!);
      return { width: style.borderBottomWidth, colour: style.borderBottomColor };
    });
    // 2px of accent over 1px of border is what makes a tab strip read as one.
    expect(row.width).toBe("1px");
  });

  test("the narrow layout has no such heading at all", async ({ page }, info) => {
    test.skip(info.project.name !== "edge-900-100", "one project drives the sweep");
    await analysed(page);
    await page.setViewportSize({ width: 560, height: 700 });
    await expect(page.locator(".pane-name")).toHaveCount(0);
    await expect(page.getByRole("tablist", { name: "Analysis details" })).toHaveCount(1);
  });
});
/**
 * "Reset details" answers the panes, so it sits with them rather than in the identity strip.
 *
 * **The point of the move is that the strip stops changing height.** It used to share the
 * header's message row, which appeared the moment any disclosure was opened and vanished again
 * on reset — so the one part of the card that should never move moved as a side effect of
 * something happening below it.
 */
test.describe("the reset control", () => {
  test.use({ viewport: WIDE });

  test("opening a disclosure leaves the identity strip exactly where it was", async ({ page }, info) => {
    test.skip(info.project.name !== "edge-900-100", "the wide layout is the one with room for it");
    await page.goto("/?qa=analysis&fixture=long");
    await page.getByPlaceholder("Paste Japanese text").fill("気持ちが伝わってきます。");
    await page.getByRole("button", { name: /^Analyze/ }).click();
    const card = page.getByTestId("selected-word-card");
    await expect(card).toBeVisible();

    const strip = page.getByTestId("selected-word-header");
    const before = await strip.boundingBox();

    await card.getByText("5 more senses", { exact: true }).click();
    await expect(card.getByRole("button", { name: "Reset details" })).toBeVisible();
    // The height can only be proven to hold if something actually changed underneath it.
    await expect(card.locator("details[open]")).not.toHaveCount(0);

    const after = await strip.boundingBox();
    expect(after!.height, "the strip must not grow a row").toBeCloseTo(before!.height, 1);
    expect(after!.y, "nor move").toBeCloseTo(before!.y, 1);
  });

  test("it sits at the right end of the headings row", async ({ page }, info) => {
    test.skip(info.project.name !== "edge-900-100", "the wide layout is the one with room for it");
    await page.goto("/?qa=analysis&fixture=long");
    await page.getByPlaceholder("Paste Japanese text").fill("気持ちが伝わってきます。");
    await page.getByRole("button", { name: /^Analyze/ }).click();
    const card = page.getByTestId("selected-word-card");
    await card.getByText("5 more senses", { exact: true }).click();

    const reset = await card.getByRole("button", { name: "Reset details" }).boundingBox();
    const heading = await page.locator(".pane-name").last().boundingBox();
    const cardBox = await card.boundingBox();

    // On the headings' line, not above or below them.
    expect(reset!.y).toBeLessThan(heading!.y + heading!.height);
    expect(reset!.y + reset!.height).toBeGreaterThan(heading!.y);
    // And at the end of the row rather than in the middle of it.
    const rightEdge = cardBox!.x + cardBox!.width;
    expect(rightEdge - (reset!.x + reset!.width)).toBeLessThan(24);
  });
});
