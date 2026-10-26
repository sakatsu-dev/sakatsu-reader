import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { assertContained } from "./contained";
import { openSettings } from "./menu";

/**
 * Reaching the parts of a grouped word.
 *
 * Grouping makes かっこいい one word, which is the easier lookup and the point of the layer. What
 * it took away was the ability to select かっこ at all: the passage only ever selects a top-level
 * token, so a constituent kept in `components` was preserved in the data and unreachable by the
 * reader. These are about the reader reaching it.
 *
 * The grouped token comes from a fixture. Production forms one only with the dictionaries
 * installed, and whether core forms the right one is `core/tests/lexical_assembly.rs`'s question —
 * this suite asks what the interface does when handed one.
 */
const passage = "気持ちが伝わってきます。";

type Harness = {
  releaseRequest: (key: string, how?: "complete" | "fail") => boolean;
  pendingRequests: () => string[];
};

const pending = (page: Page) =>
  page.evaluate(
    () => (window as Window & { __SAKATSU_QA__?: Harness }).__SAKATSU_QA__?.pendingRequests() ?? [],
  );

async function release(page: Page, key: string) {
  const letGo = await page.evaluate(
    (name) =>
      (window as Window & { __SAKATSU_QA__?: Harness }).__SAKATSU_QA__?.releaseRequest(
        name as string,
        "complete",
      ),
    key,
  );
  expect(letGo, `nothing was holding ${key}`).toBe(true);
}

async function analyze(page: Page, query: string) {
  await page.goto(`/?qa=analysis${query}`);
  await page.getByPlaceholder("Paste Japanese text").fill(passage);
  await page.getByRole("button", { name: /^Analyz/ }).click();
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
}

const parts = (page: Page) => page.getByTestId("word-part-button");

/** General settings, where the parts row is switched: it is set once, not from the app menu. */
async function openGeneral(page: Page) {
  await openSettings(page);
  await page.locator("nav.sections").getByRole("button", { name: "General", exact: true }).click();
}
const wholeWord = (page: Page) => page.getByTestId("whole-word-button");
const heading = (page: Page) => page.locator(".headword");
/** The offer's relationship line, which `expression.spec.ts` owns the wording of. */
const CONTAINING = "Expression containing this selection.";

/**
 * Show the grammar pane, where the expression offer lives.
 *
 * At the narrow measure the card is tabbed, and a selection change can leave the other tab in
 * front — so this is called before each assertion about the offer rather than once at the start.
 * At wider measures there are no tabs and this does nothing.
 */
async function showGrammar(page: Page) {
  if (await page.getByRole("tablist", { name: "Analysis details" }).count()) {
    await page.getByRole("tab", { name: "Grammar" }).click();
  }
}
const pressedStatus = (page: Page) =>
  page.getByRole("group", { name: "How well you know this word" }).locator("[aria-pressed=true]");

test("a word core rejoined says where it came from, and its parts do not", async ({ page }) => {
  await analyze(page, "&grouped=1&repaired=1");
  const note = page.getByTestId("repair-note");
  await expect(note).toContainText("rejoined using the dictionary");
  await assertContained(page);

  // A part was cut from the word, not rejoined: no attribution while it is inspected.
  await parts(page).first().click();
  await expect(note).toHaveCount(0);
  await wholeWord(page).first().click();
  await expect(note).toBeVisible();

  // And a passage with no repair record says nothing.
  await analyze(page, "&grouped=1");
  await expect(note).toHaveCount(0);
});

test("a grouped word offers its parts, and a plain word offers none", async ({ page }) => {
  await analyze(page, "&grouped=1");
  await expect(page.getByRole("navigation", { name: "Parts of this word" })).toBeVisible();
  await expect(parts(page)).toHaveCount(2);
  await assertContained(page);

  // The same passage without a grouped token: nothing to offer, and no empty row.
  await analyze(page, "");
  await expect(page.getByRole("navigation", { name: "Parts of this word" })).toHaveCount(0);
});

test("opening a part describes the part, and the whole word is one click back", async ({ page }) => {
  await analyze(page, "&grouped=1");

  // The whole word is what the passage selected — the default, unchanged by this slice.
  const whole = (await heading(page).textContent())?.trim() ?? "";
  expect(whole.length).toBeGreaterThan(0);

  // The parts spell the word between them. Asserted against the buttons rather than against the
  // heading, because the heading shows the *headword* a lookup returned and that is a different
  // claim — one this suite has no business making.
  const labels = await parts(page).allTextContents();
  expect(labels.join("")).toEqual("気持ち");

  const firstPart = labels[0].trim();
  await parts(page).first().click();

  // Now the panel is about the part, and says what it is part of.
  await expect(heading(page)).toHaveText(firstPart);
  await expect(page.getByText("Part of")).toBeVisible();
  await expect(wholeWord(page)).toHaveCount(1);
  await assertContained(page);

  // …and back.
  await wholeWord(page).click();
  await expect(heading(page)).toHaveText(whole);
  await expect(wholeWord(page)).toHaveCount(0);
});

test("the parts row is operable from the keyboard", async ({ page }) => {
  await analyze(page, "&grouped=1");
  const first = parts(page).first();
  const label = (await first.textContent())?.trim() ?? "";

  // Ordinary buttons: focusable, and Enter activates. No gesture, no pointer required.
  await first.focus();
  await expect(first).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(heading(page)).toHaveText(label);

  // Space returns to the whole word from the trail button.
  await wholeWord(page).focus();
  await page.keyboard.press("Space");
  await expect(wholeWord(page)).toHaveCount(0);
});

test("a part's status is its own, not the whole word's", async ({ page }) => {
  // The fixture reports `known` for this one part; every other word answers `new`.
  await analyze(page, "&grouped=1&partKnown=気");
  const whole = (await pressedStatus(page).textContent())?.trim() ?? "";

  await parts(page).first().click();
  // A different state than the whole word's — asserted as a difference rather than a literal,
  // because the wording belongs to the header component and this suite is about *which word* the
  // control describes.
  await expect(pressedStatus(page)).not.toHaveText(whole);

  await wholeWord(page).click();
  await expect(pressedStatus(page)).toHaveText(whole);
});

test("a part of a part is reachable, and the trail leads back through both", async ({ page }) => {
  await analyze(page, "&grouped=nested");
  const whole = (await heading(page).textContent())?.trim() ?? "";

  // Into the second part, which has parts of its own.
  await parts(page).nth(1).click();
  const middle = (await heading(page).textContent())?.trim() ?? "";
  await expect(parts(page)).toHaveCount(2);

  await parts(page).first().click();
  const inner = (await heading(page).textContent())?.trim() ?? "";
  expect(inner).not.toEqual(middle);
  expect(inner).not.toEqual(whole);

  // The trail names every ancestor, outermost first, and the first one goes all the way back.
  await expect(wholeWord(page)).toHaveCount(2);
  await expect(wholeWord(page).first()).toHaveText(whole);
  await wholeWord(page).first().click();
  await expect(heading(page)).toHaveText(whole);
  await assertContained(page);
});

test("a held answer for a part cannot overwrite the word selected after it", async ({ page }) => {
  // Every lookup is held, so "whole → part → another word" can be arranged with all three in
  // flight and released out of order.
  await analyze(page, "&grouped=1&hold-lookup=1");
  await expect.poll(() => pending(page)).not.toHaveLength(0);
  await release(page, "lookup:気持ち");

  const part = (await parts(page).first().textContent())?.trim() ?? "";
  await parts(page).first().click();
  await expect.poll(() => pending(page)).toContain(`lookup:${part}`);

  // Move on to a different word before the part's answer lands. The heading shows the word's
  // headword, which for 伝わってきます is 伝わる.
  await page.locator(".reader button", { hasText: "伝わってきます" }).first().click();
  await expect(heading(page)).toHaveText("伝わる");

  // The part's answer arrives late. It must not describe the word the reader is now on.
  await release(page, `lookup:${part}`);
  await expect(heading(page)).toHaveText("伝わる");
  await expect(page.getByText("Part of")).toHaveCount(0);
  await assertContained(page);
});

test("an expression offer for the whole word does not survive into a part", async ({ page }) => {
  // The whole word's offer is still in flight when the part is opened. Clearing the value alone
  // left it free to land afterwards and describe the part as though the construction were its.
  await analyze(page, "&grouped=1&expression=offer&hold-expression=1");
  await expect.poll(() => pending(page)).toContain("expression:気持ち");

  await parts(page).first().click();
  await showGrammar(page);
  await expect(page.getByText(CONTAINING)).toHaveCount(0);

  // The held answer arrives now, for a word the reader has left.
  await release(page, "expression:気持ち");
  await showGrammar(page);
  await expect(page.getByText(CONTAINING)).toHaveCount(0);
  await expect(page.getByText("Part of")).toBeVisible();
  await assertContained(page);
});

test("returning to the whole word asks for its expression offer again", async ({ page }) => {
  await analyze(page, "&grouped=1&expression=offer");
  await showGrammar(page);
  await expect(page.getByText(CONTAINING)).toBeVisible();

  await parts(page).first().click();
  await showGrammar(page);
  await expect(page.getByText(CONTAINING)).toHaveCount(0);

  // Back to the whole word. The offer is not restored from anything kept — it is asked for
  // again, because returning goes through the ordinary selection routine.
  await wholeWord(page).click();
  await showGrammar(page);
  await expect(page.getByText(CONTAINING)).toBeVisible();
  await assertContained(page);
});

test("marking a part Known leaves the whole word's furigana alone", async ({ page }) => {
  // **The gap this fills.** `analysis.spec.ts` already covers Known-word furigana in general —
  // marked Known, the ruby goes; marked Seen, it returns. What it cannot cover is a *grouped*
  // word and its parts, which are three separate words filed under three keys. The question here
  // is only whether the reader sees that separation.
  //
  // The fixture's grouped word is 気持ち, which has kanji and a reading of its own, so there is
  // ruby to lose. かっこいい would prove nothing: it is entirely kana and never had any.
  await analyze(page, "&grouped=1");
  const whole = page.locator("[data-reader-token]").first();
  const card = page.getByTestId("selected-word-card");
  await expect(whole.locator("rt")).toHaveCount(1);

  // Mark the *part* Known.
  await parts(page).first().click();
  await card.getByRole("button", { name: "Known" }).click();

  // The whole word is a different word: its ruby stays, and it is not shown as Known.
  await expect(whole.locator("rt")).toHaveCount(1);
  await expect(whole).not.toHaveClass(/status-known/);

  // Mark the whole word Known, and now it does lose its ruby — so the assertion above is about
  // independence rather than about ruby never going at all.
  await wholeWord(page).click();
  await card.getByRole("button", { name: "Known" }).click();
  await expect(whole).toHaveClass(/status-known/);
  await expect(whole.locator("rt")).toHaveCount(0);
  await assertContained(page);
});

test("the parts row is off unless it is asked for", async ({ page }) => {
  // **The shipped default.** Grouping exists so the whole word is the easy lookup; the parts row
  // is for the times that is not what you wanted, and under every selection it sits in front of
  // the definitions. So it is a setting, off until turned on — `?parts=off` is what the store
  // gives a reader who has never opened Settings.
  await analyze(page, "&grouped=1&parts=off");
  await expect(page.getByTestId("selected-word-card")).toBeVisible();
  await expect(parts(page)).toHaveCount(0);
  await expect(wholeWord(page)).toHaveCount(0);

  // Hidden, not broken: the passage still reads it as one word, and the panel still
  // describes it. Which headword the lookup returned is not this suite's claim to make.
  await expect(page.locator("[data-reader-token]").first()).toContainText("気持ち");
  expect(((await heading(page).textContent()) ?? '').trim().length).toBeGreaterThan(0);
  await assertContained(page);
});

test("General settings switches the row, and switching it off returns to the whole word", async ({
  page,
}) => {
  // **Hiding the row must not strand the selection behind it.** Every action — Known, Anki, the
  // audio button — is about `activeToken`, so a part left open with its row gone would leave all
  // of them pointed at a word nothing on screen names.
  await analyze(page, "&grouped=1");
  const whole = (await heading(page).textContent())?.trim() ?? "";
  await expect(parts(page)).toHaveCount(2);

  await parts(page).first().click();
  await expect(wholeWord(page)).toHaveCount(1);

  await openGeneral(page);
  await page.getByRole("checkbox", { name: "Show a word's parts" }).uncheck();
  await page.keyboard.press("Escape");

  await expect(parts(page)).toHaveCount(0);
  await expect(wholeWord(page)).toHaveCount(0);
  await expect(heading(page)).toHaveText(whole);
  await assertContained(page);

  // And on again, from the same place.
  await openGeneral(page);
  await page.getByRole("checkbox", { name: "Show a word's parts" }).check();
  await page.keyboard.press("Escape");
  await expect(parts(page)).toHaveCount(2);
});

type ContextCall = { surfaces: string[]; index: number };
const contextCalls = (page: Page) =>
  page.evaluate(
    () =>
      (window as Window & { __SAKATSU_QA__?: { contextCalls: ContextCall[] } }).__SAKATSU_QA__
        ?.contextCalls ?? [],
  );

test("a grammatical part the text does not spell is not offered, and its text stays in the context", async ({
  page,
}) => {
  // **The notebook defect, in the reader.** Ichiran names a contracted auxiliary's parts in their
  // expanded form — いる for the る of 〜てる — and the reader offered that as a piece of the
  // passage. The fixture's word is 気持ち with parts 気 (in the text) and いる (grammar only, no
  // span): the second half of the word, 持ち, is text no selectable part covers.
  await analyze(page, "&grouped=expanded");
  await expect(parts(page)).toHaveCount(1);
  await expect(parts(page).first()).toHaveText("気");
  await expect(page.getByRole("navigation", { name: "Parts of this word" })).not.toContainText("いる");

  await parts(page).first().click();
  await expect(heading(page)).toHaveText("気");

  // The context the part was looked up against keeps every character of the word — 持ち as a
  // filler cut from the source — and holds nothing the text does not spell.
  await expect.poll(async () => (await contextCalls(page)).length).toBeGreaterThan(0);
  const asked = (await contextCalls(page)).at(-1)!;
  expect(asked.surfaces.slice(0, 2)).toEqual(["気", "持ち"]);
  expect(asked.surfaces).not.toContain("いる");
  expect(asked.surfaces[asked.index]).toBe("気");
  expect(asked.surfaces.join("")).toBe(passage);
  await assertContained(page);
});
