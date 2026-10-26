import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";

/**
 * Request ordering, driven through the surface the controller actually guards.
 *
 * # Why this exists beside the controller tests
 *
 * `ui/src/lib/operation-controller.test.mjs` already proves the ordering semantics with controlled
 * promises: a replaced request's success, rejection and `finally` are all inert. What it cannot
 * prove is that `App.svelte` is *wired to* that controller on the path a reader takes — the
 * controller could be correct and the surface still painted from a stale answer by some other
 * route. That is the gap this closes.
 *
 * The ordering is chosen, not raced. `?hold-lookup=1` makes the QA harness hold each lookup open,
 * keyed by the word that asked for it, and `releaseRequest` finishes exactly one — so "B answers
 * before A" is arranged rather than hoped for. Manual speed proves nothing about ordering.
 *
 * # Why lookup rather than analysis
 *
 * Found by trying analysis first: **the reader cannot start a replacing analysis at all.** The
 * Analyze button is `disabled={busy}`, and once a passage has been analysed the input becomes a
 * read-only source line that needs an explicit edit. So for `study`, replacement is prevented at
 * the control rather than handled at the controller, and a test driving A→B through it can only
 * time out. Lookup is where replacement genuinely happens: the reader's tokens are ordinary
 * buttons with no disabled state, so clicking a second word while the first is in flight is one
 * gesture.
 *
 * The header follows the selection immediately; only the body waits for the lookup. So a stale
 * answer shows up as the **wrong meaning under the right word**, which is what these assert on.
 *
 * # What is deliberately not here
 *
 * **Disposal.** The seven controllers in `App.svelte` live as long as the window, so no reader
 * action disposes one; `operation-controller.test.mjs` covers disposal directly. A test here would
 * have to fake a teardown the product never performs.
 */

const PASSAGE = "気持ちが伝わってきます。";
/** The two meanings the fixture gives these words — distinct, so a stale paint is visible. */
const FEELING = "feeling; state of mind; sensation";
const CONVEYED = "to be conveyed; to come across";
/** Held-request keys, as `hold-lookup` records them. */
const KIMOCHI = "lookup:気持ち";
const TSUTAWARU = "lookup:伝わってきます";

type Harness = {
  releaseRequest: (key: string, how?: "complete" | "fail") => boolean;
  pendingRequests: () => string[];
};

const reach = (page: Page) =>
  page.evaluate(() => (window as Window & { __SAKATSU_QA__?: Harness }).__SAKATSU_QA__?.pendingRequests() ?? []);

async function release(page: Page, key: string, how: "complete" | "fail" = "complete") {
  const letGo = await page.evaluate(
    ([name, mode]) =>
      (window as Window & { __SAKATSU_QA__?: Harness }).__SAKATSU_QA__?.releaseRequest(
        name as string,
        mode as "complete" | "fail",
      ),
    [key, how],
  );
  expect(letGo, `nothing was holding ${key}`).toBe(true);
}

const card = (page: Page) => page.getByTestId("selected-word-card");
const words = (page: Page) => page.locator(".reader button");

/**
 * Analyse the passage and settle the lookup the analysis selects for itself.
 *
 * The reader selects a word as soon as it has one, so a lookup is already held before any test
 * clicks anything. Letting it finish first means each test starts from a settled card.
 */
async function ready(page: Page) {
  await page.goto("/?qa=analysis&hold-lookup=1");
  await page.getByPlaceholder("Paste Japanese text").fill(PASSAGE);
  await page.getByRole("button", { name: /^Analyz/ }).click();
  await expect.poll(() => reach(page)).not.toHaveLength(0);
  for (const key of await reach(page)) await release(page, key);
  await expect(card(page)).toBeVisible();
  await expect(card(page)).not.toContainText("Looking up…");
}

/** Click two words in turn, leaving both lookups held: the first is replaced by the second. */
async function replaceLookup(page: Page) {
  await words(page).nth(2).click();
  await expect.poll(() => reach(page)).toContain(TSUTAWARU);
  await words(page).nth(0).click();
  await expect.poll(() => reach(page)).toContain(KIMOCHI);
}

test("a replaced lookup never paints, however late it answers", async ({ page }) => {
  await ready(page);
  await replaceLookup(page);

  await release(page, KIMOCHI);
  await expect(card(page)).toContainText(FEELING);

  // The replaced word answers now. It must reach nothing.
  await release(page, TSUTAWARU);
  await expect(card(page)).toContainText(FEELING);
  await expect(card(page)).not.toContainText(CONVEYED);
});

test("a stale answer finishing first leaves the indicator to the request that owns it", async ({
  page,
}) => {
  await ready(page);
  await replaceLookup(page);
  await expect(card(page)).toContainText("Looking up…");

  // The *replaced* lookup answers first. Its `finally` must not report the card settled: the
  // replacement is still running, and an indicator cleared here says an answer has arrived.
  await release(page, TSUTAWARU);
  await expect(card(page)).toContainText("Looking up…");
  await expect(card(page)).not.toContainText(CONVEYED);

  await release(page, KIMOCHI);
  await expect(card(page)).toContainText(FEELING);
  await expect(card(page)).not.toContainText("Looking up…");
});

test("a late older answer cannot overwrite the current failure", async ({ page }) => {
  await ready(page);
  await replaceLookup(page);

  await release(page, KIMOCHI, "fail");
  await expect(page.locator(".error")).toBeVisible();

  // The replaced lookup succeeds after the replacement has already failed. Painting it would show
  // a meaning over the top of the reason there is none.
  await release(page, TSUTAWARU);
  await expect(page.locator(".error")).toBeVisible();
  await expect(card(page)).not.toContainText(CONVEYED);
});

test("clearing is refused while an analysis is in flight rather than racing it", async ({
  page,
}) => {
  await page.goto("/?qa=analysis&hold-study=1");
  await page.getByPlaceholder("Paste Japanese text").fill(PASSAGE);
  await page.getByRole("button", { name: /^Analyz/ }).click();
  await expect.poll(() => reach(page)).toContain(`study:${PASSAGE}`);

  // Recorded because it decides what the ordering rules have to cover: the reader cannot be
  // cleared or re-analysed mid-analysis at all, so for `study` those races are prevented here
  // rather than handled downstream.
  await expect(page.getByRole("button", { name: "Clear" })).toBeDisabled();
  await expect(page.getByRole("button", { name: /^Analyzing/ })).toBeDisabled();

  await release(page, `study:${PASSAGE}`);
  await expect(page.getByRole("button", { name: "Clear" })).toBeEnabled();
});
