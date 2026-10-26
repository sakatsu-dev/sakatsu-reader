import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { gear } from "./menu";

/**
 * **Where the passage's lines start, chosen from the quick panel.**
 *
 * Asserted on where the words actually land, not on the `text-align` value: a rule that is set and
 * does nothing is the failure `traps.md` records twice, and only positions can tell the three
 * choices apart. Measured on the passage's **last** visual row, because a full row looks the same
 * whichever way it is aligned — the last one is where the slack is.
 */

async function openReader(page: Page) {
  await page.goto("/?qa=analysis");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".reader .token").first()).toBeVisible();
}

/** The gaps either side of the last visual row's words, inside the line that holds them. */
async function lastRowGaps(page: Page) {
  return page.evaluate(() => {
    const lines = [...document.querySelectorAll(".reader .line")];
    const line = lines[lines.length - 1] as HTMLElement;
    const box = line.getBoundingClientRect();
    const words = [...line.querySelectorAll(".token, .punctuation")].map((el) =>
      el.getBoundingClientRect(),
    );
    // The row is everything that shares height with its lowest box. **Overlap, not equal bottoms**:
    // a full stop sits higher than the word before it — 5px at reader size — and a bottom-edge
    // tolerance dropped it, which put the row's end one character early.
    const lowest = words.reduce((a, b) => (b.bottom > a.bottom ? b : a));
    const row = words.filter((rect) => rect.top < lowest.bottom && rect.bottom > lowest.top);
    return {
      left: Math.min(...row.map((rect) => rect.left)) - box.left,
      right: box.right - Math.max(...row.map((rect) => rect.right)),
    };
  });
}

async function choose(page: Page, label: string) {
  await gear(page).click();
  await page.getByRole("combobox", { name: "Align text" }).selectOption({ label });
  await page.keyboard.press("Escape");
}

test("the passage starts where the quick panel says, from the left by default", async ({
  page,
}) => {
  await openReader(page);

  const left = await lastRowGaps(page);
  expect(left.left, "left by default: the row starts at the line's edge").toBeLessThan(4);
  // The comparison below needs slack to mean anything.
  expect(left.right, "the last row leaves room to move").toBeGreaterThan(24);

  await choose(page, "Center");
  const center = await lastRowGaps(page);
  expect(Math.abs(center.left - center.right), "centred: equal room either side").toBeLessThan(4);
  expect(center.left).toBeGreaterThan(left.left + 8);

  await choose(page, "Right");
  const right = await lastRowGaps(page);
  expect(right.right, "right: the row ends at the line's edge").toBeLessThan(4);
  expect(right.left).toBeGreaterThan(center.left + 8);

  await choose(page, "Left");
  expect((await lastRowGaps(page)).left, "and back").toBeLessThan(4);
});

test("the alignment is the reader's alone, so other pages do not offer it", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  await expect(page.getByRole("combobox", { name: "Align text" })).toBeVisible();
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Reference", exact: true }).click();
  await gear(page).click();
  await expect(page.getByRole("combobox", { name: "Align text" })).toHaveCount(0);
});

test("a saved alignment is what the passage opens with", async ({ page }) => {
  await page.goto("/?qa=analysis&align=center");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".reader .token").first()).toBeVisible();
  const gaps = await lastRowGaps(page);
  expect(Math.abs(gaps.left - gaps.right)).toBeLessThan(4);
  expect(gaps.left).toBeGreaterThan(8);

  await gear(page).click();
  await expect(page.getByRole("combobox", { name: "Align text" })).toHaveValue("center");
});
