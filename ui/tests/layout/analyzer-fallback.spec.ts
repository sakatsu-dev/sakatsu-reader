import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { assertContained } from "./contained";

/**
 * **Which analyzer read the passage, said as loudly as the record warrants.**
 *
 * Core records every passage's execution — the policy, the engine that produced it, and any
 * fallback, passage- or session-scoped. The reader says so: prominently when Ichiran failed on
 * this passage or was asked for by name and is unavailable, only as a quiet label when Automatic
 * simply has no Ichiran this session, and not at all while it is still starting. The QA harness
 * supplies each record through `?fallback=`.
 */
const passage = "気持ちが伝わってきます。";

async function read(page: Page, query = "") {
  await page.goto(`/?qa=analysis${query}`);
  await page.getByPlaceholder("Paste Japanese text").fill(passage);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
}

const notice = (page: Page) => page.getByTestId("engine-notice");
const label = (page: Page) => page.getByTestId("engine-label");

test("a passage Ichiran read says nothing about analyzers", async ({ page }) => {
  await read(page);
  await expect(notice(page)).toHaveCount(0);
  await expect(label(page)).toHaveCount(0);
});

test("a passage Ichiran failed on names the fallback and keeps the detail behind a disclosure", async ({ page }) => {
  await read(page, "&fallback=passage");
  await expect(notice(page)).toContainText(
    "Using Lindera because Ichiran could not analyze this passage.",
  );
  const detail = notice(page).locator("details");
  await expect(detail.locator("p")).toBeHidden();
  await detail.locator("summary").click();
  await expect(detail.locator("p")).toContainText("did not answer within the budget");
  // The passage itself is there and readable underneath.
  await expect(page.locator(".reader")).toContainText("伝わってきます");
  await assertContained(page);
});

test("Ichiran still starting says nothing about analyzers", async ({ page }) => {
  await read(page, "&fallback=starting");
  await expect(notice(page)).toHaveCount(0);
  await expect(label(page)).toHaveCount(0);
  await expect(page.getByText(/Ichiran is starting/)).toHaveCount(0);
  await assertContained(page);
});

test("an Ichiran asked for by name and unavailable says so on every passage", async ({ page }) => {
  await read(page, "&fallback=session-ichiran");
  await expect(notice(page)).toContainText("Ichiran is unavailable");
  await expect(notice(page)).toHaveClass(/warning/);
});

test("Automatic with no Ichiran this session shows only a quiet label", async ({ page }) => {
  await read(page, "&fallback=session-auto");
  await expect(notice(page)).toHaveCount(0);
  await expect(label(page)).toHaveText("Built-in analyzer");
  // The reason is still there for anyone who looks.
  await expect(label(page)).toHaveAttribute("title", /could not run wsl/);
});

test("when both engines fail the previous passage stays and both failures are named", async ({ page }) => {
  await read(page, "&reject-second-study=both");
  await page.getByRole("button", { name: "Edit" }).click();
  await page.getByPlaceholder("Paste Japanese text").fill("神姫");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  const error = page.locator(".error");
  await expect(error).toContainText("ichiran could not analyze this passage");
  await expect(error).toContainText("lindera could not either");
  await expect(page.locator(".reader")).toContainText("伝わってきます");
  await expect(page.locator(".reader")).not.toContainText("神姫");
  await assertContained(page);
});
