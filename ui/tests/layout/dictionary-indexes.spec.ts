import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { openSettings } from "./menu";

/**
 * Offering the dictionary-index rebuild to the reader who needs it.
 *
 * # Why this exists
 *
 * The rebuild already existed and was unreachable. Its only surface was the Reference pane's
 * prompt, shown when `needs_gloss_index` is true — and that is false for any dictionary whose
 * gloss bucket exists, however many other buckets it lacks. So a reader whose English search works
 * was never told their definitions were degraded and never offered the repair.
 *
 * The degradation is not abstract: a word spelled a second way is filed as a pointer to the main
 * entry, and without the sequence index the pointer is all that can be shown. Selecting 抜きとる
 * returned an arrow where a definition belongs.
 *
 * # What is covered
 *
 * Availability, progress, failure, refusal, retry, and the state after success — each driven from
 * the query string rather than inferred, because the four outcomes differ in what the reader is
 * told and a test that only saw the happy one would not notice.
 */

const panel = (page: Page) => page.locator(".dictionary-upkeep");
// Anchored: the setting's More is named "More about Update dictionary indexes", and an
// unanchored pattern would find that fold as well as the button.
const update = (page: Page) =>
  page.getByRole("button", { name: /^(Update dictionary indexes|Building…)$/ });
const status = (page: Page) => page.locator(".dictionary-upkeep p[role='status']");

async function openSearchSettings(page: Page, query: string) {
  await page.goto(`/?qa=analysis${query}`);
  await openSettings(page);
  await page.getByRole("button", { name: "Search", exact: true }).click();
}

test("a dictionary missing its sequence index is named, with what rebuilding would gain", async ({
  page,
}) => {
  await openSearchSettings(page, "&upkeep=missing");
  await expect(panel(page)).toContainText("Jitendex.org is missing an index");
  // The reader is told what it costs them and what it will cost to fix — no download, seconds.
  // On the visible line, not only in the More: `toContainText` reads hidden text too, and the
  // point is that nobody has to open anything to learn the fix costs no download.
  await expect(panel(page)).toContainText("definitions for other spellings");
  await expect(panel(page).locator(".note").first()).toContainText("downloads nothing");
  await expect(panel(page).locator(".note").first()).toBeVisible();
  await expect(update(page)).toBeEnabled();
});

test("nothing is offered when every index is present", async ({ page }) => {
  await openSearchSettings(page, "&upkeep=complete");
  await expect(panel(page)).toContainText("Every installed dictionary has all of its indexes");
  await expect(update(page)).toHaveCount(0);
});

test("the button reports progress and then what was built", async ({ page }) => {
  await openSearchSettings(page, "&upkeep=missing&build=ok");
  // **Held open while the in-progress state is checked, then let go.** The mock used to answer
  // 40ms after the click, and under a loaded parallel run the whole build could finish before the
  // assertion first looked — the button was enabled again and the test failed on a race, not on
  // the app. Holding the command makes "Building…" a state that waits to be seen.
  await page.evaluate(() =>
    (
      window as unknown as { __SAKATSU_QA__: { holdNext: (command: string) => void } }
    ).__SAKATSU_QA__.holdNext("build_gloss_index"),
  );
  await update(page).click();
  await expect(update(page)).toHaveText("Building…");
  await expect(update(page)).toBeDisabled();
  expect(
    await page.evaluate(() =>
      (
        window as unknown as { __SAKATSU_QA__: { releaseCommand: (command: string) => boolean } }
      ).__SAKATSU_QA__.releaseCommand("build_gloss_index"),
    ),
    "the rebuild was waiting to be released",
  ).toBe(true);
  await expect(status(page)).toContainText("433,885 entries");
  // Once built, the offer stops being made: the panel re-reads the status rather than assuming.
  await expect(panel(page)).toContainText("Every installed dictionary has all of its indexes");
});

test("a failure is reported and the rebuild can be tried again", async ({ page }) => {
  await openSearchSettings(page, "&upkeep=missing&build=fail");
  await update(page).click();
  await expect(status(page)).toContainText("a shard could not be written");
  await expect(status(page)).toHaveClass(/warn/);
  // Still offered, because retrying is the answer.
  await expect(update(page)).toBeEnabled();
  await expect(panel(page)).toContainText("is missing an index");
});

test("a refusal says the dictionaries are busy rather than that the rebuild failed", async ({
  page,
}) => {
  await openSearchSettings(page, "&upkeep=missing&build=busy");
  await update(page).click();
  await expect(status(page)).toContainText("another resource operation is using the dictionaries");
  await expect(update(page)).toBeEnabled();
});

test("no dictionary installed says so rather than claiming the indexes are fine", async ({
  page,
}) => {
  await openSearchSettings(page, "&upkeep=none");
  await expect(update(page)).toHaveCount(0);
  // **Three states, not two.** An empty status used to render as "every installed dictionary has
  // all of its indexes", which is a claim about dictionaries that do not exist — and the same
  // branch caught a status that could not be read at all.
  await expect(panel(page)).toContainText("No dictionary is installed yet");
  await expect(panel(page)).not.toContainText("has all of its indexes");
});

test("a status that could not be read says so, and can be retried", async ({ page }) => {
  await openSearchSettings(page, "&upkeep=unreadable");
  // **Not "every index is present".** The panel used to render an unread status as good news,
  // and a failed read as the same thing as no dictionaries.
  await expect(panel(page)).toContainText("could not be read");
  await expect(panel(page)).toContainText("the dictionary store could not be read");
  await expect(panel(page)).not.toContainText("has all of its indexes");
  await page.getByRole("button", { name: "Check again" }).click();
  await expect(panel(page)).toContainText("Every installed dictionary has all of its indexes");
});

test("two dictionaries sharing a title are two rows, not one", async ({ page }) => {
  await openSearchSettings(page, "&upkeep=duplicate");
  // Keyed on the title this is a duplicate key; keyed on the install name it is what it is.
  await expect(panel(page).locator("li")).toHaveCount(2);
  await expect(panel(page)).toContainText("2 dictionaries are missing an index");
});

test("a partial rebuild refreshes what did change and still reports the failure", async ({
  page,
}) => {
  await openSearchSettings(page, "&upkeep=missing&build=partial");
  await expect(panel(page).locator("li")).toHaveCount(2);
  await update(page).click();
  // The failure is reported...
  await expect(status(page)).toContainText("JMnedict: a shard could not be written");
  await expect(status(page)).toHaveClass(/warn/);
  // ...and the one that *did* publish is no longer offered, because the status was re-read even
  // though the attempt ended in a rejection.
  await expect(panel(page).locator("li")).toHaveCount(1);
  await expect(panel(page)).toContainText("JMnedict is missing an index");
});

// The cross-surface sharing itself is unit-tested in `src/lib/shared-operation.test.mjs`: it
// cannot be observed from one surface here, because the button disables itself while a build is
// running, and a test that appeared to press twice would only be pressing once.

test("a rebuild started from Reference refreshes the maintenance panel too", async ({ page }) => {
  // **The wiring, not the promise sharing.** Reference offers the same rebuild, and it used to
  // call the bare operation while only Settings called the wrapper that refreshed afterwards — so
  // a rebuild started here published a new index and left the status stale. The refresh now lives
  // inside the shared operation, which is why there is one function and both callers have it.
  await page.goto("/?qa=analysis&fixture=needs-index&upkeep=missing&build=ok");
  await page.getByRole("button", { name: "Reference" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill("eat");
  await search.press("Enter");

  const build = page.getByRole("button", { name: /Build index|Building/ });
  await expect(build).toBeVisible();
  await build.click();
  await expect(page.locator(".reference .notice small")).toContainText("433,885 entries");

  // Now the other surface, which never asked for anything.
  await openSettings(page);
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(panel(page)).toContainText("Every installed dictionary has all of its indexes");
  await expect(update(page)).toHaveCount(0);
});
