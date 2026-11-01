import type { Locator, Page } from "@playwright/test";
import { assertContained, assertPageDoesNotScrollSideways } from "./contained";
import { expect, test } from "./fixtures";

async function openGlossary(page: Page, keyboard = false) {
  await page.goto(`/?qa=reference&fixture=glossary${keyboard ? "&keyboard=tab" : ""}`);
  await page.getByRole("button", { name: "Glossary" }).click();
  const collection = page.getByRole("navigation", { name: "Glossary terms" });
  await expect(collection).toBeVisible();
  return collection;
}


function previews(collection: Locator) {
  return collection.locator(".term > .card > .preview");
}

/**
 * The control that opens a term: a real button, drawn as nothing.
 *
 * It carries `aria-expanded`, the roving tabindex and the keys, so tests read state from it and
 * focus it. A pointer cannot click it — the visible tab it used to be was removed on 2026-10-17 —
 * so every click goes to the card itself, through `toggle` below. Filtering still happens on the
 * preview, because the button carries no text.
 */
function openers(collection: Locator) {
  return collection.locator(".term > .disclose");
}

function opener(preview: Locator) {
  // **Up to the list item, not across.** The control is a sibling of the *card*, not of the
  // preview. `ancestor::li` survives whatever nesting the card grows next.
  return preview.locator("xpath=ancestor::li[1]").locator("> .disclose");
}

/**
 * Click a term the way a reader does: in the preview's bottom-left padding, where there is never
 * a button or a run of text. The centre can land on a control inside the preview, and once the
 * term is open the card's centre is inside `.inline-detail`, which deliberately ignores clicks.
 */
async function clickCard(item: Locator) {
  const preview = item.locator("> .card > .preview");
  const box = await preview.boundingBox();
  await preview.click({ position: { x: 4, y: box!.height - 3 } });
}


/** Open or close a term through its control's list item, the way a reader does. */
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

/**
 * **One column, at every width, with no control offering another.**
 *
 * The glossary used to make Reference's Cards/Rows choice. It is a list of names read top to
 * bottom under category headings, and the grid only ever bought density at the cost of a summary's
 * measure — so what is left is what the narrow case always drew. Asserted at every viewport,
 * including the wide one where a grid *could* fit, because that is the case that would regress.
 */
test("the glossary is one column at every width and offers no layout choice", async ({ page }) => {
  const collection = await openGlossary(page);
  const items = previews(collection);
  await expect(items).toHaveCount(4);
  await expect(collection.locator(".inline-detail")).toHaveCount(0);

  // The last two terms share the Parts of speech heading, so this measures arrangement rather
  // than grouping: in one column they start at the same x whatever the window is doing.
  const first = await items.nth(2).boundingBox();
  const second = await items.nth(3).boundingBox();
  expect(first).not.toBeNull();
  expect(second).not.toBeNull();
  expect(Math.abs(first!.x - second!.x)).toBeLessThan(2);
  await expect(page.getByRole("button", { name: /Show as rows|Show as cards/ })).toHaveCount(0);

  const tabs = openers(collection);
  await toggle(tabs.nth(0));
  await expect(page.getByRole("region", { name: "Conjunctive form detail" })).toBeVisible();
  await expect(collection.locator(".inline-detail")).toHaveCount(1);
  await toggle(tabs.nth(1));
  await expect(page.getByRole("region", { name: "Conjunctive form detail" })).toHaveCount(0);
  await expect(page.getByRole("region", { name: "Auxiliary detail" })).toBeVisible();
  await expect(collection.locator(".inline-detail")).toHaveCount(1);
  // Still `nth(1)`: Auxiliary is the only term under CONCEPTS, and promotion is within a section,
  // so opening it moved nothing.
  await toggle(tabs.nth(1));
  await expect(collection.locator(".inline-detail")).toHaveCount(0);

  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
});

test("normalised filtering exposes the alias that caused the match and uses safe marks", async ({ page }) => {
  const collection = await openGlossary(page);
  await page.getByRole("textbox", { name: "Find a glossary term" }).fill("TE-FORM");
  await expect(previews(collection)).toHaveCount(1);
  const preview = previews(collection).filter({ hasText: "Conjunctive form" });
  await expect(preview).toContainText("Matched “te-form”");
  await expect(preview.locator("mark")).toHaveText("te-form");
});

test("the filter reaches a term by its names and not through its prose", async ({ page }) => {
  const collection = await openGlossary(page);
  // "object" is in both verb terms' summaries and in exactly one term's aliases — so one query
  // separates the two rules. Full-text matching returned Transitive and Intransitive alike.
  await page.getByRole("textbox", { name: "Find a glossary term" }).fill("object");
  await expect(previews(collection)).toHaveCount(1);
  const preview = previews(collection).first();
  await expect(preview).toContainText("Transitive verb");
  await expect(preview).toContainText("Matched “verb taking an object”");
  // The summary still says "object" and must not claim that is why the card is here.
  await expect(preview.locator("mark")).toHaveText("object");
});

test("opening a term preserves its category and alphabetical positions", async ({ page }) => {
  const collection = await openGlossary(page);
  const order = () => collection.locator(".preview strong").allTextContents();
  const before = await order();
  await toggle(openers(collection).last());
  await expect.poll(order).toEqual(before);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "A–Z", exact: true }).click();
  const alpha = await order();
  await toggle(openers(collection).nth(2));
  await expect.poll(order).toEqual(alpha);
  await assertContained(page);
});

test("similar terms stay visible and move the sole expansion", async ({ page }) => {
  const collection = await openGlossary(page, true);
  await toggle(opener(previews(collection).filter({ hasText: "Conjunctive form" })));
  const conjunctive = page.getByRole("region", { name: "Conjunctive form detail" });
  await expect(conjunctive.getByRole("region", { name: "Similar terms", exact: true })).toBeVisible();
  await expect(conjunctive.locator("details")).toHaveCount(0);
  await expect(conjunctive).not.toContainText("Connected grammar patterns");
  expect(await commandCount(page, "patterns_for_term")).toBe(0);
  await conjunctive.getByRole("button", { name: "Auxiliary" }).click();
  await expect(conjunctive).toHaveCount(0);
  const auxiliary = page.getByRole("region", { name: "Auxiliary detail" });
  await expect(auxiliary).toBeVisible();
  await expect(auxiliary.getByRole("button", { name: "Conjunctive form", exact: true })).toHaveCount(1);
  await expect(collection.locator('.disclose[data-term="aux"]')).toBeFocused();
  await expect(collection.locator(".inline-detail")).toHaveCount(1);
  await assertContained(page);
});

test("similar-term navigation clears only a filter hiding its destination", async ({ page }) => {
  const collection = await openGlossary(page, true);
  const filter = page.getByRole("textbox", { name: "Find a glossary term" });
  await filter.fill("te-form");
  await toggle(openers(collection).first());
  await page.getByRole("region", { name: "Conjunctive form detail" }).getByRole("button", { name: "Auxiliary", exact: true }).press("Enter");
  await expect(filter).toHaveValue("");
  await expect(collection.locator('.disclose[data-term="aux"]')).toHaveAttribute("aria-expanded", "true");
  await expect(collection.locator('.disclose[data-term="aux"]')).toBeFocused();
  await expect(collection.locator('.term[data-item="aux"] .preview')).toBeInViewport();

  await filter.fill("verb");
  await page.getByRole("button", { name: "A–Z", exact: true }).click();
  const order = await previews(collection).allTextContents();
  await toggle(collection.locator('.disclose[data-term="vt"]'));
  await page.getByRole("region", { name: "Transitive verb detail" }).getByRole("button", { name: "Intransitive verb", exact: true }).click();
  await expect(filter).toHaveValue("verb");
  await expect(page.getByRole("region", { name: "Intransitive verb detail" })).toBeVisible();
  await expect(previews(collection)).toHaveText(order);
  await assertContained(page);
});

test("an unlinked term hides the similar-terms section", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=glossary");
  await expect(page.getByRole("button", { name: "Glossary", exact: true })).toBeVisible();
  await page.evaluate(() => {
    const qa = (window as Window & { __SAKATSU_QA__: { answerNext: (command: string, value: unknown) => void } }).__SAKATSU_QA__;
    qa.answerNext("all_terms", [{ id: "unlinked", label: "Unlinked term", aliases: [], summary: "A standalone test concept.", detail: null, examples: [], requires: [], contrasts: [], related: [], kind: "concept" }]);
  });
  await page.getByRole("button", { name: "Glossary", exact: true }).click();
  const collection = page.getByRole("navigation", { name: "Glossary terms" });
  await toggle(openers(collection).first());
  const similar = page.getByRole("region", { name: "Similar terms", exact: true });
  await expect(similar).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Similar terms", exact: true })).toHaveCount(0);
  expect(await commandCount(page, "patterns_for_term")).toBe(0);
});

test("Glossary uses roving focus and Escape restores the expanded preview", async ({ page }) => {
  const collection = await openGlossary(page);
  const items = openers(collection);
  await items.first().focus();
  await items.first().press("End");
  await expect(items.last()).toBeFocused();
  await items.last().press("Home");
  await expect(items.first()).toBeFocused();
  await items.first().press("ArrowDown");
  await expect(items.nth(1)).toBeFocused();

  // Follow the term, not the position: opening one promotes it to the front of its section, so
  // `nth(1)` is a different term the moment Space lands and back to this one after Escape.
  const key = await items.nth(1).getAttribute("data-term");
  const opened = collection.locator(`.disclose[data-term="${key}"]`);
  await items.nth(1).press("Space");
  await expect(opened).toHaveAttribute("aria-expanded", "true");
  await page.keyboard.press("Escape");
  await expect(opened).toHaveAttribute("aria-expanded", "false");
  await expect(opened).toBeFocused();
});

test("the collection keeps a stable accessible hierarchy and a deterministic single column", async ({ page }, testInfo) => {
  const collection = await openGlossary(page);
  await expect.poll(() => commandCount(page, "all_terms")).toBe(1);
  expect(await commandCount(page, "patterns_for_term")).toBe(0);
  const previewNodes = await collection.locator("*").count();
  expect(previewNodes).toBeLessThan(140);
  await expect(collection).toMatchAriaSnapshot(`
    - navigation "Glossary terms":
      - heading "Forms 1" [level=3]
      - list:
        - listitem:
          - strong: Conjunctive form
          - text: A form that connects a word to the construction that follows it.
          - button "Show details for Conjunctive form"
      - heading "Concepts 1" [level=3]
      - list:
        - listitem:
          - strong: Auxiliary
          - text: A supporting word that adds tense, voice, direction, or another grammatical meaning.
          - button "Show details for Auxiliary"
      - heading "Parts of speech 2" [level=3]
      - list:
        - listitem:
          - strong: Transitive verb
          - text: A verb whose action is directed at an object.
          - button "Show details for Transitive verb"
        - listitem:
          - strong: Intransitive verb
          - text: A verb describing an event without directing the action at an object.
          - button "Show details for Intransitive verb"
  `);

  // Pinned here rather than in the snapshot above, for the reason `reference.spec.ts` records:
  // the generator omits `[expanded=false]`, and the tab is invisible until hovered, so this is
  // the only thing telling a screen reader that a term can be opened at all.
  for (const tab of await openers(collection).all()) {
    await expect(tab).toHaveAttribute("aria-expanded", "false");
    await expect(tab).toHaveAttribute("aria-controls", /glossary-detail-/);
  }

  if (testInfo.project.name === "edge-900-100") {
    await toggle(openers(collection).first());
    await expect(page.getByRole("region", { name: "Conjunctive form detail" })).toContainText("Try eating it.");
    expect(await commandCount(page, "patterns_for_term")).toBe(0);
    const detailNodes = await collection.locator("*").count();
    expect(detailNodes).toBeLessThan(260);
    await testInfo.attach("glossary-effect-boundary.json", {
      contentType: "application/json",
      body: JSON.stringify({ previewNodes, detailNodes, patternRequests: await commandCount(page, "patterns_for_term") }, null, 2),
    });
    await expect(collection).toHaveScreenshot("glossary-workspace.png", { animations: "disabled" });
  }
});

/**
 * **The same two steps as Reference, over this page's own roving key.**
 *
 * Clicking anywhere on a term used to open it, and that was removed because it left no quiet
 * place to select a summary from. This is that gesture returning with a step in front of it and
 * the guards that keep the summary selectable — so the removal is not simply undone.
 */
test("a term opens on its first click with the legacy preference enabled", async ({ page }) => {
  const collection = await openGlossary(page);
  await expect(collection).toHaveAttribute("data-cards", "single");
  // Not the first term: the roving key starts there, so it is already picked — the one card on
  // the page that cannot show what a term at rest looks like.
  const term = collection.locator(".term").nth(1);
  const tab = term.locator("> .disclose");

  await expect(term).not.toHaveAttribute("data-focused", "");

  await clickCard(term);
  await expect(term).toHaveAttribute("data-focused", "");
  await expect(tab).toHaveAttribute("aria-expanded", "true");
  await expect(collection.locator(".inline-detail")).toHaveCount(1);

  // The next preview click closes the same entry without changing list order.
  const id = await tab.getAttribute("data-term");
  await clickCard(term);
  await expect(collection.locator(`.disclose[data-term="${id}"]`)).toHaveAttribute("aria-expanded", "false");
  await expect(collection.locator(".inline-detail")).toHaveCount(0);
});

/** Older stored values must not bring back a two-step activation. */
test("the legacy preference off also uses one click to open and close", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=glossary&cards=hover");
  await page.getByRole("button", { name: "Glossary" }).click();
  const collection = page.getByRole("navigation", { name: "Glossary terms" });
  await expect(collection).toHaveAttribute("data-cards", "single");
  // Follow the key: opening promotes the term to the head of its heading.
  const id = await collection.locator(".term").nth(1).locator("> .disclose").getAttribute("data-term");
  const tab = collection.locator(`.disclose[data-term="${id}"]`);
  const item = collection.locator(`.term:has(> .disclose[data-term="${id}"])`);

  await clickCard(item);
  await expect(tab).toHaveAttribute("aria-expanded", "true");
  await expect(collection.locator(".inline-detail")).toHaveCount(1);
  await clickCard(item);
  await expect(tab).toHaveAttribute("aria-expanded", "false");
  await expect(collection.locator(".inline-detail")).toHaveCount(0);
});

/** The same burst that wedged a Reference card, over the Glossary's own click handler. */
test("clicking a term repeatedly does not wedge it", async ({ page }) => {
  const collection = await openGlossary(page);
  const term = collection.locator(".term").first();
  const tab = term.locator("> .disclose");
  await expect(tab).toHaveAttribute("aria-expanded", "false");

  // The summary line: plain text, no control under it, stable when the term opens below it.
  const spot = await term.locator("> .card .preview span").first().boundingBox();
  const x = spot!.x + spot!.width / 2;
  const y = spot!.y + spot!.height / 2;

  // Every press checked, not just the last: an even number of swallowed clicks would leave the
  // parity intact and hide the bug entirely.
  const seen: (string | null)[] = [];
  for (let n = 0; n < 8; n += 1) {
    await page.mouse.click(x, y);
    seen.push(await tab.getAttribute("aria-expanded"));
  }
  expect(seen).toEqual(["true", "false", "true", "false", "true", "false", "true", "false"]);
});

/** The regression itself, over the Glossary's own handler: a double-click must still open a term. */
test("double-clicking toggles twice and keeps later clicks responsive", async ({ page }) => {
  const collection = await openGlossary(page);
  const term = collection.locator(".term").first();
  const tab = term.locator("> .disclose");
  const spot = await term.locator("> .card .preview span").first().boundingBox();
  const x = spot!.x + spot!.width / 2;
  const y = spot!.y + spot!.height / 2;

  // Two presses: open, then closed again. Without the travel guard the second is swallowed by the
  // word the browser just selected, and the term stays open and then stops responding.
  await page.mouse.dblclick(x, y);
  await expect(tab).toHaveAttribute("aria-expanded", "false");
  await page.mouse.click(x + 1, y + 1);
  await expect(tab).toHaveAttribute("aria-expanded", "true");
});

/**
 * **Nothing a term does can move the window's frame.**
 *
 * Opening a term far down the list blanked the whole window in the desktop app on 2026-10-19: the
 * title bar, the rail and the page were all gone, leaving only the background. Each term's hidden
 * control (`.disclose`) is `position: absolute`, and its list item was not positioned, so its
 * containing block lay outside the scrolling list: the controls sat down a column as tall as the
 * whole list that did not scroll with it but stretched `.app`. Focusing one far down scrolled `.app`
 * — `overflow: hidden` is still scrollable from script — and everything slid off the top. 57 of the
 * 73 real terms did it, through a click or a plain focus.
 *
 * **The cause is checked directly**, because the fixture's list is too short to push the frame at
 * every window size: each control must be positioned against its own list item, and the frame
 * must have nothing to scroll. Then the reader's path — a click on the last term, and the plain
 * focus the arrow keys make — must leave the title bar at the top.
 */
test("focusing or opening a term at the bottom never scrolls the window's frame", async ({
  page,
}) => {
  const collection = await openGlossary(page);
  const anchored = await openers(collection).evaluateAll((controls) =>
    controls.every((control) => (control as HTMLElement).offsetParent === control.parentElement),
  );
  expect(anchored, "every control is positioned against its own list item").toBe(true);
  const frame = () =>
    page.evaluate(() => {
      const app = document.querySelector(".app")!;
      return {
        bar: document.querySelector(".titlebar")!.getBoundingClientRect().top,
        app: app.scrollTop,
        spare: app.scrollHeight - app.clientHeight,
      };
    });
  expect(await frame(), "the frame has nothing to scroll").toEqual({ bar: 0, app: 0, spare: 0 });

  // By id, not by position: opening a term lifts it to the top of its heading, after which
  // `.last()` would be some other term.
  const id = await openers(collection).last().getAttribute("data-term");
  const last = collection.locator(`.term > .disclose[data-term="${id}"]`);
  await toggle(last);
  await expect(last).toHaveAttribute("aria-expanded", "true");
  const previous = openers(collection).nth(-2);
  await previous.evaluate((control) => (control as HTMLElement).focus());
  const after = await frame();
  expect({ bar: after.bar, app: after.app }, "after opening and focusing").toEqual({ bar: 0, app: 0 });
});
