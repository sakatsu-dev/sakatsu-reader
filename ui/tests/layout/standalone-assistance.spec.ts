import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";

async function read(page: Page, text: string, query = "") {
  await page.goto(`/?qa=analysis${query}`);
  await page.getByPlaceholder("Paste Japanese text").fill(text);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
}

test("component definitions identify the component without changing the selected word", async ({ page }) => {
  await read(page, "神姫", "&component=神");
  const card = page.getByTestId("selected-word-card");
  await expect(page.getByTestId("selected-word-header")).toContainText("神姫");
  await expect(card.locator(".component-scope").first()).toHaveText("For the component 神");
  await expect(card).toContainText("god; deity; divinity");
  await expect(card).not.toContainText("Likely here");
  await card.getByRole("button", { name: "Known" }).click();
  await expect.poll(() => page.evaluate(() =>
    (window as Window & { __SAKATSU_QA__?: { statusCalls: { surface: string; dict_form: string }[] } })
      .__SAKATSU_QA__?.statusCalls.at(-1),
  )).toMatchObject({ surface: "神姫", dict_form: "神姫" });
});

for (const [surface, leading, alternative] of [
  ["たち", "pluralizing suffix", "long sword"],
  ["なん", "what; why", "difficulty; trouble"],
] as const) {
  test(`${surface} leads with its morphological sense and retains the alternative`, async ({ page }) => {
    await read(page, surface, "&fixture=lexical-role");
    const card = page.getByTestId("selected-word-card");
    await expect(card).toContainText(leading);
    await expect(card.getByText(alternative, { exact: true })).not.toBeVisible();
    await card.getByText("1 more sense", { exact: true }).click();
    await expect(card.getByText(alternative, { exact: true })).toBeVisible();
  });
}

test("a rejected new analysis leaves the last successful passage and explains the failure", async ({ page }) => {
  const previous = "気持ちが伝わってきます。";
  await read(page, previous, "&reject-second-study=1");
  await page.getByRole("button", { name: "Edit" }).click();
  await page.getByPlaceholder("Paste Japanese text").fill("神姫");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".error")).toContainText("invalid source spans");
  await expect(page.locator(".reader")).toContainText("気持ち");
  await expect(page.locator(".reader")).toContainText("伝わってきます");
  await expect(page.locator(".reader")).not.toContainText("神姫");
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
});
