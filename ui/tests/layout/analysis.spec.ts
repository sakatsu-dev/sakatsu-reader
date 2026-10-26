import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { assertContained, assertPageDoesNotScrollSideways } from "./contained";

const passage = "気持ちが伝わってきます。";

async function analyze(page: Page, query = "") {
  await page.goto(`/?qa=analysis${query}`);
  await page.getByPlaceholder("Paste Japanese text").fill(passage);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
}

async function openTranslation(page: Page) {
  await page.getByRole("button", { name: "Translation", exact: true }).click();
  await expect(page.getByTestId("translation-card")).toBeVisible();
}

async function finishHeldTranslation(page: Page) {
  expect(await page.evaluate(() =>
    (window as Window & { __SAKATSU_QA__?: { releaseTranslation: () => boolean } })
      .__SAKATSU_QA__?.releaseTranslation(),
  )).toBe(true);
}

/**
 * Kept as a name the tests below already read well with; the check itself is shared now.
 *
 * It used to be a list of nine selectors, which is how a licence overflowing its card on the
 * About page went unnoticed by a green suite — see `contained.ts`.
 */
const assertNoHorizontalOverflow = (page: Page) => assertContained(page);

test("the Analyze description grows into the page instead of beneath the side rail", async ({ page }) => {
  await page.goto("/?qa=analysis");
  const analyzeButton = page.getByRole("button", { name: /^Analyze/ });
  await analyzeButton.hover();
  await expect.poll(() => analyzeButton.evaluate((element) =>
    getComputedStyle(element, "::after").opacity,
  )).toBe("1");

  const geometry = await analyzeButton.evaluate((element) => {
    const button = element.getBoundingClientRect();
    const rail = document.querySelector<HTMLElement>(".rail")!.getBoundingClientRect();
    const app = document.querySelector<HTMLElement>(".app")!.getBoundingClientRect();
    const tip = getComputedStyle(element, "::after");
    const tooltipLeft = button.left + Number.parseFloat(tip.left);
    const tooltipWidth = Number.parseFloat(tip.width)
      + Number.parseFloat(tip.paddingLeft)
      + Number.parseFloat(tip.paddingRight)
      + Number.parseFloat(tip.borderLeftWidth)
      + Number.parseFloat(tip.borderRightWidth);
    return {
      tooltipLeft,
      tooltipRight: tooltipLeft + tooltipWidth,
      railRight: rail.right,
      appRight: app.right,
    };
  });

  expect(geometry.tooltipLeft).toBeGreaterThanOrEqual(geometry.railRight - 1);
  expect(geometry.tooltipRight).toBeLessThanOrEqual(geometry.appRight + 1);
});

test("Japanese keeps the complete study width and the card hierarchy never overlaps", async ({ page }) => {
  await analyze(page);
  const sourceOrder = await page.locator(".source-bar").evaluate((element) => {
    const left = (selector: string) => element.querySelector(selector)!.getBoundingClientRect().left;
    return [left(".source-name"), left(".source-divider"), left(".source-text")];
  });
  expect(sourceOrder[0]).toBeLessThan(sourceOrder[1]);
  expect(sourceOrder[1]).toBeLessThan(sourceOrder[2]);
  const readerBefore = await page.locator(".passage-column").boundingBox();
  await openTranslation(page);
  await expect(page.getByRole("button", { name: "Read as one passage" })).toHaveCount(0);
  const readerAfter = await page.locator(".passage-column").boundingBox();
  expect(readerBefore).not.toBeNull();
  expect(readerAfter).not.toBeNull();
  expect(Math.abs(readerAfter!.width - readerBefore!.width)).toBeLessThanOrEqual(1);

  const geometry = await page.evaluate(() => {
    const rect = (selector: string) => document.querySelector(selector)!.getBoundingClientRect();
    return {
      action: rect(".actions"), workspace: rect(".analysis-workspace"),
      translation: rect("[data-testid=translation-card]"),
      selected: rect("[data-testid=selected-word-card]"),
      header: rect("[data-testid=selected-word-header]"),
      details: rect(".study-detail"), footer: rect("footer"),
    };
  });
  expect(geometry.action.bottom).toBeLessThanOrEqual(geometry.workspace.top + 1);
  expect(geometry.translation.bottom).toBeLessThanOrEqual(geometry.selected.top + 1);
  expect(geometry.header.bottom).toBeLessThanOrEqual(geometry.details.top + 1);
  expect(geometry.details.bottom).toBeLessThanOrEqual(geometry.selected.bottom + 1);
  expect(geometry.selected.bottom).toBeLessThanOrEqual(geometry.footer.top + 1);
  expect(geometry.workspace.bottom).toBeLessThanOrEqual(geometry.footer.top + 1);
  await assertNoHorizontalOverflow(page);
});

test("a short selected-word card follows its content instead of filling a tall study pane", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "edge-900-100", "one tall desktop viewport proves the intrinsic-size rule");
  await page.setViewportSize({ width: 900, height: 900 });
  await analyze(page);
  await openTranslation(page);

  const geometry = await page.evaluate(() => {
    const study = document.querySelector<HTMLElement>(".study")!;
    const card = document.querySelector<HTMLElement>("[data-testid=selected-word-card]")!;
    const translation = document.querySelector<HTMLElement>("[data-testid=translation-card]")!;
    const gap = Number.parseFloat(getComputedStyle(study).rowGap);
    const bodies = [...card.querySelectorAll<HTMLElement>(".column-body")];
    return {
      availableForCard: study.getBoundingClientRect().height
        - translation.getBoundingClientRect().height
        - gap,
      cardHeight: card.getBoundingClientRect().height,
      bodyOverflow: bodies.map((body) => body.scrollHeight - body.clientHeight),
    };
  });

  expect(geometry.availableForCard - geometry.cardHeight).toBeGreaterThan(48);
  expect(geometry.bodyOverflow.every((overflow) => overflow <= 1)).toBe(true);
  await assertNoHorizontalOverflow(page);
});

test("the unified card has one authoritative Vocabulary and Grammar presentation", async ({ page }) => {
  await analyze(page);
  const card = page.getByTestId("selected-word-card");
  await expect(card.getByRole("heading", { name: "気持ち" })).toBeVisible();
  await expect(card.getByText("feeling; state of mind; sensation", { exact: true })).toHaveCount(1);
  await expect(card.getByText("Likely here", { exact: true })).toHaveCount(1);
  await expect(card.getByText("Meaning", { exact: true })).toHaveCount(0);
  await expect(card.getByText("No additional grammar.", { exact: true })).toHaveCount(0);
  await expect(card.getByText("Vocabulary", { exact: true })).toHaveCount(1);
  await expect(card.getByText("Grammar", { exact: true })).toHaveCount(1);
  await card.getByRole("button", { name: "Seen" }).click();
  await expect(card.getByRole("button", { name: "Seen" })).toHaveAttribute("aria-pressed", "true");

  await page.locator(".token").nth(2).click();
  if (await page.getByRole("tablist", { name: "Analysis details" }).count()) {
    await page.getByRole("tab", { name: "Grammar" }).click();
  }
  await expect(card.getByText("Te-form", { exact: true })).toHaveCount(1);
  await expect(card.getByText("〜てくる", { exact: true })).toHaveCount(1);
  await expect(card.getByText("Non-past", { exact: true })).toHaveCount(1);
  const patternRequests = await page.evaluate(() =>
    ((window as Window & { __SAKATSU_QA__?: { commands: string[] } }).__SAKATSU_QA__?.commands ?? [])
      .filter((command) => command === "explain_pattern").length,
  );
  expect(patternRequests).toBe(1);
});

test("known words lose furigana and regain it when marked seen", async ({ page }) => {
  await analyze(page);
  const words = page.locator("[data-reader-token]");
  const first = words.first();
  const other = words.nth(2);
  const card = page.getByTestId("selected-word-card");

  await expect(first.locator("rt")).toHaveCount(1);
  await expect(other.locator("rt")).toHaveCount(1);

  await card.getByRole("button", { name: "Known" }).click();
  await expect(first).toHaveClass(/status-known/);
  await expect(first.locator("rt")).toHaveCount(0);
  await expect(other.locator("rt")).toHaveCount(1);

  await card.getByRole("button", { name: "Seen" }).click();
  await expect(first).toHaveClass(/status-seen/);
  await expect(first.locator("rt")).toHaveCount(1);

  await page.getByRole("button", { name: "Furigana" }).click();
  await expect(first.locator("rt")).toHaveCount(0);
  await expect(other.locator("rt")).toHaveCount(0);

  await page.getByRole("button", { name: "Furigana" }).click();
  for (const index of [0, 1, 2]) {
    await words.nth(index).click({ button: "right" });
    await expect(words.nth(index)).toHaveClass(/status-known/);
  }
  await expect(page.locator(".reader rt")).toHaveCount(0);
  await expect(page.locator(".reader")).not.toHaveClass(/has-ruby/);
});

test("the inline pronunciation and status actions share the word row's centre", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "edge-900-100", "one reviewed desktop geometry is sufficient");
  await analyze(page, "&fixture=audio");
  await page.locator(".token").nth(2).click();

  const header = page.getByTestId("selected-word-header");
  const word = header.locator(".word");
  const headword = word.locator(".headword");
  const reading = word.locator(".reading");
  const sound = word.getByRole("button", { name: /Hear/ });
  await expect(headword).toBeVisible();
  await expect(reading).toBeVisible();
  await expect(sound).toBeVisible();

  const geometry = await word.evaluate((element) => {
    const box = (selector: string) => element.querySelector(selector)!.getBoundingClientRect();
    const headwordBox = box(".headword");
    const readingBox = box(".reading");
    const soundBox = box(".speak");
    return {
      centres: [headwordBox, readingBox, soundBox].map((item) => item.top + item.height / 2),
      order: [headwordBox.left, readingBox.left, soundBox.left],
    };
  });
  expect(Math.max(...geometry.centres) - Math.min(...geometry.centres)).toBeLessThanOrEqual(1);
  expect(geometry.order[0]).toBeLessThan(geometry.order[1]);
  expect(geometry.order[1]).toBeLessThan(geometry.order[2]);

  const rowCentres = await header.evaluate((element) => {
    const wordBox = element.querySelector(".word")!.getBoundingClientRect();
    const statusBox = element.querySelector(".status")!.getBoundingClientRect();
    return [wordBox.top + wordBox.height / 2, statusBox.top + statusBox.height / 2];
  });
  expect(Math.abs(rowCentres[0] - rowCentres[1])).toBeLessThanOrEqual(1);
  await expect(header).toHaveScreenshot("selected-word-header-inline-audio.png", {
    maxDiffPixelRatio: 0.005,
  });
});

test("Anki feedback stays in the unified fixed header", async ({ page }) => {
  await analyze(page, "&fixture=anki");
  const card = page.getByTestId("selected-word-card");
  const add = card.getByRole("button", { name: /Anki/ });
  await expect(add).toBeVisible();
  await add.click();
  await expect(page.getByTestId("selected-word-header").getByText("added to Sakatsu")).toBeVisible();
});

test("the selected-word header remains fixed while both wide detail sections scroll", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "edge-900-100", "the narrow layout intentionally shows one section");
  await analyze(page, "&fixture=long");
  const header = page.getByTestId("selected-word-header");
  const card = page.getByTestId("selected-word-card");
  await card.getByText("5 more senses", { exact: true }).click();
  await card.getByText("6 more examples", { exact: true }).click();
  // Not in the header any more: it sits at the end of the pane headings, so that opening a
  // disclosure cannot change the identity strip's height.
  await expect(card.getByRole("button", { name: "Reset details" })).toBeVisible();
  await expect(page.getByTestId("selected-word-header").getByRole("button", { name: "Reset details" })).toHaveCount(0);
  const before = await header.boundingBox();

  const bodies = card.locator(".column-body");
  expect(await bodies.count()).toBe(2);
  const vocabulary = bodies.nth(0);
  await vocabulary.evaluate((element) => { element.scrollTop = element.scrollHeight; });
  expect(await vocabulary.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  const after = await header.boundingBox();
  expect(before).not.toBeNull();
  expect(after).not.toBeNull();
  expect(Math.abs(after!.y - before!.y)).toBeLessThanOrEqual(1);
  expect(Math.abs(after!.height - before!.height)).toBeLessThanOrEqual(1);

  await page.locator(".token").nth(2).click();
  await expect.poll(() => vocabulary.evaluate((element) => element.scrollTop)).toBe(0);
  await expect(card.locator("details[open]")).toHaveCount(2);

  const grammar = bodies.nth(1);
  await grammar.evaluate((element) => { element.scrollTop = element.scrollHeight; });
  expect(await grammar.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
  await page.locator(".token").first().click();
  await expect.poll(() => grammar.evaluate((element) => element.scrollTop)).toBe(0);

  await card.getByRole("button", { name: "Reset details" }).click();
  await expect(card.locator("details[open]")).toHaveCount(0);
});

test("responsive details switch at the established usable-width threshold", async ({ page }, testInfo) => {
  await analyze(page);
  const constrained = testInfo.project.name !== "edge-900-100";
  await expect(page.getByRole("tablist", { name: "Analysis details" })).toHaveCount(constrained ? 1 : 0);
  if (constrained) {
    await expect(page.getByRole("tab", { name: "Vocabulary" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Grammar" })).toBeVisible();
  }
  await assertNoHorizontalOverflow(page);
});

test("a comparison opens line by line whatever the default, and context stays lazy there", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await page.getByPlaceholder("Paste Japanese text").fill(`前の文です。${passage}`);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();

  // **A comparison is a per-sentence operation, so it opens line by line whatever the default
  // is.** It transforms the one affected sentence in place, which the whole-passage reading has
  // nowhere to put.
  await page.locator(".token").filter({ hasText: "気持ち" }).click({ modifiers: ["Control"] });
  await expect(page.getByTestId("comparison-output")).toBeVisible();
  await expect(page.getByTestId("comparison-baseline")).toHaveCount(1);
  await expect(page.getByTestId("translation-sentence")).toHaveCount(2);
  await expect(page.getByText("This is the preceding sentence.", { exact: true })).toBeVisible();

  const callsBefore = await page.evaluate(() =>
    ((window as Window & { __SAKATSU_QA__?: { commands: string[] } }).__SAKATSU_QA__?.commands ?? [])
      .filter((command) => command === "translate").length,
  );
  expect(callsBefore).toBe(0);

  await page.getByRole("button", { name: "Read as one passage" }).click();
  await expect(page.getByTestId("context-translation")).toHaveText(
    "This is the preceding sentence. My feelings are coming through.",
  );
  await expect(page.getByTestId("aligned-translation")).toHaveCount(0);
  await expect(page.getByTestId("comparison-output")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Clear comparison" })).toBeVisible();

  await page.getByRole("button", { name: "Read line by line" }).click();
  await expect(page.getByTestId("comparison-output")).toBeVisible();
  await page.getByRole("button", { name: "Read as one passage" }).click();
  const contextCalls = await page.evaluate(() =>
    ((window as Window & { __SAKATSU_QA__?: { commands: string[] } }).__SAKATSU_QA__?.commands ?? [])
      .filter((command) => command === "translate").length,
  );
  expect(contextCalls).toBe(1);

  await page.getByRole("button", { name: "Clear comparison" }).click();
  await expect(page.getByTestId("context-translation")).toBeVisible();
  await expect(page.getByRole("button", { name: "Clear comparison" })).toHaveCount(0);
  await page.getByRole("button", { name: "Read line by line" }).click();
  await expect(page.getByTestId("translation-sentence")).toHaveCount(2);
  await expect(page.getByTestId("comparison-output")).toHaveCount(0);
});

test("comparison progress and failure stay beside the affected aligned sentence", async ({ page }) => {
  await analyze(page, "&fixture=comparison-loading&hold-translation=1");
  await openTranslation(page);
  await page.locator(".token").first().click({ modifiers: ["Control"] });
  const active = page.locator("[data-active-sentence=true]");
  await expect(active.getByText("My feelings are coming through.", { exact: true })).toBeVisible();
  await expect(active.getByText("Updating comparison…", { exact: true })).toBeVisible();
  await finishHeldTranslation(page);
  await expect(active.getByTestId("comparison-output")).toBeVisible();

  await analyze(page, "&fixture=comparison-error");
  await openTranslation(page);
  await page.locator(".token").first().click({ modifiers: ["Control"] });
  const failed = page.locator("[data-active-sentence=true]");
  await expect(failed.getByText("My feelings are coming through.", { exact: true })).toBeVisible();
  await expect(failed.getByRole("alert")).toContainText("Focused comparison is unavailable");
  await expect(failed.getByRole("button", { name: "Retry" })).toBeVisible();
  await assertNoHorizontalOverflow(page);
});

/**
 * **The accuracy judgement is the app's to make, not the reader's.**
 *
 * Opening a translation used to land on the sentence-aligned flow and leave the whole-passage
 * reading behind a button, which asked the reader to know when Japanese had dropped a subject.
 * `core::translate::passage` measured the two: on unpunctuated text the whole passage scores
 * 0.865 against 0.852, and on wrapped lines 0.847 against 0.819 — which is what a manga panel
 * and a game text box are.
 */
test("a translation opens on the whole passage when there is more than one sentence", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await page.getByPlaceholder("Paste Japanese text").fill(`前の文です。${passage}`);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  await openTranslation(page);

  await expect(page.getByTestId("context-translation")).toHaveText(
    "This is the preceding sentence. My feelings are coming through.",
  );
  await expect(page.getByTestId("aligned-translation")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Read line by line" })).toBeVisible();

  // Line by line is the lazy one now, and reachable in one click.
  await page.getByRole("button", { name: "Read line by line" }).click();
  await expect(page.getByTestId("translation-sentence")).toHaveCount(2);
  await assertNoHorizontalOverflow(page);
});

test("context loading and failure replace the aligned flow without disturbing the page", async ({ page }) => {
  const multi = `前の文です。${passage}`;
  await page.goto("/?qa=analysis&fixture=context-loading&hold-translation=1");
  await page.getByPlaceholder("Paste Japanese text").fill(multi);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  // Opening is enough; the whole-passage reading is the default for a two-sentence passage.
  await openTranslation(page);
  await expect(page.getByText("Translating context…", { exact: true })).toBeVisible();
  await expect(page.getByTestId("aligned-translation")).toHaveCount(0);
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  await finishHeldTranslation(page);
  await expect(page.getByTestId("context-translation")).toBeVisible();

  await page.goto("/?qa=analysis&fixture=context-error");
  await page.getByPlaceholder("Paste Japanese text").fill(multi);
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await openTranslation(page);
  await expect(page.getByTestId("translation-card").getByRole("alert")).toContainText(
    "Passage context is unavailable",
  );
  await page.getByRole("button", { name: "Read line by line" }).click();
  await expect(page.getByTestId("translation-sentence")).toHaveCount(2);
  await assertNoHorizontalOverflow(page);
});

/** How much of the translation is on show, against how much there is and the card's bound. */
async function translationShown(page: Page) {
  return page.getByTestId("translation-body").evaluate((element) => ({
    height: element.getBoundingClientRect().height,
    scrollHeight: element.scrollHeight,
    maxHeight: Number.parseFloat(getComputedStyle(element).maxHeight),
  }));
}

/**
 * **The whole translation, always.** It used to clamp to three lines behind "Show full
 * translation", which cut sentences off mid-line. Now it shows in full and scrolls only past the
 * card's bound — a share of the pane, so a long passage cannot push the panels below off screen.
 */
test("the whole translation shows with no disclosure, and keeps the card's geometry", async ({ page }) => {
  await analyze(page, "&fixture=long-translation");
  await openTranslation(page);
  await expect(page.getByTestId("translation-sentence").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Show full translation" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Show less" })).toHaveCount(0);

  const shown = await translationShown(page);
  expect(shown.height, "as tall as the text, up to the card's bound").toBeGreaterThanOrEqual(
    Math.min(shown.scrollHeight, shown.maxHeight) - 1,
  );

  // A comparison and its clearing leave it whole.
  await page.locator(".token").first().click({ modifiers: ["Control"] });
  await expect(page.getByTestId("comparison-output")).toBeVisible();
  await page.getByRole("button", { name: "Clear comparison" }).click();
  const after = await translationShown(page);
  expect(after.height).toBeGreaterThanOrEqual(Math.min(after.scrollHeight, after.maxHeight) - 1);
  await expect(page.getByRole("button", { name: "Show full translation" })).toHaveCount(0);
  const card = await page.getByTestId("translation-card").evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      border: Number.parseFloat(style.borderTopWidth), radius: Number.parseFloat(style.borderTopLeftRadius),
      left: Number.parseFloat(style.paddingLeft), right: Number.parseFloat(style.paddingRight),
    };
  });
  expect(card.border).toBe(1);
  expect(card.radius).toBeGreaterThan(0);
  expect(card.left).toBeGreaterThan(5);
  expect(Math.abs(card.left - card.right)).toBeLessThanOrEqual(1);
  await assertNoHorizontalOverflow(page);
});

test("loading and failures stay inside the Translation card", async ({ page }) => {
  await analyze(page, "&fixture=loading");
  await page.getByRole("button", { name: "Translation", exact: true }).click();
  const loadingCard = page.getByTestId("translation-card");
  await expect(loadingCard.getByText("Translating…")).toBeVisible();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  await expect(loadingCard.getByText("My feelings are coming through.")).toBeVisible();

  await analyze(page, "&fixture=error");
  await openTranslation(page);
  const card = page.getByTestId("translation-card");
  await expect(card.getByRole("alert")).toContainText("translation model is unavailable");
  await expect(card.getByRole("button", { name: "Retry" })).toBeVisible();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  await assertNoHorizontalOverflow(page);
});

test("draft, ambiguity, long content and themes retain the same shell", async ({ page }, testInfo) => {
  const query = testInfo.project.name === "edge-620-150"
    ? "&fixture=ambiguous&theme=light"
    : testInfo.project.name === "edge-480-200"
      ? "&fixture=long"
      : "&fixture=long";
  await analyze(page, query);
  // Still found by name after the button became a glyph: `aria-label="Edit"` is what carries it
  // now that the word is not on screen, and this is the assertion that would catch its loss.
  await page.getByRole("button", { name: "Edit" }).click();
  // The draft caption that stood here said the current analysis remains below until the new one
  // succeeds. The line under this one *demonstrates* that by finding it, which is why the caption
  // went: it described what the screen was already showing.
  await expect(page.getByPlaceholder("Paste Japanese text")).toBeVisible();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();

  if (query.includes("ambiguous")) {
    const card = page.getByTestId("selected-word-card");
    await expect(card.getByText("feeling; state of mind; sensation", { exact: true })).toBeVisible();
    await expect(card.getByText("another meaning supported equally by this context", { exact: true })).toBeVisible();
    await expect(card.getByText("Several meanings fit this context.", { exact: true })).toHaveCount(1);
    await expect(card.getByText("Likely here", { exact: true })).toHaveCount(0);
  }
  if (query.includes("vertical")) {
    await expect(page.locator(".reader")).toHaveCSS("writing-mode", "vertical-rl");
  }
  if (query.includes("theme=light")) {
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  }
  await assertNoHorizontalOverflow(page);
});

test("Reference kanji enrichment remains contained and uses the current search tab", async ({ page }) => {
  await page.goto("/?qa=reference");
  await page.getByRole("button", { name: "Reference" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill("図書館");
  await search.press("Enter");
  // The list item, not the preview: the card is content and its tab is the control, and the tab
  // hangs off the card's bottom edge rather than sitting inside it — a card that clips to its
  // rounded corners could not let it out. Expansion is read off the tab.
  const result = page.locator(".result").filter({ hasText: "図書館" });
  await expect(result.locator(".preview")).toBeVisible();
  await expect(result.locator("> .disclose")).toHaveAttribute("aria-expanded", "true");

  const tabCount = await page.locator(".tabs .tab").count();
  // **Every kanji in the word, already open.** They used to be chips that opened one at a time,
  // so reading 図書館 meant three decisions before it said anything about any of them. The panel
  // itself is what is counted, not a box around it: Reference places these in their own column
  // beside the word, so there is no wrapper.
  const panels = page.getByRole("region", { name: "図書館 detail" }).locator(".character");
  await expect(panels).toHaveCount(3);
  // Located by its own diagram rather than by position, so the assertions below cannot quietly
  // move to a different character if the order ever changes.
  const detail = panels.filter({
    has: page.getByRole("img", { name: "Stroke-order diagram for 図" }),
  });
  await expect(detail).toHaveCount(1);
  await expect(detail.getByRole("img", { name: "Stroke-order diagram for 図" })).toBeVisible();
  // The facts are a labelled list now, and the components sit under a section heading.
  await expect(detail.getByText("Nanori", { exact: true })).toBeVisible();
  await expect(detail.getByText("のり", { exact: true })).toBeVisible();
  await expect(detail.getByRole("heading", { name: "Components" })).toBeVisible();

  // **A character's commonest compounds, and an honest count of the rest.** The kanji detail had
  // neither until the index existed; the count comes from the whole posting list while only the
  // shown rows are read from disk, which is the bargain that lets it say "of 214" cheaply.
  await expect(detail.getByRole("heading", { name: "Common compounds" })).toBeVisible();
  const compounds = detail.locator(".compounds li");
  await expect(compounds).toHaveCount(3);
  await expect(compounds.first()).toContainText("図書館");
  await expect(detail.getByText("Showing 3 of 214")).toBeVisible();
  // The character is marked inside each compound, so it is visible why the word is on this page.
  await expect(compounds.first().locator("mark")).toHaveText("図");
  await expect(detail.getByText("Visible parts; not necessarily traditional radicals.")).toBeVisible();
  await detail.getByRole("button", { name: "Play stroke order" }).click();
  await expect(detail.getByRole("button", { name: "Replay stroke order" })).toBeVisible();

  await detail.getByRole("button", { name: "囗" }).click();
  await expect(detail.getByText("30 characters share 囗", { exact: true })).toBeVisible();
  await expect(detail.getByRole("button", { name: /Look up 囲/ })).toBeVisible();
  await detail.getByRole("button", { name: /Look up 囲/ }).click();
  await expect(page.locator(".tabs .tab")).toHaveCount(tabCount);

  // 書 needed a press of its own before; it is simply there now, which is what the count above
  // already says. What is worth re-checking is that walking to a related kanji left the rest of
  // the stack alone rather than collapsing it.
  await expect(panels).toHaveCount(3);
  await assertNoHorizontalOverflow(page);
  await assertPageDoesNotScrollSideways(page);
});

test("default and constrained layouts match their reviewed baselines", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name === "edge-480-200", "two compact baselines are sufficient");
  await analyze(page);
  await openTranslation(page);
  await expect(page).toHaveScreenshot("analysis-translation.png", {
    animations: "disabled",
    maxDiffPixelRatio: 0.01,
  });
});

/**
 * The unified card keeps its status control and both halves, in whichever shape it is wearing.
 *
 * **This is the failure the screenshots are worst at.** A pixel diff of a card that has lost its
 * Grammar section is a large red region that looks exactly like a card that merely reflowed, and
 * the reviewer is left to decide which. The accessible tree says which.
 *
 * Two templates because the card genuinely has two shapes, and flattening that to something both
 * satisfy would assert almost nothing. Wide, the sections are headings side by side; constrained,
 * they are a tablist — the same threshold the test above measures, stated here as structure.
 *
 * Unlike the baselines, this survives a font, theme or spacing change, so it runs everywhere.
 * See `fixtures.ts` for why that is not free for the pixel snapshots.
 */
test("the selected-word card keeps its status control and both sections", async ({ page }, testInfo) => {
  await analyze(page);
  const constrained = testInfo.project.name !== "edge-900-100";
  const sections = constrained
    ? `- tablist "Analysis details":
        - tab "Vocabulary" [selected]
        - tab "Grammar"`
    : `- heading "Vocabulary" [level=2]
      - heading "Grammar" [level=2]`;
  await expect(page.getByTestId("selected-word-card")).toMatchAriaSnapshot(`
    - region "Selected word":
      - heading "気持ち" [level=2]
      - group "How well you know this word":
        - button "new" [pressed]
        - button "seen"
        - button "known"
        - button "ignored"
      ${sections}
  `);
});

/**
 * The Vocabulary/Grammar divider takes the last few pixels itself.
 *
 * Even panes are the position anybody wants *back*, and landing on exactly `0.5` by hand is not
 * something a pointer can do. Within `snapWithin` pixels of the middle the divider lands centred;
 * outside it, a position somebody chose is left exactly as chosen.
 *
 * `aria-valuenow` is the reading, and it is also the only trace the snap leaves: there is no guide
 * line and no highlight, because "silently" was the requirement. Telling a screen reader where the
 * separator actually ended up is not a leak.
 */
const panels = (page: Page) => page.getByRole("separator", { name: "Resize the panels" });

/**
 * Skip where there is no divider to drag.
 *
 * Below a certain width the two panes stop being a split and become a pair of tabs — see the
 * `{#if}` around `<Split direction="row">` in `App.svelte`. There is no separator then, and no
 * centre to snap to, so these are wide-viewport tests by nature rather than by convenience.
 */
async function skipWithoutTheSplit(page: Page) {
  test.skip((await panels(page).count()) === 0, "the panes are tabs at this width, not a split");
}

/** Drag the panels divider to `x`, in pixels from the left of the viewport. */
async function dragPanelsTo(page: Page, x: number) {
  const box = (await panels(page).boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(x, box.y + box.height / 2, { steps: 8 });
  await page.mouse.up();
}

/** The midpoint of the split the divider lives in, in viewport pixels. */
async function panelsCentre(page: Page) {
  return page.evaluate(() => {
    const split = document.querySelector(".analysis-workspace .split.sideways")!;
    const box = split.getBoundingClientRect();
    return box.left + box.width / 2;
  });
}

test("the panels divider snaps to the centre, and only near it", async ({ page }) => {
  await analyze(page);
  await skipWithoutTheSplit(page);
  const divider = panels(page);
  const centre = await panelsCentre(page);

  // Eight pixels off, inside a fourteen-pixel zone. Nothing on screen says so; the value does.
  await dragPanelsTo(page, centre + 8);
  await expect(divider).toHaveAttribute("aria-valuenow", "50");

  // A quarter of the way along is a position somebody chose, and it survives untouched. Asserted
  // as "not centred" rather than as an exact share, because the clamp and the pane floors have
  // their own say about where a drag lands and this test is not about them.
  const box = (await page.locator(".analysis-workspace .split.sideways").boundingBox())!;
  await dragPanelsTo(page, box.x + box.width * 0.3);
  await expect(divider).not.toHaveAttribute("aria-valuenow", "50");

  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

test("the keyboard can still leave the centre once the pointer has snapped it there", async ({ page }) => {
  await analyze(page);
  await skipWithoutTheSplit(page);
  const divider = panels(page);
  const centre = await panelsCentre(page);
  await dragPanelsTo(page, centre + 8);
  await expect(divider).toHaveAttribute("aria-valuenow", "50");

  // Arrow keys move off the centre and land where they were aimed.
  //
  // **This does not, on its own, prove the keyboard is safe from the snap** — at the widths where
  // the split exists the zone comes out narrower than the 0.02 step, so a snapped `key()` would
  // pass this too. The trap itself is demonstrated in `split.test.mjs`, on a container narrow
  // enough for the zone to swallow a step. What this holds is the behaviour a reader would notice
  // first: that the divider can be moved off centre at all once the pointer has parked it there.
  await divider.focus();
  await divider.press("ArrowLeft");
  await expect(divider).not.toHaveAttribute("aria-valuenow", "50");
  await divider.press("ArrowRight");
  await expect(divider).toHaveAttribute("aria-valuenow", "50");
  await divider.press("ArrowRight");
  await expect(divider).not.toHaveAttribute("aria-valuenow", "50");
});
