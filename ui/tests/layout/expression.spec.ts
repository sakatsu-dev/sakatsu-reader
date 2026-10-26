import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { assertContained } from "./contained";

const passage = "気持ちが伝わってきます。";

/**
 * The four relationship labels, and the caveat, written once.
 *
 * They are assertions about exact wording, not about phrasing that happens to contain a word:
 * "Expression." is a prefix of nothing here only because `WHOLE` is matched exactly, and
 * `CONTAINING` would otherwise match it.
 */
const CONTAINING = "Expression containing this selection.";
const WITHIN = "Expression within this selection.";
const OVERLAPPING = "Expression overlapping this selection.";
const WHOLE = "Expression.";
const CAVEAT = "Dictionary senses; no contextual preference established.";

/**
 * The expression offer, rendered.
 *
 * The core tests prove the offer is *computed* correctly. These prove the reader can actually
 * reach it — which is the whole point of the slice, and the part a core test cannot see.
 */
async function analyze(page: Page, query: string) {
  await page.goto(`/?qa=analysis${query}`);
  await page.getByPlaceholder("Paste Japanese text").fill(passage);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  // The card is unified at this width and tabbed at narrower ones, so the tab is opened only
  // where there is one — the same conditional `analysis.spec.ts` uses.
  if (await page.getByRole("tablist", { name: "Analysis details" }).count()) {
    await page.getByRole("tab", { name: "Grammar" }).click();
  }
}

test("either component of a recognised expression reaches the expression's entry", async ({ page }) => {
  await analyze(page, "&expression=offer");

  // Token 0 — the first component — is selected by default, and the match covers 0..2.
  await expect(page.getByText("のに", { exact: true })).toBeVisible();
  await expect(page.getByText("although; despite; even though")).toBeVisible();
  // Every sense, and **no favourite**: the matcher does not know which applies.
  await expect(page.getByText("in order to")).toBeVisible();
  await expect(page.getByText(CAVEAT)).toBeVisible();

  // The *other* component reaches the same offer. A feature that only worked from the first token
  // would leave half the selections a reader actually makes with nothing.
  await page.locator(".token").nth(1).click();
  if (await page.getByRole("tablist", { name: "Analysis details" }).count()) {
    await page.getByRole("tab", { name: "Grammar" }).click();
  }
  await expect(page.getByText(CONTAINING)).toBeVisible();
  await expect(page.getByText("although; despite; even though")).toBeVisible();

  // And a token the match does not cover is offered nothing, so the offer follows the
  // construction rather than the sentence.
  await page.locator(".token").nth(2).click();
  if (await page.getByRole("tablist", { name: "Analysis details" }).count()) {
    await page.getByRole("tab", { name: "Grammar" }).click();
  }
  await expect(page.getByText(CONTAINING)).toHaveCount(0);

  await assertContained(page);
});

test("the component's own answer survives beside the expression", async ({ page }) => {
  await analyze(page, "&expression=offer");

  // The selected word's own card is still there and still names the word. The offer is an
  // addition; nothing about reading the component changed.
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  await expect(page.getByText(CONTAINING)).toBeVisible();
});

test("a selection with no recognised expression is offered nothing", async ({ page }) => {
  await analyze(page, "");

  await expect(page.getByText(CONTAINING)).toHaveCount(0);
  await expect(page.getByText(CAVEAT)).toHaveCount(0);
});

test("a dictionary that could not be read is distinguishable from having no offer", async ({ page }) => {
  await analyze(page, "&expression=fail");

  // **Not silence.** A failure and an absence would otherwise look identical, and the reader
  // would have no way to know the entry existed.
  await expect(
    page.getByText("The expression entry could not be read. The word's own senses are unaffected."),
  ).toBeVisible();
  await expect(page.getByText(CONTAINING)).toHaveCount(0);

  // And the word's own answer is untouched by the failure.
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
});

test("a selection that is the whole expression is not told it is a part of one", async ({ page }) => {
  // The one shape production does not currently produce — both offered expressions are two
  // tokens — which is exactly why it is worth rendering from a fixture. `covers_whole_selection`
  // comes from the core comparing spans; the panel only has to say the right one of two things.
  await analyze(page, "&expression=whole");

  await expect(page.getByText(WHOLE, { exact: true })).toBeVisible();
  await expect(page.getByText(CONTAINING)).toHaveCount(0);
  // Same entry, same senses, same absence of a favourite — only the relationship differs.
  await expect(page.getByText("although; despite; even though")).toBeVisible();
  await expect(page.getByText(CAVEAT)).toBeVisible();

  await assertContained(page);
});

test("a selection that holds the expression is not told it is inside one", async ({ page }) => {
  // The fused case: one token spelling more than the expression, with the expression inside it.
  // Lindera splits, so production cannot reach this shape today — but Ichiran fuses 映画とか, and
  // the two claims are opposites. Saying "containing this selection" here would be backwards.
  await analyze(page, "&expression=within");

  await expect(page.getByText(WITHIN, { exact: true })).toBeVisible();
  await expect(page.getByText(CONTAINING)).toHaveCount(0);
  await expect(page.getByText("although; despite; even though")).toBeVisible();
  await assertContained(page);
});

test("a selection that only overlaps the expression is told so, not told it is part of one", async ({
  page,
}) => {
  // **The fourth relationship.** An ending may begin inside a token that also holds text before
  // it and finish in the next, so neither span contains the other. Two booleans reported this as
  // "contains", which is untrue of both halves; there is a fourth sentence because there is a
  // fourth case.
  await analyze(page, "&expression=overlap");

  await expect(page.getByText(OVERLAPPING, { exact: true })).toBeVisible();
  await expect(page.getByText(CONTAINING)).toHaveCount(0);
  await expect(page.getByText(WITHIN)).toHaveCount(0);
  await expect(page.getByText("although; despite; even though")).toBeVisible();
  await assertContained(page);
});

test("the relationship label sits below the heading, not beside it", async ({ page }) => {
  // **This is the separation the slice was asked for**, and the reason the label is a `<p>` and
  // not a `<span>` in the `<h4>`. Beside the heading it is a sentence competing with the
  // expression for one baseline row; every project viewport here is narrower than the one this
  // was designed at, and the longest label is a full sentence.
  //
  // `assertContained` catches the card overflowing its panel. It cannot see the two sharing a
  // row, because a flex row that wraps overflows nothing — it just reads wrong. So the geometry
  // is measured: the label starts at or below where the heading ends.
  for (const [query, label] of [
    ["&expression=offer", CONTAINING],
    ["&expression=whole", WHOLE],
    ["&expression=within", WITHIN],
    // The longest of the four, which is the one the geometry has to survive.
    ["&expression=overlap", OVERLAPPING],
  ]) {
    await analyze(page, query);
    const heading = page.locator(".expression h4");
    const relationship = page.locator(".expression .relationship");
    await expect(relationship).toHaveText(label);

    const [above, below] = await Promise.all([heading.boundingBox(), relationship.boundingBox()]);
    expect(above, "the expression heading").not.toBeNull();
    expect(below, "the relationship label").not.toBeNull();
    expect(
      below!.y,
      `"${label}" shares a row with the heading instead of sitting under it`,
    ).toBeGreaterThanOrEqual(above!.y + above!.height - 1);

    await assertContained(page);
  }
});
