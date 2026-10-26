import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { openHelp, openSettings } from "./menu";
import { assertContained } from "./contained";

/**
 * **Leaving Help goes back where you came from — the scheme Settings already uses.**
 *
 * Help is the same kind of page: you are reading, you want to know what a mark means, you want the
 * reading back. It reached the app before that scheme existed and kept the only way out the rail
 * offered, which meant the one page you open when you are lost was the one page with no marked way
 * back. It has the × and it answers to Escape, like its sibling.
 *
 * The two differ in exactly one term. Settings stands its Escape handler down while a chord
 * recorder is armed, because Escape is a bindable key; Help has no bindable input, so there is
 * nothing to stand down for. `settings-exit.spec.ts` covers that half.
 */

const help = (page: Page) => page.getByRole("heading", { name: "Help", exact: true });
// The × is `aria-hidden`, so the button has no text and its accessible name is the thing under
// test — located by class, as in `settings-exit.spec.ts`.
const leave = (page: Page) => page.locator("button.exit");

test("Help offers a way out that names where it goes", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openHelp(page);
  await expect(leave(page)).toHaveCount(1);
  await expect(leave(page)).toHaveAttribute("aria-label", /Reading$/);
});

test("the button returns to the reader it was opened from", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openHelp(page);
  await leave(page).click();
  await expect(help(page)).toHaveCount(0);
  await expect(page.getByPlaceholder("Paste Japanese text")).toBeVisible();
});

test("Escape leaves Help for the same place the button does", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openHelp(page);
  await page.keyboard.press("Escape");
  await expect(help(page)).toHaveCount(0);
  await expect(page.getByPlaceholder("Paste Japanese text")).toBeVisible();
});

/** The case a fixed destination would get wrong. */
test("leaving from Reference returns to Reference, not the reader", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=parts");
  await page.getByRole("button", { name: "Reference" }).click();
  await openHelp(page);
  await expect(leave(page)).toHaveAttribute("aria-label", /Reference$/);

  await page.keyboard.press("Escape");
  await expect(page.getByRole("heading", { name: "Reference", exact: true })).toBeVisible();
});

/**
 * **A detour must not record another detour.** Help is reachable from inside Settings, which is
 * the path that would overwrite the remembered page with `Settings` and turn the way out into the
 * way back in. `cameFrom` should still name a page somebody was actually reading.
 */
test("reaching Help from inside Settings still returns to the reading", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=parts");
  await page.getByRole("button", { name: "Reference" }).click();
  await openSettings(page);
  await openHelp(page);
  await expect(leave(page)).toHaveAttribute("aria-label", /Reference$/);

  await page.keyboard.press("Escape");
  await expect(page.getByRole("heading", { name: "Reference", exact: true })).toBeVisible();
});

test("the heading row stays contained with the button on it", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openHelp(page);
  await assertContained(page);
});
