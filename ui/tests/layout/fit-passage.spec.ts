import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";

/**
 * "Fit passage" appears when it would change something, and not otherwise.
 *
 * It used to be shown for as long as the sizing was manual, which is a different question: a
 * hand-dragged split that happens to show the whole passage still carried a button that would do
 * nothing. The condition is now the reading pane's own overflow — `.column-body` is the scroller,
 * so content taller than its box is exactly a passage with a line out of sight.
 */

const fit = (page: Page) => page.getByRole("button", { name: "Fit passage" });

/** Drag the outer splitter by `dy`, which is what puts the sizing into manual. */
async function dragSplit(page: Page, dy: number) {
  const handle = page.locator(".analysis-workspace [role=separator]").first();
  const box = await handle.boundingBox();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2 + dy, { steps: 8 });
  await page.mouse.up();
}

async function analyze(page: Page, text: string, fixture = "") {
  await page.goto(`/?qa=analysis${fixture ? `&fixture=${fixture}` : ""}`);
  await page.getByPlaceholder("Paste Japanese text").fill(text);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
}

test("auto sizing never offers it", async ({ page }) => {
  await analyze(page, "気持ちが伝わってきます。");
  await expect(fit(page)).toHaveCount(0);
});

test("a short passage that still fits after a drag does not offer it", async ({ page }) => {
  await analyze(page, "気持ちが伝わってきます。");
  // Grow the reading pane. One short line cannot be clipped by more room.
  await dragSplit(page, 60);
  await page.waitForTimeout(200);
  const clipped = await page.evaluate(() => {
    const pane = document.querySelector<HTMLElement>(".passage-column .column-body")!;
    return pane.scrollHeight > pane.clientHeight + 1;
  });
  expect(clipped, "a grown pane must not be clipping").toBe(false);
  await expect(fit(page)).toHaveCount(0);
});

/**
 * The branch that matters most: the button must still be there when it would do something.
 *
 * `fixture=long-passage` repeats the seed twelve times so the reader has more than a screenful.
 * Without it the mock returns the same four tokens whatever is submitted, and the reading pane's
 * `--reading-floor` is taller than one line — so nothing can be made to clip and this would pass
 * for the wrong reason.
 */
test("a passage squeezed until it clips offers it again", async ({ page }) => {
  await analyze(page, "気持ちが伝わってきます。".repeat(12), "long-passage");
  await dragSplit(page, -220);
  await page.waitForTimeout(200);
  const clipped = await page.evaluate(() => {
    const pane = document.querySelector<HTMLElement>(".passage-column .column-body")!;
    return pane.scrollHeight > pane.clientHeight + 1;
  });
  expect(clipped, "the squeeze has to actually clip, or this proves nothing").toBe(true);
  await expect(fit(page)).toBeVisible();
});
