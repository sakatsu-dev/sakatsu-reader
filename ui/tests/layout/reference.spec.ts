import type { Locator, Page } from "@playwright/test";
import { assertContained, assertPageDoesNotScrollSideways } from "./contained";
import { expect, test } from "./fixtures";

async function openReference(page: Page, fixture = "parts", query = "eat", extra = "") {
  await page.goto(`/?qa=reference&fixture=${fixture}${extra}`);
  await page.getByRole("button", { name: "Reference" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill(query);
  await search.press("Enter");
  const collection = page.getByRole("list", { name: "Reference results" });
  await expect(collection).toBeVisible();
  const filters = page.locator(".word-filter-bar details");
  if (await filters.count()) await filters.evaluate((element) => (element as HTMLDetailsElement).open = true);
  return collection;
}

async function isWide(page: Page) {
  return page.locator(".reference").evaluate((element) => element.getBoundingClientRect().width >= 672);
}

function previews(collection: Locator) {
  return collection.locator(":scope > .result > .card > .preview");
}

/**
 * The control that opens a card: a real button, drawn as nothing.
 *
 * It carries `aria-expanded`, the roving tabindex and the keys, so tests read state from it and
 * focus it. A pointer cannot click it — the visible tab it used to be was removed on 2026-10-17 —
 * so every click goes to the card itself, through `toggle` below. Filtering still happens on the
 * preview, because the button carries no text.
 */
function openers(collection: Locator) {
  return collection.locator(":scope > .result > .disclose");
}

function opener(preview: Locator) {
  // **Up to the list item, not across.** The control is a sibling of the *card*, not of the
  // preview. `ancestor::li` survives whatever nesting the card grows next.
  return preview.locator("xpath=ancestor::li[1]").locator("> .disclose");
}

/**
 * Click a card the way a reader does: in the preview's bottom-left padding, where there is never
 * a button or a run of text. The centre can land on a control inside the preview, and once the
 * card is open the card's centre is inside `.inline-detail`, which deliberately ignores clicks.
 */
async function clickCard(item: Locator) {
  const preview = item.locator("> .card > .preview");
  const box = await preview.boundingBox();
  await preview.click({ position: { x: 4, y: box!.height - 3 } });
}


/** Open or close a card through its control's list item, the way a reader does. */
async function toggle(tab: Locator) {
  const item = tab.locator("xpath=..");
  await clickCard(item);
}


async function commandCount(page: Page, command: string) {
  return page.evaluate((wanted) => {
    const qa = (window as Window & { __SAKATSU_QA__?: { commands: string[] } }).__SAKATSU_QA__;
    return qa?.commands.filter((entry) => entry === wanted).length ?? 0;
  }, command);
}

test("facets and all-sense POS filters preserve one stable collection", async ({ page }) => {
  const collection = await openReference(page);
  await expect(previews(collection)).toHaveCount(5);

  await page.getByRole("radio", { name: /^Words 5$/ }).click();
  await expect(previews(collection)).toHaveCount(5);
  // 道中 is a noun as well as an adverb, so it is counted here too.
  await page.getByRole("button", { name: /^Noun 2$/ }).click();
  await expect(previews(collection)).toHaveCount(2);
  await expect(previews(collection).first()).toContainText("図書館");
  await page.getByRole("button", { name: "Clear word filters" }).click();
  await expect(previews(collection)).toHaveCount(5);

  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

test("ambiguous previews stay collapsed and exactly one detail expands inline", async ({ page }) => {
  const collection = await openReference(page);
  await expect(collection.locator(".inline-detail")).toHaveCount(0);

  const library = opener(previews(collection).filter({ hasText: "図書館" }));
  const eat = opener(previews(collection).filter({ hasText: "食べ" }));
  await expect(library).toHaveAttribute("aria-expanded", "false");
  await toggle(library);
  await expect(library).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByRole("region", { name: "図書館 detail" })).toBeVisible();
  await expect(collection.locator(".inline-detail")).toHaveCount(1);

  await toggle(eat);
  await expect(page.getByRole("region", { name: "図書館 detail" })).toHaveCount(0);
  await expect(page.getByRole("region", { name: /食べ.* detail/ })).toBeVisible();
  await expect(collection.locator(".inline-detail")).toHaveCount(1);

  await toggle(eat);
  await expect(collection.locator(".inline-detail")).toHaveCount(0);
});

test("the result collection uses roving focus and Escape returns focus to its opener", async ({ page }) => {
  const collection = await openReference(page);
  const items = openers(collection);
  await items.first().focus();
  await items.first().press("End");
  await expect(items.last()).toBeFocused();
  await items.last().press("Home");
  await expect(items.first()).toBeFocused();
  await items.first().press("ArrowDown");
  await expect(items.nth(1)).toBeFocused();

  // **Follow the card, not the position.** Opening a result promotes it to the front of the
  // collection, so `nth(1)` is a different result the moment Enter lands and back to this one
  // after Escape. `data-result` is the identity that survives the trip both ways — which is the
  // point of the roving focus this test is about.
  const key = await items.nth(1).getAttribute("data-result");
  const opened = collection.locator(`.disclose[data-result="${key}"]`);
  await items.nth(1).press("Enter");
  await expect(opened).toHaveAttribute("aria-expanded", "true");
  await expect(items.nth(1)).toHaveAttribute("data-result", key!);
  await page.keyboard.press("Escape");
  await expect(opened).toHaveAttribute("aria-expanded", "false");
  await expect(opened).toBeFocused();
  await expect(items.nth(1)).toHaveAttribute("data-result", key!);
});

test("tabs restore their expansion and collection scroll for the session", async ({ page }) => {
  const collection = await openReference(page);
  const library = opener(previews(collection).filter({ hasText: "図書館" }));
  await toggle(library);
  const savedScroll = await collection.evaluate((element) => {
    element.scrollTop = Math.min(32, element.scrollHeight - element.clientHeight);
    element.dispatchEvent(new Event("scroll"));
    return element.scrollTop;
  });

  await page.getByRole("button", { name: "New search" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill("second");
  await search.press("Enter");
  await toggle(openers(collection).first());

  await page.getByRole("button", { name: "eat", exact: true }).click();
  await expect(library).toHaveAttribute("aria-expanded", "true");
  await expect.poll(() => collection.evaluate((element) => element.scrollTop)).toBeCloseTo(savedScroll, 0);
});

test("Reference is one continuous column at every width", async ({ page }) => {
  const collection = await openReference(page);
  await expect(page.getByRole("button", { name: /Show as rows|Show as cards/ })).toHaveCount(0);
  const first = await previews(collection).nth(0).boundingBox();
  const second = await previews(collection).nth(1).boundingBox();
  expect(Math.abs(first!.x - second!.x)).toBeLessThan(2);
  expect(second!.y).toBeGreaterThanOrEqual(first!.y + first!.height);
  await assertContained(page);
});

/**
 * **The card's control is invisible, takes no room, and is still the keyboard's way in.**
 *
 * The visible tab under each card was removed on 2026-10-17 because it crowded the page. What
 * must not go with it is everything that is not pixels: the button keeps its place in the
 * accessibility tree, its state and the tab order — `display: none` or `visibility: hidden` would
 * have left the cards reachable by mouse alone — and its keyboard focus has to show somewhere,
 * which is on the card.
 */
test("a card's control is invisible, takes no room, and shows keyboard focus on the card", async ({ page }) => {
  const collection = await openReference(page);
  const card = collection.locator(":scope > .result").nth(1);
  const tab = card.locator("> .disclose");

  const geometry = await card.evaluate((item) => {
    const box = item.getBoundingClientRect();
    const body = item.querySelector(".card")!.getBoundingClientRect();
    const control = item.querySelector(".disclose")!.getBoundingClientRect();
    return { itemBottom: box.bottom, cardBottom: body.bottom, width: control.width, height: control.height };
  });
  expect(geometry.width, "the control draws nothing").toBeLessThanOrEqual(1);
  expect(geometry.height).toBeLessThanOrEqual(1);
  // **Nothing is reserved under the card**, which is what removing the tab was for: the list item
  // ends where the card does.
  expect(geometry.itemBottom).toBeCloseTo(geometry.cardBottom, 0);

  // Still announced, with its state.
  await expect(tab).toHaveAttribute("aria-expanded", "false");
  expect(await card.ariaSnapshot()).toContain("Show details for");

  // **Focus is asserted before any click**: Chromium stops matching `:focus-visible` once a click
  // has told it a pointer is in charge.
  const ring = () => card.locator("> .card").evaluate((el) => getComputedStyle(el).outlineStyle);
  expect(await ring(), "no ring at rest").not.toBe("solid");
  await tab.focus();
  await expect(tab).toBeFocused();
  await expect.poll(ring, "the card wears its control's keyboard focus").toBe("solid");

  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

/**
 * **Two clicks, and what must survive being able to click the card at all.**
 *
 * The first click picks the card — it lightens — and only the second opens
 * it. The card was deliberately not a click target before this, because a card that is one has no
 * quiet place to select a definition from; the guards are what let the click come back without
 * taking that away, and they are the half of this feature a reader hits first.
 */
test("a card opens on its first click with the legacy preference enabled", async ({ page }) => {
  const collection = await openReference(page);
  await expect(collection).toHaveAttribute("data-cards", "single");
  const card = collection.locator(":scope > .result").nth(1);
  const tab = card.locator("> .disclose");

  // At rest: nothing about this card is picked.
  await expect(card).not.toHaveAttribute("data-focused", "");

  // The first click selects and opens the entry together.
  await clickCard(card);
  await expect(card).toHaveAttribute("data-focused", "");
  await expect(tab).toHaveAttribute("aria-expanded", "true");
  await expect(collection.locator(".inline-detail")).toHaveCount(1);

  // A second preview click closes the same entry.
  const key = await tab.getAttribute("data-result");
  const opened = collection.locator(`.disclose[data-result="${key}"]`);
  await clickCard(card);
  await expect(opened).toHaveAttribute("aria-expanded", "false");
  await expect(collection.locator(".inline-detail")).toHaveCount(0);
});

/**
 * **The card stays content, which is the whole reason it was not a button.**
 *
 * Both guards protect a gesture a reader makes constantly and would lose silently: copying a
 * headword, and dragging across a definition to select it. Neither may open or close anything.
 */
test("copying a word and selecting text never open a card", async ({ page }) => {
  const collection = await openReference(page);
  const card = collection.locator(":scope > .result").nth(1);
  const tab = card.locator("> .disclose");

  // Copy and drag must leave the initially closed entry closed.
  await expect(tab).toHaveAttribute("aria-expanded", "false");

  // A control inside the card belongs to that control.
  await card.getByRole("button", { name: /^Copy/ }).click();
  await expect(tab).toHaveAttribute("aria-expanded", "false");
  await expect(collection.locator(".inline-detail")).toHaveCount(0);

  // A drag that selects text was someone selecting text.
  const word = card.locator(".identity").first();
  const box = await word.boundingBox();
  await page.mouse.move(box!.x + 2, box!.y + box!.height / 2);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width - 2, box!.y + box!.height / 2, { steps: 8 });
  await page.mouse.up();
  expect(await page.evaluate(() => window.getSelection()?.toString() ?? ""), "the drag selected something").not.toBe("");
  await expect(tab).toHaveAttribute("aria-expanded", "false");
  await expect(collection.locator(".inline-detail")).toHaveCount(0);
});

/** Older stored values must not bring back a two-step activation. */
test("the legacy preference off also uses one click to open and close", async ({ page }) => {
  const collection = await openReference(page, "parts", "eat", "&cards=hover");
  await expect(collection).toHaveAttribute("data-cards", "single");
  // **Follow the card, not the position.** Opening promotes it to the front of the collection, so
  // `nth(1)` is a different result the moment the click lands; `data-result` is the identity that
  // survives the trip.
  const key = await collection.locator(":scope > .result").nth(1).locator("> .disclose").getAttribute("data-result");
  const tab = collection.locator(`.disclose[data-result="${key}"]`);
  const item = collection.locator(`.result:has(> .disclose[data-result="${key}"])`);

  await clickCard(item);
  await expect(tab).toHaveAttribute("aria-expanded", "true");
  await expect(collection.locator(".inline-detail")).toHaveCount(1);
  await clickCard(item);
  await expect(tab).toHaveAttribute("aria-expanded", "false");
  await expect(collection.locator(".inline-detail")).toHaveCount(0);
});


test("opening an entry preserves preceding rows and reveals its heading", async ({ page }) => {
  const collection = await openReference(page);
  const firstKey = await openers(collection).first().getAttribute("data-result");
  const target = openers(collection).nth(2);
  await toggle(target);
  await expect(target).toHaveAttribute("aria-expanded", "true");
  await expect(openers(collection).first()).toHaveAttribute("data-result", firstKey!);
  const bounds = await collection.boundingBox();
  const heading = await previews(collection).nth(2).boundingBox();
  expect(heading!.y).toBeGreaterThanOrEqual(bounds!.y - 1);
  expect(heading!.y).toBeLessThan(bounds!.y + bounds!.height);
});


test("opening, navigating and closing retain the original result order", async ({ page }) => {
  const collection = await openReference(page);
  const order = () => openers(collection).evaluateAll((nodes) => nodes.map((n) => n.getAttribute("data-result")));
  const before = await order();
  const target = openers(collection).nth(1);
  await toggle(target);
  await expect.poll(order).toEqual(before);
  await target.focus();
  await target.press("ArrowDown");
  await expect(openers(collection).nth(2)).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(target).toBeFocused();
  await expect.poll(order).toEqual(before);
  await toggle(openers(collection).last());
  await expect.poll(order).toEqual(before);
  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

test("a uniquely direct result auto-expands in its stable row", async ({ page }) => {
  const collection = await openReference(page, "mixed", "図");
  const first = previews(collection).first();
  await expect(first).toContainText("図");
  await expect(opener(first)).toHaveAttribute("aria-expanded", "true");
  await expect(collection.locator(".result.expanded")).toHaveCount(1);

  if (await isWide(page)) {
    const detail = await collection.locator(".result.expanded").boundingBox();
    const bounds = await collection.boundingBox();
    expect(detail).not.toBeNull();
    expect(bounds).not.toBeNull();
    expect(detail!.width).toBeGreaterThan(bounds!.width * 0.8);
  }

  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

test("balanced previews keep heavyweight detail work lazy and expose stable accessibility", async ({ page }, testInfo) => {
  const collection = await openReference(page);
  for (const command of ["search_words", "search_grammar", "search_terms", "search_particles"]) {
    await expect.poll(() => commandCount(page, command)).toBe(1);
  }
  expect(await commandCount(page, "explain_tags")).toBe(0);
  // **One for the page, and only because the speak control is now on the front of a card.**
  // It used to be zero, which was right while nothing on a collapsed card could be played.
  // `pronunciation.want` batches a whole page of results into a single request, so this is the
  // bound that matters — not that it never asks, but that it never asks per card. Tags stay at
  // zero above: they are the expensive half and remain behind the chevron.
  await expect.poll(() => commandCount(page, "audio_availability")).toBe(1);
  await expect(collection.locator(".inline-detail")).toHaveCount(0);
  const previewNodes = await collection.locator("*").count();
  expect(previewNodes).toBeLessThan(190);

  await expect(collection).toMatchAriaSnapshot(`
    - list "Reference results":
      - listitem:
        - button "Copy 図書館": Copy this word to the clipboard
        - text: 図書館 としょかん
        - button "Hear 図書館": ▶ Hear this word
        - text: Word · Noun 1. library
        - button "Show details for 図書館"
      - listitem:
        - button "Copy 食べる": Copy this word to the clipboard
        - text: 食べる たべる Word · Verb 1. to eat
        - button "Show details for 食べる"
      - listitem:
        - button "Copy 早い": Copy this word to the clipboard
        - text: 早い はやい Word · Adjective 1. fast; early
        - button "Show details for 早い"
      - listitem:
        - button "Copy 相槌": Copy this word to the clipboard
        - text: 相槌 あいづち Word · Other 1. aizuchi; a listener's interjection
        - button "Show details for 相槌"
      - listitem:
        - button "Copy 道中": Copy this word to the clipboard
        - text: 道中 どうちゅう Word · Noun · Adverb 1. journey; trip; travels 2. during the journey; on the way; while travelling
        - button "Show details for 道中"
  `);

  await toggle(openers(collection).first());
  await expect.poll(() => commandCount(page, "explain_tags")).toBe(1);
  await expect(collection.locator(".inline-detail")).toHaveCount(1);
  await expect(page.getByRole("region", { name: "図書館 detail" })).toContainText("library");
  await page.evaluate(() => new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  ));

  const expanded = await collection.locator(".result.expanded").boundingBox();
  const following = await collection.locator(":scope > .result:not(.expanded)").first().boundingBox();
  expect(expanded).not.toBeNull();
  expect(following).not.toBeNull();
  expect(following!.y).toBeGreaterThanOrEqual(expanded!.y + expanded!.height - 1);
  const detailNodes = await collection.locator("*").count();
  expect(detailNodes).toBeLessThan(340);
  await testInfo.attach("reference-effect-boundary.json", {
    contentType: "application/json",
    body: JSON.stringify({ previewNodes, detailNodes, explainTagRequests: await commandCount(page, "explain_tags") }, null, 2),
  });

  if (testInfo.project.name === "edge-900-100") {
    await expect(collection).toHaveScreenshot("reference-workspace.png", { animations: "disabled" });
    await page.keyboard.press("Escape");
    await expect(collection).toHaveScreenshot("reference-rows.png", { animations: "disabled" });
  }
});

/**
 * **A card names every part of speech its word has, not the first one.**
 *
 * Two reports, one cause. Filtering to Adverb listed 道中 — correctly, it is a noun *and* an
 * adverb — and badged it NOUNS, so the list looked broken while being right. The same omission
 * hid why the counts overlap: 15 results reporting 15 nouns, 2 adverbs and 1 verb sums to 18,
 * which reads as three results missing. Naming every kind answers both, and is true of the
 * language: a Japanese word is often several parts of speech depending on the nuance.
 *
 * Asserted in a browser as well as in `reference-results.test.mjs` because the fixture had no
 * two-kind word until these reports: every other result belongs to exactly one category, so
 * nothing here could tell "the first kind" from "every kind".
 */
test("a card names every kind its word belongs to, filtered or not", async ({ page }) => {
  const collection = await openReference(page);
  const journey = previews(collection).filter({ hasText: "道中" });
  const single = previews(collection).filter({ hasText: "早い" });

  // Unfiltered: both memberships, so the counts row adding to more than the total has a
  // visible reason on the cards themselves.
  await expect(journey).toContainText("Noun");
  await expect(journey).toContainText("Adverb");
  // And a word with one kind gains nothing to read.
  await expect(single).toContainText("Adjective");
  await expect(single).not.toContainText("Noun");

  // Filtered: the card still names both, so it can never contradict the filter that found it.
  await page.getByRole("button", { name: /^Adverb 1$/ }).click();
  await expect(previews(collection)).toHaveCount(1);
  await expect(journey).toContainText("Adverb");
  await expect(journey).toContainText("Noun");

  await page.getByRole("button", { name: "Clear word filters" }).click();
  await expect(previews(collection)).toHaveCount(5);
  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

/**
 * **A filtered card describes itself in the terms that were asked for.**
 *
 * Part of speech in Japanese belongs to a sense, not a word. 道中 has a noun sense meaning
 * *journey* and an adverb sense meaning *on the way*; all-sense membership decided whether it
 * appeared and nothing downstream knew which sense earned it, so filtering to Adverb listed it
 * correctly and then printed its noun glosses underneath — the filter finding the right word
 * and saying the wrong thing about it.
 */
test("filtering to a kind shows the meanings of that kind", async ({ page }) => {
  const collection = await openReference(page);
  const journey = previews(collection).filter({ hasText: "道中" });

  // Unfiltered, the card leads with the word’s first senses, as every card does.
  await expect(journey).toContainText("journey; trip; travels");

  await page.getByRole("button", { name: /^Adverb 1$/ }).click();
  await expect(previews(collection)).toHaveCount(1);
  await expect(journey).toContainText("during the journey");
  await expect(journey).not.toContainText("journey; trip; travels");

  // And back, so the narrowing is a view and not a mutation of the result.
  await page.getByRole("button", { name: "Clear word filters" }).click();
  await expect(journey).toContainText("journey; trip; travels");
  await assertContained(page);
});

/**
 * **The two filter rows must not be told apart only by trying them.**
 *
 * Facets choose exactly one; parts of speech choose any number. They used to share a style rule
 * and both report `aria-pressed`, so neither the eye nor a screen reader could tell which press
 * would replace a choice and which would add to it. The row of parts of speech also carried its
 * `aria-label` on a bare div, where assistive technology ignores it.
 */
test("choosing one and choosing many are different controls", async ({ page }) => {
  const collection = await openReference(page);

  const facets = page.getByRole("radiogroup", { name: "Result types" });
  await expect(facets.getByRole("radio", { name: /^All / })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("radio", { name: /^Words 5$/ }).click();
  // One replaces: choosing Words leaves All unchosen.
  await expect(facets.getByRole("radio", { name: /^Words 5$/ })).toHaveAttribute("aria-checked", "true");
  await expect(facets.getByRole("radio", { name: /^All / })).toHaveAttribute("aria-checked", "false");

  const kinds = page.getByRole("group", { name: "Parts of speech" });
  const noun = kinds.getByRole("button", { name: /^Noun 2$/ });
  const adverb = kinds.getByRole("button", { name: /^Adverb 1$/ });
  await noun.click();
  await adverb.click();
  // Many accumulate: the second press does not release the first.
  await expect(noun).toHaveAttribute("aria-pressed", "true");
  await expect(adverb).toHaveAttribute("aria-pressed", "true");
  await expect(previews(collection)).toHaveCount(2);

  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

test("word filters are a disclosure and active scope stays visible when shut", async ({ page }) => {
  const collection = await openReference(page);
  const details = page.locator(".word-filter-bar details");
  await page.getByRole("button", { name: /^Noun 2$/ }).click();
  await details.locator("summary").click();
  await expect(page.getByText("Words only: Noun", { exact: false })).toBeVisible();
  await expect(page.getByRole("button", { name: "Clear word filters" })).toBeVisible();
  await page.getByRole("button", { name: "Clear word filters" }).click();
  await expect(previews(collection)).toHaveCount(5);
  await assertContained(page);
});

/**
 * **The card’s own chevron is the disclosure; nothing inside it needs opening again.**
 *
 * Expanding 道 used to answer with three more things to open — 20 more senses, 1 more example,
 * 16 name readings — so reading one entry took four clicks. The folds remain, as *collapse*
 * controls, which is what a 21-sense entry needs.
 *
 * The state lives in `Reference.svelte`; Analysis passes its own through `Vocabulary`, and
 * `analysis.spec.ts` still clicks "5 more senses" to open it. That pair passing unchanged is
 * what proves the two pages are not sharing a default.
 */
test("an opened card shows its meanings and examples without a second click", async ({ page }) => {
  const collection = await openReference(page);
  await toggle(openers(collection).first());
  const detail = page.getByRole("region", { name: "図書館 detail" });

  const examples = detail.locator("details.extra").filter({ hasText: "more example" });
  const second = detail.getByText("例文です2", { exact: false });
  await expect(examples).toHaveAttribute("open", "");
  // **Visibility, not text.** A closed `<details>` keeps its children in the DOM, so
  // `toContainText` passes either way — which is the same property that makes these folds free
  // to open: nothing is rendered on demand, only shown.
  await expect(second).toBeVisible();

  // Still a control: collapsing it sticks while this card stays open.
  await examples.locator("summary").click();
  await expect(examples).not.toHaveAttribute("open", "");
  await expect(second).toBeHidden();

  await assertContained(page);
});

/**
 * **Asking for a part of speech is asking for words.**
 *
 * Non-word answers used to be exempt from the parts-of-speech filter — the rule read
 * `result.type !== "word"`, which passed every grammar pattern, particle, kanji and term. On a
 * real search, filtering to Expression returned nine grammar patterns beside two words, and the
 * patterns dominated a list nobody had asked them into.
 *
 * The second half is the trap the fix could have introduced: the row is only drawn on the All
 * and Words facets, so a selection left over from All must not go on hiding grammar results on
 * a facet with nothing on screen to clear it.
 */
test("filtering by part of speech leaves only words, and only where the row is", async ({ page }) => {
  const collection = await openReference(page, "mixed", "図書館");
  const kinds = page.getByRole("group", { name: "Parts of speech" });

  // Unfiltered, the answer is a union: the word plus the grammar pattern the fixture supplies.
  await expect(previews(collection).filter({ hasText: "図書館" })).toHaveCount(1);
  // Located by the pattern itself, not by its badge: the badge now carries the JLPT level, so
  // matching on "Grammar pattern" pinned a label rather than the result it labels.
  const patterns = previews(collection).filter({ hasText: "〜てくる" });
  await expect(patterns).toHaveCount(1);

  await kinds.getByRole("button", { name: /^Noun 1$/ }).click();
  await expect(previews(collection).filter({ hasText: "図書館" })).toHaveCount(1);
  await expect(patterns).toHaveCount(0);

  // The row is not drawn on Grammar, so the leftover selection must not apply there.
  await page.getByRole("radio", { name: /^Grammar patterns 1$/ }).click();
  await expect(kinds).toHaveCount(0);
  await expect(patterns).toHaveCount(1);

  await assertContained(page);
});

/**
 * **A preview numbers the senses it shows, with the number each one actually has.**
 *
 * Two dim lines said nothing about what they were. The expanded detail hangs the same faint
 * ordinal beside every gloss, so numbering the preview makes it the truncation of one list and
 * carries the numbers over when the card opens.
 *
 * The number is the sense’s place in the *word*, not in the preview. Previews follow the
 * parts-of-speech filter, so 道中 under Adverb shows its second meaning — and calling that "1."
 * would be a new way of telling the reader the wrong thing about which meaning they are seeing.
 */
test("preview senses are numbered by their place in the word", async ({ page }) => {
  const collection = await openReference(page);
  const journey = previews(collection).filter({ hasText: "道中" });
  const numbers = () => journey.locator(".scent .n").allTextContents();

  await expect.poll(numbers).toEqual(["1.", "2."]);

  // Filtered to Adverb the second meaning is the one shown, and it keeps its own number.
  await page.getByRole("button", { name: /^Adverb 1$/ }).click();
  await expect.poll(numbers).toEqual(["2."]);

  // A result that is not a word has one summary line, not a numbered list.
  await page.getByRole("button", { name: "Clear word filters" }).click();
  await expect(previews(collection).filter({ hasText: "相槌" }).locator(".scent .n")).toHaveCount(1);

  await assertContained(page);
});

/**
 * The card says what was typed, and only when that differs from what was answered.
 *
 * **The negative half is the point.** A chip on every card says nothing; this one earns its space
 * because it appears exactly when the analyzer resolved a form on the reader's behalf — which on a
 * page for learners is the lesson rather than plumbing. The fixture carries one conjugated word and
 * one that was already its dictionary form, so a rule that always fired would fail here.
 */
test("a card reports the form that was typed when the answer differs from it", async ({ page }) => {
  const collection = await openReference(page, "inflected", "走った");
  const cards = collection.locator(":scope > .result");

  const resolved = cards.filter({ hasText: "走る" });
  await expect(resolved.locator(".flag")).toHaveText("as written 走った");

  const unchanged = cards.filter({ hasText: "道" });
  await expect(unchanged).toHaveCount(1);
  await expect(unchanged.locator(".flag")).toHaveCount(0);

  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

/**
 * The page admits when it stopped early.
 *
 * `ENGLISH_RESULTS` is a hard ceiling, and until now nothing said so — a reader who did not find
 * the word in twenty had no sign a twenty-first existed. No number: the count available cheaply is
 * of dictionary entries, and several spellings of one word are one card, so a figure taken from it
 * would overstate what is actually reachable.
 */
test("a capped search says so, and an uncapped one stays quiet", async ({ page }) => {
  const capped = await openReference(page, "capped", "run");
  await expect(capped).toBeVisible();
  await expect(page.locator(".capped")).toHaveText("More words matched than are shown.");

  // The same page, a fixture that returned everything it had. The notice is driven by the flag
  // rather than by counting the cards, which is what lets it be honest either way.
  const whole = await openReference(page, "parts", "eat");
  await expect(whole).toBeVisible();
  await expect(page.locator(".capped")).toHaveCount(0);

  await assertContained(page);
});

test("the expanded word leads with compact character choices, not character articles", async ({ page }) => {
  const collection = await openReference(page, "standard", "図書館");
  const word = await collection.locator(".word-column").boundingBox();
  const choices = await collection.locator(".character-choices").boundingBox();
  expect(choices!.y).toBeGreaterThanOrEqual(word!.y + word!.height);
  await expect(collection.locator(".character")).toHaveCount(0);
  await expect(collection.getByRole("button", { name: /^Study / })).toHaveCount(3);
  await assertContained(page);
});

/**
 * Two headings both read MEANINGS — the kanji's and the word's — stacked in one column. The
 * columns separate them now, and the name does too, so neither reading depends on the other.
 */
test("the character panel does not repeat the word's heading", async ({ page }) => {
  await page.goto("/?qa=reference");
  await page.getByRole("button", { name: "Reference" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill("図書館");
  await search.press("Enter");
  const detail = page.getByRole("list", { name: "Reference results" }).locator(".inline-detail");
  await expect(detail).toBeVisible();

  await detail.getByRole("button", { name: "Study 図", exact: true }).click();
  await expect(detail.locator(".character-inspector")).toContainText("Character meanings");
  const headings = await detail.locator(".section").allTextContents();
  const meanings = headings.filter((text) => text.trim().toLowerCase() === "meanings");
  expect(meanings, `one heading may say Meanings, got ${JSON.stringify(headings)}`).toHaveLength(1);
});

test("a wide Reference fills the page while prose keeps a readable measure", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  const collection = await openReference(page, "standard", "図書館");
  const detail = collection.locator(".inline-detail");
  const measured = await detail.evaluate((el) => ({
    detail: el.getBoundingClientRect().width,
    word: el.querySelector(".word-column")!.getBoundingClientRect().width,
  }));
  expect(measured.detail).toBeGreaterThan(1300);
  expect(measured.word).toBeLessThan(measured.detail * 0.7);
  await detail.getByRole("button", { name: "Study 図", exact: true }).click();
  const word = await detail.locator(".word-column").boundingBox();
  const inspector = await detail.locator(".character-inspector").boundingBox();
  expect(inspector!.x).toBeGreaterThanOrEqual(word!.x + word!.width);
  await assertContained(page);
});

/**
 * Senses of different parts of speech are drawn in different colours.
 *
 * 道中 is the fixture that can prove it: one noun sense and one adverb sense, which is why it was
 * put there in the first place. **The rule lives on the sense row, not only on the group heading**
 * — a heading is drawn only where a kind covers several senses, so a lone adverb between two
 * pronouns would have had no heading to colour, and that is exactly the change of kind a reader
 * most needs to see.
 */
test("senses of different kinds are drawn in different colours", async ({ page }) => {
  const collection = await openReference(page, "parts", "eat");
  const card = collection.locator(":scope > .result").filter({ hasText: "道中" });
  const tab = card.locator("> .disclose");
  await toggle(tab);
  await expect(card.locator(".inline-detail")).toBeVisible();

  const rules = await card.locator(".inline-detail .sense").evaluateAll((rows) =>
    rows.map((row) => getComputedStyle(row).boxShadow));
  expect(rules.length, "the noun sense and the adverb sense").toBe(2);
  for (const rule of rules) {
    expect(rule, "every classified sense carries a rule").toContain("inset");
  }
  expect(rules[0]).not.toBe(rules[1]);

  // Filters now use a neutral selected treatment; sense classification colours remain.
  const chip = page.getByRole("button", { name: /^Noun/ });
  await chip.click();
  await expect(chip).toHaveAttribute("aria-pressed", "true");

  await assertContained(page);
});

/**
 * Function words are classified too.
 *
 * **The gap this closes.** The five kinds the filter row offers are content words, so が — a
 * particle, and a first-class result type on this page with a detail of its own — belonged to
 * none of them and was the one word drawn in no colour at all. 何 lost its interjection and
 * prefix senses the same way, in the middle of a list whose other senses were coloured.
 */
test("a particle's senses are classified like any other word's", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=function");
  await page.getByRole("button", { name: "Reference" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill("が");
  await search.press("Enter");
  const detail = page.getByRole("list", { name: "Reference results" }).locator(".inline-detail");
  await expect(detail).toBeVisible();

  const rules = await detail.locator(".sense").evaluateAll((rows) =>
    rows.map((row) => getComputedStyle(row).boxShadow));
  expect(rules.length, "particle, conjunction and a noun").toBe(3);
  for (const rule of rules) {
    expect(rule, "every classified sense carries a rule, function words included").toContain("inset");
  }
  // The particle and the conjunction share the Function hue; the noun does not.
  expect(rules[0]).toBe(rules[1]);
  expect(rules[2]).not.toBe(rules[0]);

  await assertContained(page);
});

test("requested character facts read from a consistent left edge", async ({ page }) => {
  const collection = await openReference(page, "standard", "図書館");
  await collection.getByRole("button", { name: "Study 図", exact: true }).click();
  const facts = collection.locator(".facts");
  await expect(facts).toBeVisible();
  const positions = await facts.locator("dt").evaluateAll((nodes) => nodes.map((n) => n.getBoundingClientRect().left));
  expect(Math.max(...positions) - Math.min(...positions)).toBeLessThan(1);
  expect(await facts.locator("dd").first().evaluate((el) => getComputedStyle(el).justifyContent)).toBe("flex-start");
  await assertContained(page);
});

/**
 * **Robustness of the two-step click.** Each of these is a gesture a reader makes without
 * thinking, and each one used to leave the card stuck.
 */
test("repeated clicks keep toggling a card", async ({ page }) => {
  const collection = await openReference(page);
  const start = collection.locator(":scope > .result").nth(1);
  const key = await start.locator("> .disclose").getAttribute("data-result");
  // Follow the key: opening promotes the card, so any positional locator goes stale immediately.
  const tab = collection.locator(`.disclose[data-result="${key}"]`);
  // **The preview, not the whole card.** Playwright aims at an element's centre, and once the card
  // is open its centre is inside `.inline-detail` — which deliberately ignores clicks, so half of
  // this burst would land on the one part of the card that is not a target. The preview means the
  // same thing in both states.
  const body = collection.locator(`.result:has(> .disclose[data-result="${key}"]) .preview`);

  await expect(tab).toHaveAttribute("aria-expanded", "false");
  // Four in a row, at the same spot and at speed — which is what makes Chromium call them
  // double- and triple-clicks and start selecting words under the pointer.
  for (const expected of ["true", "false", "true", "false"]) {
    await body.click();
    await expect(tab).toHaveAttribute("aria-expanded", expected);
  }
});

test("a double-click toggles twice without disabling later single clicks", async ({ page }) => {
  const collection = await openReference(page);
  const start = collection.locator(":scope > .result").nth(1);
  const key = await start.locator("> .disclose").getAttribute("data-result");
  const tab = collection.locator(`.disclose[data-result="${key}"]`);
  const body = collection.locator(`.result:has(> .disclose[data-result="${key}"]) > .card`);

  // Two clicks open then close; browser word selection must not swallow either event.
  await body.dblclick();
  await expect(tab).toHaveAttribute("aria-expanded", "false");
  await clickCard(start);
  await expect(tab).toHaveAttribute("aria-expanded", "true");
});

test("a selection made elsewhere does not freeze a card", async ({ page }) => {
  const collection = await openReference(page);
  const start = collection.locator(":scope > .result").nth(1);
  const key = await start.locator("> .disclose").getAttribute("data-result");
  const tab = collection.locator(`.disclose[data-result="${key}"]`);

  // Select something on a different card and leave it selected.
  const other = collection.locator(":scope > .result").nth(2).locator(".identity").first();
  const box = await other.boundingBox();
  await page.mouse.move(box!.x + 2, box!.y + box!.height / 2);
  await page.mouse.down();
  await page.mouse.move(box!.x + box!.width - 2, box!.y + box!.height / 2, { steps: 6 });
  await page.mouse.up();
  expect(await page.evaluate(() => window.getSelection()?.toString() ?? "")).not.toBe("");

  // A stale selection anywhere on the page is not a reason to ignore a click over here.
  await clickCard(start);
  await expect(tab).toHaveAttribute("aria-expanded", "true");
  await clickCard(start);
  await expect(tab).toHaveAttribute("aria-expanded", "false");
});

/**
 * **A burst of clicks as fast as the mouse can send them.**
 *
 * The reported failure: clicking a card over and over wedges it, and it only comes back after the
 * pointer is moved "a certain way". Chromium raises the click count on each press in a burst and
 * starts selecting the word, then the line, under the pointer — so this is where a guard that asks
 * whether anything is selected falls over, and it is the gesture a reader makes without thinking.
 *
 * The first card is deliberate: it is already picked and already at the head of the collection, so
 * opening it promotes nothing and one fixed point stays over the same card for the whole burst.
 */
test("clicking as fast as the mouse can send it does not wedge a card", async ({ page }) => {
  const collection = await openReference(page);
  const card = collection.locator(":scope > .result").first();
  const tab = card.locator("> .disclose");
  await expect(tab).toHaveAttribute("aria-expanded", "false");

  // The kind label: plain text, no control under it, and it does not move when the card opens.
  const spot = await card.locator(".classification").first().boundingBox();
  const x = spot!.x + spot!.width / 2;
  const y = spot!.y + spot!.height / 2;

  // **Every press is checked, not just the last one.** Asserting only the final state lets this
  // pass for the wrong reason: an even number of swallowed clicks leaves the parity intact and
  // the card looks fine at the end.
  //
  // What this does *not* pin down is the selection bug — verified by reverting the fix, and worth
  // recording because it is not obvious. Each press toggles the card, which re-renders it, and
  // Chromium restarts its click count when the element under the pointer changes; so these never
  // compound into a double-click. Sustained responsiveness is what this test is for. The two
  // tests below keep the click count alive, and those are the ones that fail without the fix.
  const seen: (string | null)[] = [];
  for (let n = 0; n < 8; n += 1) {
    await page.mouse.click(x, y);
    seen.push(await tab.getAttribute("aria-expanded"));
  }
  expect(seen).toEqual(["true", "false", "true", "false", "true", "false", "true", "false"]);
});

test("a triple-click selects a line without wedging the card", async ({ page }) => {
  const collection = await openReference(page);
  const card = collection.locator(":scope > .result").first();
  const tab = card.locator("> .disclose");
  const spot = card.locator(".classification").first();

  const box = await spot.boundingBox();
  const x = box!.x + box!.width / 2;
  const y = box!.y + box!.height / 2;

  // Three presses in place: the browser selects the word and then the line, and all three still
  // have to reach the card. Checked one at a time — a final-state assertion cannot tell three
  // presses that worked from one that worked and two that were swallowed.
  const seen: (string | null)[] = [];
  for (let n = 0; n < 3; n += 1) {
    await page.mouse.click(x, y);
    seen.push(await tab.getAttribute("aria-expanded"));
  }
  expect(seen).toEqual(["true", "false", "true"]);
  // Nothing is claimed here about what stayed selected: opening and closing the card between the
  // presses re-renders it and drops the selection's anchor, which is the app's own churn rather
  // than anything this rule decides. That a drag keeps its selection *and* leaves the card alone
  // is asserted where it is stable — "copying a word and selecting text never open a card".
});

test("a hand that drifts a pixel is still clicking, and a drag off the card is not", async ({ page }) => {
  const collection = await openReference(page);
  const card = collection.locator(":scope > .result").first();
  const tab = card.locator("> .disclose");
  const spot = await card.locator(".classification").first().boundingBox();
  const x = spot!.x + spot!.width / 2;
  const y = spot!.y + spot!.height / 2;

  // Within the slop: a press that drifts two pixels is a press. Requiring perfect stillness would
  // fail for a shaky hand before it failed for anyone else.
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 2, y + 1);
  await page.mouse.up();
  await expect(tab).toHaveAttribute("aria-expanded", "true");

  // Past it: a press that leaves the card was reaching for something else.
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 90, y + 4, { steps: 8 });
  await page.mouse.up();
  await expect(tab).toHaveAttribute("aria-expanded", "true");
});

/**
 * **Double-clicks, one after another — the reported gesture, at its worst.**
 *
 * `dblclick` sends both presses in one batch, so the click count really does reach two and
 * Chromium really does select the word underneath. Repeating it is a reader clicking a card over
 * and over: without the fix the first double-click selects a word, every later press is swallowed,
 * and the card only comes back once something clears the selection — "unless I shift the cursor in
 * a certain way", exactly as reported.
 */
test("double-clicking over and over keeps the card answering", async ({ page }) => {
  const collection = await openReference(page);
  const card = collection.locator(":scope > .result").first();
  const tab = card.locator("> .disclose");
  const spot = await card.locator(".classification").first().boundingBox();
  const x = spot!.x + spot!.width / 2;
  const y = spot!.y + spot!.height / 2;

  // Two presses each, so the card lands back where it started every time — and has to have moved
  // twice in between, which is what the reads check.
  const seen: (string | null)[] = [];
  for (let n = 0; n < 4; n += 1) {
    await page.mouse.dblclick(x, y);
    seen.push(await tab.getAttribute("aria-expanded"));
  }
  expect(seen).toEqual(["false", "false", "false", "false"]);
  // And a single press still moves it, rather than the card having quietly stopped listening.
  await page.mouse.click(x + 1, y + 1);
  await expect(tab).toHaveAttribute("aria-expanded", "true");
});

/**
 * **An open card's answer is not a close button.**
 *
 * The shut card is one big target, which is what makes it easy to open. Once it is open the detail
 * is the thing the reader asked for, and a stray press in the middle of a definition taking it
 * away again is the same mistake as making the whole card a button was. The preview still closes
 * it, and so does the card's control from the keyboard.
 */
test("clicking inside an open card's detail never closes it", async ({ page }) => {
  const collection = await openReference(page);
  const start = collection.locator(":scope > .result").nth(1);
  const key = await start.locator("> .disclose").getAttribute("data-result");
  const tab = collection.locator(`.disclose[data-result="${key}"]`);
  const card = collection.locator(`.result:has(> .disclose[data-result="${key}"])`);

  await card.locator("> .card").click();
  await card.locator("> .card").click();
  await expect(tab).toHaveAttribute("aria-expanded", "true");

  // Prose in the middle of the answer, twice — the second press is the one that would have
  // toggled it back had the first been swallowed rather than ignored.
  const detail = card.locator(".inline-detail");
  await detail.click({ position: { x: 6, y: 6 } });
  await expect(tab).toHaveAttribute("aria-expanded", "true");
  await detail.click({ position: { x: 6, y: 6 } });
  await expect(tab).toHaveAttribute("aria-expanded", "true");

  // The preview above it still answers, and so does the card's control from the keyboard.
  await card.locator(".preview").click();
  await expect(tab).toHaveAttribute("aria-expanded", "false");
  await tab.focus();
  await tab.press("Enter");
  await expect(tab).toHaveAttribute("aria-expanded", "true");
  await tab.press("Enter");
  await expect(tab).toHaveAttribute("aria-expanded", "false");
});

test("all word characters remain available while only one article opens at a time", async ({ page }) => {
  const collection = await openReference(page, "standard", "図書館", "&keyboard=tab");
  for (const character of ["図", "書", "館"]) {
    await collection.getByRole("button", { name: `Study ${character}`, exact: true }).click();
    await expect(collection.locator(".character")).toHaveCount(1);
    await expect(collection.getByRole("complementary", { name: `Details for ${character}` })).toBeVisible();
    await expect(collection.getByRole("img", { name: `Stroke-order diagram for ${character}` })).toBeVisible();
  }
  await collection.getByRole("button", { name: "Close character details" }).click();
  await expect(collection.locator(".character")).toHaveCount(0);
  await expect(collection.getByRole("button", { name: "Study 館", exact: true })).toBeFocused();
});

/**
 * **A sense and an example start on the same line.**
 *
 * Both are lists under a heading in one card, and both are drawn with a hairline down their left —
 * but by different means: an example uses `border-left`, which takes layout, and a sense uses an
 * inset `box-shadow`, which does not. Matching their paddings therefore does *not* match their
 * text, and the two rules are a dozen files apart, so nothing but this notices when one moves.
 */
test("senses and examples share one left edge", async ({ page }) => {
  await page.goto("/?qa=reference");
  await page.getByRole("button", { name: "Reference" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill("図書館");
  await search.press("Enter");
  const detail = page.getByRole("list", { name: "Reference results" }).locator(".inline-detail");
  await expect(detail).toBeVisible();

  const edges = await detail.evaluate((el) => {
    // The text, not the box: both boxes span the column whatever their padding says.
    const textLeft = (node: Element | null) => {
      if (!node) return null;
      const range = document.createRange();
      range.selectNodeContents(node);
      return range.getBoundingClientRect().left;
    };
    return {
      // The badge row and the ordinal move together, so either one answers for the sense.
      badges: textLeft(el.querySelector(".word-column .sense .tags")),
      ordinal: textLeft(el.querySelector(".word-column .sense .n")),
      example: textLeft(el.querySelector(".word-column .example.ruled")),
    };
  });

  expect(edges.example, "the card has an example to line up with").not.toBeNull();
  expect(edges.badges, "the card has a classified sense").not.toBeNull();
  expect(Math.abs(edges.badges! - edges.example!)).toBeLessThan(1);
  expect(Math.abs(edges.ordinal! - edges.example!), "the number moved with the badges").toBeLessThan(1);
});


/**
 * **A reading that only lives inside a compound says so with a wave dash.**
 *
 * KANJIDIC2 marks those with an ASCII hyphen — `-がき` is the 書き of 走り書き and never a word on
 * its own. The mark carries real information and stays; the character does not, because `〜` is
 * what this app writes its grammar patterns with and what Japanese uses to say "something goes
 * here". The stored data keeps the hyphen: the Rust matcher trims `'-'` and reads it directly.
 */
test("a suffix-only kanji reading is written with a wave dash", async ({ page }) => {
  await page.goto("/?qa=reference");
  await page.getByRole("button", { name: "Reference" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill("図書館");
  await search.press("Enter");
  const detail = page.getByRole("list", { name: "Reference results" }).locator(".inline-detail");
  await expect(detail).toBeVisible();
  await detail.getByRole("button", { name: "Study 書", exact: true }).click();

  const readings = await detail.locator(".facts .reading").allTextContents();
  expect(readings, "the marked reading is written with a wave dash").toContain("〜がき");
  expect(readings, "and no longer with a hyphen").not.toContain("-がき");
});

/**
 * **The kanji's own sound, and the kana written beside it, told apart by tone.**
 *
 * KANJIDIC writes `か.く`, and at this size a baseline dot between two kana is easy to look past —
 * which means misreading which sound the character is actually responsible for. That is the one
 * fact the row exists to teach, so it is carried by a difference in tone rather than by a mark:
 * every mark that could go there is spoken for (`〜` is an affix, `・` joins spellings) or unsafe
 * (a dash beside kana reads as `ー`).
 */
test("okurigana is drawn a step fainter than the reading it follows", async ({ page }) => {
  await page.goto("/?qa=reference");
  await page.getByRole("button", { name: "Reference" }).click();
  const search = page.getByRole("textbox", { name: "Search Reference" });
  await search.fill("図書館");
  await search.press("Enter");
  const detail = page.getByRole("list", { name: "Reference results" }).locator(".inline-detail");
  await expect(detail).toBeVisible();
  await detail.getByRole("button", { name: "Study 書", exact: true }).click();

  // The period is gone from what is drawn, and both halves survive it.
  const readings = await detail.locator(".facts .reading").allTextContents();
  expect(readings, "no separator is printed").not.toContain("か.く");
  expect(readings, "and neither half was dropped with it").toContain("かく");

  const split = detail.locator(".facts .reading").filter({ hasText: "かく" }).first();
  await expect(split.locator(".okurigana")).toHaveText("く");

  // Fainter, measured rather than assumed — the whole signal is that these two differ.
  const shades = await split.evaluate((el) => {
    const okurigana = el.querySelector(".okurigana")!;
    const paint = (node: Element) => {
      const cs = getComputedStyle(node);
      return { colour: cs.color, opacity: Number(cs.opacity), weight: cs.fontWeight };
    };
    return { stem: paint(el), okurigana: paint(okurigana) };
  });
  expect(shades.okurigana.opacity).toBeLessThan(shades.stem.opacity);
  expect(shades.okurigana.opacity).toBeGreaterThan(0.3);
});

/**
 * **The headword row groups what belongs together.**
 *
 * It had one gap — `--gap-xs` — for three different relationships: an action against its object,
 * two halves of one identity, and a second action. At four pixels nothing was grouped, so a
 * 23px headword, its reading and two icons read as one undifferentiated run.
 *
 * Asserted as a *relationship* rather than as pixel counts: the point is that the identity holds
 * together more tightly than it holds to the controls, and that stays true if the gap scale is
 * ever retuned. Absolute values would fail on a token change that broke nothing.
 */
test("the headword and its reading group more tightly than the controls around them", async ({ page }) => {
  const collection = await openReference(page, "parts", "eat");
  // A result with a reading and both controls, so all four neighbours exist to measure.
  const line = collection.locator(".preview .word-line").filter({ has: page.locator(".reading") }).first();
  await expect(line).toBeVisible();

  const gaps = await line.evaluate((el) => {
    const box = (sel: string) => el.querySelector(sel)?.getBoundingClientRect() ?? null;
    const copy = box("button[aria-label^='Copy']");
    const word = box(".identity");
    const reading = box(".reading");
    // The speak control only exists for a word with a recording. 図書館 has one in this fixture,
    // and the assertions below require it rather than skipping quietly — a conditional here is
    // how the second half of this test would stop running without anyone noticing.
    const speak = box("button[aria-label^='Hear']");
    return {
      identity: word && reading ? reading.left - word.right : null,
      beforeWord: copy && word ? word.left - copy.right : null,
      afterReading: reading && speak ? speak.left - reading.right : null,
    };
  });

  expect(gaps.identity, "the word and its reading are both drawn").not.toBeNull();
  expect(gaps.beforeWord, "a copy control precedes the word").not.toBeNull();
  expect(gaps.afterReading, "a speak control follows the reading").not.toBeNull();
  expect(gaps.identity!).toBeGreaterThanOrEqual(0);
  expect(
    gaps.beforeWord!,
    "the copy control stands further from the word than the reading does",
  ).toBeGreaterThan(gaps.identity! + 2);
  expect(
    gaps.afterReading!,
    "and so does the speak control",
  ).toBeGreaterThan(gaps.identity! + 2);
});

test("at ordinary widths the requested character stacks below the word", async ({ page }) => {
  const collection = await openReference(page, "standard", "図書館");
  await collection.getByRole("button", { name: "Study 図", exact: true }).click();
  const word = await collection.locator(".word-column").boundingBox();
  const inspector = await collection.locator(".character-inspector").boundingBox();
  expect(inspector!.y).toBeGreaterThanOrEqual(word!.y + word!.height);
});


/**
 * **A result's hidden control is positioned against its own list item**, as the glossary's must
 * be. Unanchored, the controls sat down a column as tall as the whole list that stretched the
 * window's frame, and focusing one far down scrolled the frame itself — the title bar and page slid
 * off the top. Found in the glossary on 2026-10-19; the same shared `.disclose` is used here.
 */
test("every result's hidden control belongs to its own item, and the frame has nothing to scroll", async ({
  page,
}) => {
  await openReference(page);
  const controls = page.locator(".result > .disclose");
  await expect(controls.first()).toBeAttached();
  const anchored = await controls.evaluateAll((all) =>
    all.every((control) => (control as HTMLElement).offsetParent === control.parentElement),
  );
  expect(anchored).toBe(true);
  const spare = await page.evaluate(() => {
    const app = document.querySelector(".app")!;
    return app.scrollHeight - app.clientHeight;
  });
  expect(spare).toBe(0);
});

/**
 * **A rare or old spelling says so, rather than looking like the headword repeated.** 散歩 lists 散步
 * — 步 is the older form of 歩, a stroke apart — and the page printed "Also 散步" with nothing to
 * tell the two apart, which read as a duplicate. The dictionary scores such forms below zero and the
 * search already carried that as `rare`; it is now on the page. Real data: Jitendex seq 1303620.
 */
test("a rare or old spelling is marked as one", async ({ page }) => {
  const collection = await openReference(page, "forms", "散歩");
  // A lone result may already be open; opening it only when it is not keeps this about the form.
  const card = openers(collection).first();
  if ((await card.getAttribute("aria-expanded")) !== "true") await toggle(card);
  await expect(card).toHaveAttribute("aria-expanded", "true");
  const form = collection.locator(".forms .form").first();
  await expect(form).toContainText("散步");
  await expect(form).toHaveClass(/rare/);
  await expect(form).toContainText("rare or old form");
  await assertContained(page);
});
