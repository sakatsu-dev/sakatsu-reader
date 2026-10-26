import type { Locator, Page } from "@playwright/test";
import { expect } from "./fixtures";

/**
 * Reaching the app's menu, and the settings page through it.
 *
 * **Shared because four specs walk this path** — `app-menu`, `binding`, `kind-hues` and
 * `settings-exit` — and each held its own copy. When the gear's menu absorbed the appearance
 * controls, the row that opens the settings page stopped being called "Settings" and became
 * "All settings…", which would have been four separate edits and four chances to miss one. The
 * route is one thing, so it is written once.
 */

/** The gear. It is the app's whole menu now: appearance, then Settings, Help and Restart. */
export const gear = (page: Page): Locator =>
  page.getByRole("button", { name: "Settings", exact: true });

/** The menu it drops, or nothing when it is shut. */
export const menu = (page: Page): Locator => page.locator(".menu");

/** A settings row inside the menu — a label and its control, not a destination. */
export const menuRows = (page: Page): Locator => page.locator(".menu .row");

/**
 * Open the settings page the way a reader does.
 *
 * **The gear is a menu, not a link.** A single press on its accessible name opens the menu and
 * navigates nowhere; the row inside is what gets you to the page. That is how this spec first
 * failed, and it is why the two presses are spelled out here rather than assumed.
 */
export async function openSettings(page: Page) {
  await gear(page).click();
  await page.getByRole("button", { name: "All settings…" }).click();
  await expect(page.getByRole("heading", { name: "Settings", exact: true })).toBeVisible();
}

/**
 * Open Help the way a reader does — the same two presses as `openSettings`, and for the same
 * reason: the gear is a menu, and the row inside it is what navigates.
 */
export async function openHelp(page: Page) {
  await gear(page).click();
  await page.getByRole("button", { name: "Help", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Help", exact: true })).toBeVisible();
}
