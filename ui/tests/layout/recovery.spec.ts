import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";

/**
 * The recovery panel: what an interrupted installation offers, and what it says it costs.
 *
 * Four situations that must read differently, because they need different actions — something to
 * finish, a competing newer installation, a record nothing can read, and a restoration that was
 * tried and failed. A single "recovery pending" line would send somebody looking for a decision
 * that does not exist, or hide one that does.
 *
 * Asserted through the rendered page because that is where the distinctions live. The report
 * carries the states; only this can say whether the panel draws them apart.
 */

const panel = (page: Page) => page.locator(".recovery");
const entry = (page: Page, name: string) => page.locator(`[data-recovery="${name}"]`);

async function open(page: Page, recovery: string) {
  await page.goto(`/?qa=analysis&recovery=${recovery}`);
  await expect(panel(page)).toBeVisible();
}

test("an unresolved installation offers both actions and says what keeping costs", async ({ page }) => {
  await open(page, "unresolved");
  const jitendex = entry(page, "jitendex");

  await expect(jitendex).toHaveAttribute("data-state", "unresolved");
  await expect(jitendex.getByRole("button", { name: "Keep current installation" })).toBeVisible();
  await expect(jitendex.getByRole("button", { name: "Restore previous installation" })).toBeVisible();

  // **The consequence is on screen before the button is pressed.** Keeping preserves the live
  // selection and relinquishes protection of the older version — promising that nothing can be
  // lost would be the easier sentence and the wrong one.
  await expect(jitendex.locator(".consequence")).toContainText("stops being protected");
  await expect(jitendex.locator(".consequence")).toContainText("may be removed");
});

test("a competing installation offers only the action that discards nothing live", async ({ page }) => {
  await open(page, "conflicting");
  const jitendex = entry(page, "jitendex");

  await expect(jitendex).toHaveAttribute("data-state", "conflicting");
  await expect(jitendex.getByRole("button", { name: "Keep current installation" })).toBeVisible();
  // Restoring would replace a newer installation with an older one, so it is not offered at all —
  // rather than offered and refused, which is a button that never works.
  await expect(
    jitendex.getByRole("button", { name: "Restore previous installation" }),
  ).toHaveCount(0);
  await expect(jitendex.locator(".what")).toContainText("discard that newer version");
});

test("a record nothing can read offers no action and gives something to copy", async ({ page }) => {
  await open(page, "unreadable");
  const jitendex = entry(page, "jitendex");

  await expect(jitendex).toHaveAttribute("data-state", "unreadable");
  await expect(jitendex.getByRole("button")).toHaveCount(0);
  // Every button here would be a guess about a transaction whose state cannot be established, so
  // what the panel offers instead is the diagnostic, selectable.
  const diagnostic = jitendex.locator("[data-diagnostic]");
  await expect(diagnostic).toContainText("jitendex");
  await expect(diagnostic).toContainText("not valid JSON");
  await expect(diagnostic).toHaveCSS("user-select", "text");
});

/**
 * **A stale action's entry goes, and its explanation stays.**
 *
 * The first version of this test asserted the message against a fixture whose entry *survived* the
 * refresh — a situation staleness cannot produce, because staleness means the record has already
 * gone. So it passed while the real panel showed nothing at all: the message was rendered inside
 * the entry, and the refresh took both. Found by a person pressing the button on 2026-09-10, after
 * this test had been green for a week.
 *
 * The fixture now resolves the container before it throws, exactly as the backend has. Two shapes,
 * because they fail differently: one where a second entry remains, and one where the stale entry
 * was the last thing on screen.
 */
test("a stale action leaves its explanation behind when its entry goes", async ({ page }) => {
  await open(page, "stale");
  const jitendex = entry(page, "jitendex");
  await jitendex.getByRole("button", { name: "Keep current installation" }).click();

  // The entry is gone, because there was nothing left to act on.
  await expect(entry(page, "jitendex")).toHaveCount(0);
  // And the other one is still listed, so this is the case where the panel survives.
  await expect(entry(page, "jmnedict")).toHaveCount(1);

  const outcome = panel(page).locator(".outcome");
  await expect(outcome).toBeVisible();
  await expect(outcome).toContainText("already resolved elsewhere");
  await expect(outcome).toContainText("Showing what is true now");
  // Nothing went wrong, so nothing is drawn as an error: the screen was describing a moment that
  // had passed, which is a different thing from a failure the reader can act on.
  await expect(outcome).not.toHaveClass(/failed/);
  // And it must not claim the action did anything.
  await expect(outcome).not.toContainText("Kept");
  await expect(outcome).not.toContainText("Restored");
});

test("the explanation survives the last entry disappearing", async ({ page }) => {
  await open(page, "stale-last");
  await entry(page, "jitendex")
    .getByRole("button", { name: "Keep current installation" })
    .click();

  // Nothing is outstanding any more — and the panel is still here, for the one thing it has to say.
  await expect(entry(page, "jitendex")).toHaveCount(0);
  await expect(panel(page)).toBeVisible();
  const outcome = panel(page).locator(".outcome");
  await expect(outcome).toContainText("already resolved elsewhere");
  await expect(outcome).not.toHaveClass(/failed/);
  // The heading goes with the list: there are no unfinished installations to head.
  await expect(panel(page).getByRole("heading")).toHaveCount(0);
});

/// It is announced, not merely displayed.
test("the explanation sits in a polite live region that exists beforehand", async ({ page }) => {
  await open(page, "stale-last");
  // **Before the action.** A live region created at the same moment as its text is one some screen
  // readers never announce, so the container has to be on the page already.
  const region = panel(page).locator("[role='status']");
  await expect(region).toHaveCount(1);
  await expect(region).toHaveAttribute("aria-live", "polite");

  await entry(page, "jitendex")
    .getByRole("button", { name: "Keep current installation" })
    .click();
  await expect(region).toContainText("already resolved elsewhere");
});

/// The reader decides when it goes.
test("the explanation is cleared by dismissing it, not by the next refresh", async ({ page }) => {
  await open(page, "stale-last");
  await entry(page, "jitendex")
    .getByRole("button", { name: "Keep current installation" })
    .click();
  await expect(panel(page).locator(".outcome")).toBeVisible();

  await panel(page).getByRole("button", { name: "Dismiss" }).click();
  // With nothing outstanding and nothing left to say, the panel goes entirely.
  await expect(panel(page)).toHaveCount(0);
});

/**
 * A genuine failure keeps its reason, and keeps its entry to retry from.
 *
 * Observed against the real app on 2026-09-10 (checkpoint row 4): a restoration whose generation
 * was missing reported *"the generation g-000000000001 this would restore is no longer on disk"*,
 * in red, with the entry still listed — and the retry then succeeded. This is that, in the panel.
 */
test("an action that genuinely fails keeps its own reason", async ({ page }) => {
  await open(page, "fails");
  const jitendex = entry(page, "jitendex");
  await jitendex.getByRole("button", { name: "Keep current installation" }).click();

  const outcome = panel(page).locator(".outcome");
  await expect(outcome).toHaveClass(/failed/);
  await expect(outcome).toHaveText("Access is denied. (os error 5)");
  await expect(outcome).not.toContainText("object Object");
  // The entry stays, because a failure leaves something to try again.
  await expect(entry(page, "jitendex")).toHaveCount(1);
});

test("a successful action refreshes and the entry is gone", async ({ page }) => {
  await open(page, "unresolved");
  await entry(page, "jitendex")
    .getByRole("button", { name: "Keep current installation" })
    .click();

  // The refresh is the half that matters as much as the action: a panel that acted successfully
  // and went on describing the old state would be as wrong as one that acted on stale information.
  await expect(entry(page, "jitendex")).toHaveCount(0);
  // And the reader is told it worked, rather than watching a row vanish for no stated reason.
  await expect(panel(page).locator(".outcome")).toContainText("Kept the current installation");
});

/**
 * **Restored files and usable files are different claims.**
 *
 * A component put back on disk but not adopted is one the reader will not see until they restart,
 * and reporting that as plain success would be the fix reading as a different bug.
 */
test("a restored component that loaded says it is in use now", async ({ page }) => {
  await open(page, "install-restored");
  const kanji = entry(page, "kanji");

  await expect(kanji).toHaveAttribute("data-state", "restored");
  await expect(kanji.locator(".what")).toContainText("in use now");
  // The claim the panel used to make about everything it restored, and could not support.
  await expect(kanji.locator(".what")).not.toContainText("restart");
});

/**
 * A restoration whose loading failed says so, with the reason.
 *
 * **Not inferred from having called the adoption path.** A reload that ran and produced an empty
 * table looks identical to a successful one from the caller's side, so the outcome is established
 * by asking the handle whether it now holds anything — and this is the answer a reader needs most,
 * because their files are back and the app still cannot use them.
 */
test("a restored component whose loading failed says so and why", async ({ page }) => {
  await open(page, "install-load-failed");
  const kanji = entry(page, "kanji");

  await expect(kanji).toHaveAttribute("data-state", "restored");
  await expect(kanji.locator(".what")).toContainText("loading it did not work");
  await expect(kanji.locator(".what")).toContainText("produced no characters");
  await expect(kanji.locator(".what")).not.toContainText("in use now");
});

/**
 * And an outcome nothing established says that, rather than advising a restart.
 *
 * Telling somebody to restart when nothing knows whether they need to is a guess wearing the
 * clothes of advice.
 */
test("an unestablished loading outcome is not dressed up as a restart", async ({ page }) => {
  await open(page, "install-failed");
  const kanji = entry(page, "kanji");
  await expect(kanji.locator(".what")).not.toContainText("restart");
});

test("a component that could not be put back says where its files are", async ({ page }) => {
  await open(page, "install-failed");
  const kanji = entry(page, "kanji");

  await expect(kanji).toHaveAttribute("data-state", "failed");
  await expect(kanji.locator(".what")).toContainText("could not be put back");
  // The path is the actionable half: a failure nobody can locate is not a report.
  await expect(kanji.locator("[data-diagnostic]")).toContainText("backup");
});

test("nothing outstanding draws no panel at all", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await expect(page.locator(".recovery")).toHaveCount(0);
});
