import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";

/**
 * A session that cannot save says so once, and confirms each refusal briefly.
 *
 * **Written from a desktop checkpoint, not from taste.** Running the app against a store path
 * occupied by a file — the only way `Store::open` returns `Err` — put three banners on screen
 * saying one thing, the longest of them nested three prefixes deep:
 *
 *     Settings were not saved: nothing can be saved: Nothing can be saved this session: Cannot
 *     create a file when that file already exists. (os error 183)
 *
 * One sentence, written whole in `open_store`, then prefixed again in `commands::unavailable`,
 * then prefixed a third time in `App.svelte`.
 *
 * **Silence was the alternative and was rejected on review.** Suppressing action errors while the
 * standing warning is up would make a click that saved nothing look like a click that worked,
 * which is the exact misunderstanding the warning exists to prevent. So both are kept and the
 * second one is short: the warning explains the session, the notice confirms this action.
 *
 * What is asserted here is what only a rendered page can answer — how many of each appear, and
 * that a refused mark leaves the word alone.
 */

const SENTENCE = /Nothing can be saved this session/;

// Lowercase, and `exact` so it stays that way. The control's text is the `WordStatus` value
// itself — `known` — and the capital K on screen is `text-transform` in CSS. A matcher written
// from the screenshot passes nothing.
const KNOWN = { name: "known", exact: true } as const;

async function degradedReader(page: Page) {
  await page.goto("/?qa=analysis&fixture=store-unavailable");
  await page.getByPlaceholder("Paste Japanese text").fill("気持ちが伝わってきます。");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".reader")).toBeVisible();
}

test("the reason is stated once, however many saves fail", async ({ page }) => {
  await degradedReader(page);

  const warning = page.locator(".warning", { hasText: SENTENCE });
  await expect(warning, "the session's own explanation, in the words Rust wrote").toHaveCount(1);

  // Three refusals. Selecting a word attempts a save of its own — "seen on focus" — so this is
  // already more than three by the time the assertions run, which is the point.
  const words = page.locator("[data-reader-token]");
  for (const index of [0, 1, 2]) {
    await words.nth(index).click();
    await page.getByRole("button", KNOWN).click();
  }

  await expect(warning, "still one standing warning, not one per attempt").toHaveCount(1);
  await expect(
    page.locator(".save-notice"),
    "and one action line, replaced rather than stacked",
  ).toHaveCount(1);
  await expect(page.locator(".save-notice")).toHaveText("Word status wasn't saved");

  // The refusal must not restate the reason: that is what the warning above is for, and repeating
  // it is what the change came from.
  await expect(page.locator(".save-notice")).not.toHaveText(SENTENCE);
  await expect(
    page.locator(".error"),
    "a refusal for the reason already shown is not a new error",
  ).toHaveCount(0);
});

test("a mark that was refused leaves the word unmarked", async ({ page }) => {
  await degradedReader(page);

  const word = page.locator("[data-reader-token]").first();
  const before = await word.getAttribute("class");
  expect(before, "every word starts new when nothing is stored").toContain("status-new");

  await word.click();
  await page.getByRole("button", KNOWN).click();
  await expect(page.locator(".save-notice")).toHaveText("Word status wasn't saved");

  // **The status is read from what came back, and nothing came back.** A word drawn as Known on
  // a save that failed would be the worst outcome available here — worse than an error nobody
  // reads, because it is a lie the reader has no way to detect.
  await expect(word, "unchanged, because the write never landed").toHaveClass(/status-new/);
  await expect(word).not.toHaveClass(/status-known/);
});

/**
 * A store that opened and refused this write keeps its own message.
 *
 * The other half of the distinction, and the one that would be lost by answering every failed save
 * with "not saved". There is no standing warning here — the store opened — so the refusal's own
 * sentence is the only account of it anywhere on screen. Suppressing it would leave a click that
 * did nothing and said nothing, which is the failure the short notice exists to prevent, arriving
 * from the opposite direction.
 *
 * This is what the typed `code` buys. The first version of this logic compared the rejection
 * against the standing warning by string, and with no standing warning to compare against it
 * happened to fall through to the full message — the right answer for the wrong reason. A code
 * says which case this is whether or not anything else is on screen.
 */
test("a refusal that nothing else explains is shown in full", async ({ page }) => {
  await page.goto("/?qa=analysis&fixture=store-refuses");
  await page.getByPlaceholder("Paste Japanese text").fill("気持ちが伝わってきます。");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".reader")).toBeVisible();

  // No standing warning: this store opened.
  await expect(page.locator(".warning", { hasText: SENTENCE })).toHaveCount(0);

  const word = page.locator("[data-reader-token]").first();
  await word.click();
  await page.getByRole("button", KNOWN).click();

  await expect(page.locator(".error")).toHaveText("Access is denied. (os error 5)");
  await expect(
    page.locator(".save-notice"),
    "the short line is for the case a warning already explains, and there is no warning here",
  ).toHaveCount(0);

  // The same guarantee as the unavailable case, by the same mechanism: the status is written from
  // what came back, and nothing came back.
  await expect(word).toHaveClass(/status-new/);
  await expect(word).not.toHaveClass(/status-known/);
});

/**
 * And `[object Object]` never reaches the screen.
 *
 * The rejection is a `PersistenceError` now, not a string, so the `String(thrown)` that every
 * catch block used would render the shape rather than the sentence. Cheap to assert and the exact
 * thing a reader would report as gibberish.
 */
test("a typed rejection renders as its sentence, not as its shape", async ({ page }) => {
  await page.goto("/?qa=analysis&fixture=store-refuses");
  await page.getByPlaceholder("Paste Japanese text").fill("気持ちが伝わってきます。");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await page.locator("[data-reader-token]").first().click();
  await page.getByRole("button", KNOWN).click();

  await expect(page.locator(".error")).not.toContainText("object Object");
  await expect(page.locator(".error")).not.toContainText("undefined");
});
