import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { assertContained } from "./contained";

/**
 * **A rejoined contraction is explained from its base form.**
 *
 * The ladder the Grammar pane draws here is core's own output: `contraction_repair.rs` writes the
 * `StudyView` breakdowns of 間違っとる and 怒んなきゃ into `src/qa/contraction-breakdowns.json` and
 * compares them byte for byte, and the QA harness serves that file for `fixture=contraction`. So
 * this pins what a reader sees for what core actually produces: the base form with its formation,
 * then the contraction with the uncontracted form it stands for.
 */
async function explain(page: Page, word: string) {
  await page.goto("/?qa=analysis&fixture=contraction");
  await page.getByPlaceholder("Paste Japanese text").fill(word);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  if (await page.getByRole("tablist", { name: "Analysis details" }).count()) {
    await page.getByRole("tab", { name: "Grammar" }).click();
  }
  return page.getByTestId("selected-word-card");
}

test("a contracted ～ておる is explained as the te-form, then the contraction", async ({ page }) => {
  const card = await explain(page, "間違っとる");
  await expect(card.getByText("Te-form", { exact: true })).toBeVisible();
  await expect(card.getByText("Contracted ～ておる (～とる)", { exact: true })).toBeVisible();
  const formations = card.locator(".formation");
  await expect(formations).toHaveCount(2);
  await expect(formations.nth(0)).toContainText("まちがう → まちがって");
  await expect(formations.nth(1)).toContainText("まちがっておる → まちがっとる");
  await expect(formations.nth(1)).toContainText("ておる → とる");
  await assertContained(page);
});

test("a nasalised ～なきゃ is explained as the negative provisional, then the contraction", async ({ page }) => {
  const card = await explain(page, "怒んなきゃ");
  await expect(card.getByText("Provisional (〜ば)", { exact: true })).toBeVisible();
  await expect(card.locator(".flag.negative")).toHaveCount(1);
  await expect(card.getByText("Contracted ～なければ (～なきゃ)", { exact: true })).toBeVisible();
  const formations = card.locator(".formation");
  await expect(formations).toHaveCount(2);
  await expect(formations.nth(0)).toContainText("おこる → おこらなければ");
  await expect(formations.nth(1)).toContainText("おこらなければ → おこんなきゃ");
  await expect(formations.nth(1)).toContainText("なければ → なきゃ, ら → ん");
  await assertContained(page);
});
