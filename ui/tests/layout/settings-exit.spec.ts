import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { openSettings } from "./menu";
import { assertContained } from "./contained";

/**
 * Leaving Settings goes back where you came from.
 *
 * Settings is a page rather than a drawer, so there is no "underneath" to return to on its own,
 * and it is almost always a detour: you are reading, you change one thing, you want the reading
 * back. Sending everyone to the reader would be wrong for the detour that starts in Reference,
 * which is the other common one — so the way out is named after the way in.
 *
 * `App.svelte` records this in one place (`show`) because there are four doors into Settings: the
 * title bar's gear, its section menu, the rail, and the button first run offers. An assignment at
 * each is four chances for one to forget.
 */

const settings = (page: Page) => page.getByRole("heading", { name: "Settings", exact: true });
// The x is `aria-hidden`, so the button has no text at all and its accessible name is the
// thing under test — hence located by class rather than by name.
const leave = (page: Page) => page.locator("button.exit");

/** The x carries no text, so the destination has to survive somewhere a reader can reach. */
test("the way out still names the page it goes back to", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  await expect(leave(page)).toHaveAttribute("aria-label", /Reading$/);
});

test("leaving from the reader returns to the reader", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  await leave(page).click();
  await expect(settings(page)).toHaveCount(0);
  await expect(page.getByPlaceholder("Paste Japanese text")).toBeVisible();
});

/** The case a fixed destination would get wrong. */
test("leaving from Reference returns to Reference, not the reader", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=parts");
  await page.getByRole("button", { name: "Reference" }).click();
  await expect(page.getByRole("heading", { name: "Reference", exact: true })).toBeVisible();

  await openSettings(page);
  await expect(leave(page)).toHaveAttribute("aria-label", /Reference$/);

  await leave(page).click();
  await expect(page.getByRole("heading", { name: "Reference", exact: true })).toBeVisible();
});

/**
 * Opening Settings while already in Settings must not make the way out point at itself.
 *
 * The title bar's section menu can be used from inside, which is exactly the path that would
 * overwrite the recorded page with `settings`.
 */
test("reaching a section from inside Settings leaves the way back alone", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=parts");
  await page.getByRole("button", { name: "Reference" }).click();
  await openSettings(page);
  await expect(leave(page)).toHaveAttribute("aria-label", /Reference$/);

  // Move between sections the way the in-page nav does.
  await page.getByRole("button", { name: "Appearance", exact: true }).click();
  await expect(leave(page)).toHaveAttribute("aria-label", /Reference$/);
});

test("the heading row stays contained with the button on it", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  await assertContained(page);
});

/**
 * **Escape leaves, and leaves for the same place the button does.**
 *
 * It did not, and the code recorded why: Escape is a bindable key, so a handler that fired while a
 * chord recorder was waiting would close the page instead of recording the chord. The guard is the
 * recorders' own flag, which is why the handler lives on the page rather than in `App.svelte`.
 */
test("Escape leaves Settings for the page it was opened from", async ({ page }) => {
  await page.goto("/?qa=reference");
  await page.getByRole("button", { name: "Reference" }).click();
  await openSettings(page);
  await expect(leave(page)).toHaveAttribute("aria-label", /Reference$/);

  await page.keyboard.press("Escape");
  await expect(settings(page)).toHaveCount(0);
  // Back where it came from, not merely back to the reader. The search box rather than a results
  // list: nothing has been searched for, so there is no list to find.
  await expect(page.getByRole("textbox", { name: "Search Reference" })).toBeVisible();
});

test("Escape from the reader's own trip to Settings returns to the reader", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  await page.keyboard.press("Escape");
  await expect(settings(page)).toHaveCount(0);
});

/**
 * **A chord recorder owns Escape while it is waiting.**
 *
 * Escape is a bindable chord. Pressing it at a recorder means "record Escape", and a page that
 * closed instead would make that binding unreachable — the case that kept Escape off this page
 * until now.
 */
test("Escape while recording a chord does not leave the page", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  await page.getByRole("button", { name: "Capture", exact: true }).click();

  // The recorder's button is labelled with the binding it holds, not with a verb — so it is
  // located by its place in the row, and the "press anything…" it changes to is what proves the
  // recorder is actually waiting. Without that check this test passes for the wrong reason: an
  // unstarted recorder leaves `recording` false and the page closes exactly as it should.
  const recorder = page.locator(".bind .current").first();
  await recorder.click();
  await expect(recorder).toHaveText(/press anything/);

  await page.keyboard.press("Escape");
  await expect(settings(page), "the page is still here").toHaveCount(1);
  // And the press reached the recorder, which is the whole reason the page had to ignore it.
  await expect(recorder).not.toHaveText(/press anything/);
});
