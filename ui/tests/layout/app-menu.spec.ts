import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { gear, menu, menuRows, openSettings } from "./menu";

/**
 * **The app's menu: how things look, then what the app can do.**
 *
 * Settings is a page, so it replaces the reader — every appearance change was judged blind, then
 * confirmed by closing the page, looking, and going back in. These tests are about the two halves
 * of the fix: the menu opens *over* what you are reading, and what it changes is the real thing
 * behind it rather than a preview of it.
 *
 * It was briefly a second button beside the gear. One gear, one menu — because "change how this
 * behaves" and "change how this looks" are both the app's own menu, and two buttons for that is a
 * distinction nobody asked for.
 */

/// A passage on screen, which is what the menu exists to be judged against. The fixture starts
/// empty, so it has to be analysed first — the same step `analysis.spec.ts` takes.
async function openReader(page: Page) {
  await page.goto("/?qa=analysis");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".reader")).toBeVisible();
}

test("the title bar carries one app button, not two", async ({ page }) => {
  await openReader(page);
  // The merge, asserted directly: pin, gear, and the OS's three. An "Aa" beside the gear was the
  // arrangement this replaced.
  await expect(page.locator(".cap")).toHaveCount(5);
  await expect(page.getByRole("button", { name: "Quick appearance" })).toHaveCount(0);
  await expect(gear(page)).toBeVisible();
});

test("the menu opens over the reader, with the passage still behind it", async ({ page }) => {
  await openReader(page);
  await expect(menu(page)).toHaveCount(0);

  await gear(page).click();
  await expect(menu(page)).toBeVisible();
  // The whole premise: the text you are judging against is still on screen.
  await expect(page.locator(".reader")).toBeVisible();

  const overlap = await page.evaluate(() => {
    const box = document.querySelector(".menu")!.getBoundingClientRect();
    const reader = document.querySelector(".reader")!.getBoundingClientRect();
    return box.left < reader.right && box.right > reader.left && box.bottom > reader.top;
  });
  expect(overlap, "the menu sits over the reading surface").toBe(true);
});

/**
 * **The property the whole feature exists for.** A slider here moves the passage, not a mock of it.
 */
test("dragging a size in the menu changes the real passage", async ({ page }) => {
  await openReader(page);
  await gear(page).click();

  const line = page.locator(".reader");
  const before = await line.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));

  await page.getByRole("slider", { name: /Analyzed text/ }).fill("2");
  await expect
    .poll(() => line.evaluate((el) => parseFloat(getComputedStyle(el).fontSize)))
    .toBeGreaterThan(before);
});

/**
 * **A control for something you cannot see is the problem this feature removes**, so the menu must
 * not reintroduce one. Passage controls belong to the reader, including the capture-copy setting
 * added after these tests were first written. The guess mark and the parts row are set once rather
 * than adjusted while reading, so they live in General settings and on no page's menu.
 * Furigana is absent on purpose: the analysis page carries its own toggle beside the text.
 */
test("the settings rows follow the page behind them, the actions do not", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  await expect(menuRows(page)).toHaveCount(10);
  await expect(page.getByRole("checkbox", { name: "Mark uncertain readings" })).toHaveCount(0);
  await expect(page.getByRole("checkbox", { name: "Show a word's parts" })).toHaveCount(0);
  await expect(page.getByRole("checkbox", { name: "Copy captures to clipboard" })).toBeVisible();
  await expect(page.getByRole("slider", { name: /Analyzed text/ })).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Align text" })).toBeVisible();
  await page.keyboard.press("Escape");

  // Reference shows definitions but no passage, so the reader's four go and `--panel-scale` stays.
  await page.getByRole("button", { name: "Reference" }).click();
  await gear(page).click();
  await expect(menuRows(page)).toHaveCount(4);
  await expect(page.getByRole("slider", { name: /Definitions/ })).toBeVisible();
  // The watermarks are on every page, so their switches are too.
  await expect(page.getByRole("checkbox", { name: "Clouds", exact: true })).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Sakura", exact: true })).toBeVisible();
  await expect(page.getByRole("slider", { name: /Analyzed text/ })).toHaveCount(0);
  await expect(page.getByRole("checkbox", { name: "Copy captures to clipboard" })).toHaveCount(0);

  // The three actions are about the app, not about what is visible, so they are always offered.
  for (const action of ["All settings…", "Help", "Restart"]) {
    await expect(page.getByRole("button", { name: action })).toBeVisible();
  }
});

/**
 * The seven section shortcuts went with the merge — keeping them would have made an eighteen-row
 * menu whose whole point is being quicker than the settings page.
 */
test("the menu no longer offers the settings sections", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  for (const section of ["Capture", "Recognition", "Anki", "Diagnostics"]) {
    await expect(menu(page).getByRole("button", { name: section, exact: true })).toHaveCount(0);
  }
  // And the way to them is still one press away.
  await expect(page.getByRole("button", { name: "All settings…" })).toBeVisible();
});

test("Help is reachable from the menu on any page", async ({ page }) => {
  await openReader(page);
  await page.getByRole("button", { name: "Reference" }).click();
  await gear(page).click();
  await page.getByRole("button", { name: "Help" }).click();

  await expect(page.getByRole("heading", { name: "Help", exact: true })).toBeVisible();
  // Closed on the way, or it would outlive the page it was opened over.
  await expect(menu(page)).toHaveCount(0);
});

test("Escape closes the menu and hands focus back to the gear", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  await expect(menu(page)).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(menu(page)).toHaveCount(0);
  // Or focus is left on an element that no longer exists and the next Tab starts from the top.
  await expect(gear(page)).toBeFocused();
});

/**
 * **One value behind both surfaces.** The menu binds what the settings page binds, so a change in
 * one is the same number in the other — there is nothing to keep in step because nothing is
 * duplicated.
 */
test("the menu and the settings page agree about a value", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  await page.getByRole("slider", { name: /^Interface/ }).fill("21");
  await page.keyboard.press("Escape");

  await openSettings(page);
  await expect(page.getByRole("slider", { name: /^Interface/ })).toHaveValue("21");
});

/**
 * The clouds' strength on the settings page. Found inside the item its "Clouds" switch heads,
 * since the slider itself is called Strength now that each motif has a switch above it.
 */
const cloudStrength = (page: Page) =>
  page
    .locator(".item", { has: page.getByRole("checkbox", { name: "Clouds", exact: true }) })
    .getByRole("slider", { name: /^Strength/ });

/**
 * **One list, one answer — and this had already gone wrong.**
 *
 * The picker was written twice. The settings page's copy applied a palette's colours *and* its
 * decoration; the menu's applied only the colours. Picking the same saved palette therefore gave
 * two different results depending on where you picked it from, and renaming the option groups
 * meant editing both files.
 *
 * Asserted through the decoration, because that is the half the menu used to drop.
 */
test("a palette applies the same from the menu as from the settings page", async ({ page }) => {
  await openReader(page);

  // A saved palette carrying decoration, made through the settings page so it is a real one.
  // The decoration lives under Appearance, which the page does not necessarily open on.
  await openSettings(page);
  await page.getByRole("button", { name: "Appearance", exact: true }).click();
  await cloudStrength(page).fill("0.14");
  await page.getByRole("button", { name: /^Save as/ }).click();
  await page.getByRole("textbox", { name: "Call it" }).fill("Cloudy");
  await page.getByRole("button", { name: /^Save$/ }).click();

  // Move the decoration away from what that palette holds.
  await cloudStrength(page).fill("0.02");
  const moved = await cloudStrength(page).inputValue();
  expect(moved).toBe("0.02");

  // Now pick it from the *menu*, which is the surface that used to drop the decoration.
  await page.keyboard.press("Escape");
  await gear(page).click();
  await page.getByRole("combobox").first().selectOption("Cloudy");
  await page.keyboard.press("Escape");

  await openSettings(page);
  await page.getByRole("button", { name: "Appearance", exact: true }).click();
  await expect(
    cloudStrength(page),
    "the menu applied the palette's decoration, not only its colours",
  ).toHaveValue("0.14");
});

/**
 * **The picker is drawn by us, so it has to be drawn in our type.**
 *
 * A `<select>`'s list is normally the OS's, painted outside the page; `appearance: base-select`
 * brings it inside so it can be themed. What comes with that is the top layer, which inherits
 * nothing from the page — and an `<option>` that says `font: inherit` inherits from its
 * `<optgroup>`, whose heading treatment is deliberately different. That caught this twice: first
 * every palette name came out uppercase and letterspaced, then set in a serif.
 *
 * So both faces are asserted against the body's rather than described. The group label is a
 * heading, but it is chrome inside a control rather than a heading over page content — it stays
 * one by being smaller, uppercase, tracked and faint, not by changing typeface.
 */
test("the palette list is set in the app's own type", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  const select = page.locator(".menu select").first();
  await select.click();

  const type = await page.evaluate(() => {
    const face = (node: Element | null) => (node ? getComputedStyle(node).fontFamily : "");
    return {
      body: face(document.body),
      option: face(document.querySelector(".menu select option")),
      group: face(document.querySelector(".menu select optgroup")),
    };
  });

  expect(type.option, "an option is set in the face the app reads in").toBe(type.body);
  expect(type.group, "and so is the group label — a menu is chrome, not editorial").toBe(type.body);
});

/**
 * **Nothing else in the app may disturb an open menu.**
 *
 * The capture hint alternates two faces on a four-second timer, which is a state change in
 * `App.svelte` — the same component that owns the menu's bindings and hands it down as a snippet.
 * If that re-created the snippet, the menu would be rebuilt and would lose the one thing it owns:
 * whether it is open. Anything else that ticks in the background would do the same.
 *
 * **This is the one test in the suite that needs motion on.** The hint only alternates when
 * animations are enabled, and the harness reports `prefers-reduced-motion: reduce`, so the cycling
 * path is otherwise never exercised at all.
 */
test("a background tick elsewhere leaves the open menu alone", async ({ page }) => {
  await page.goto("/?qa=analysis&motion=on");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".reader")).toBeVisible();

  // Two faces means the timer is running; with one there is nothing to alternate and this would
  // pass without testing anything.
  await expect(page.locator(".shortcut .face")).toHaveCount(2);

  await gear(page).click();
  await expect(menu(page)).toBeVisible();
  const shape = () =>
    menu(page).evaluate((el) => {
      const box = el.getBoundingClientRect();
      return `${Math.round(box.x)},${Math.round(box.y)} ${Math.round(box.width)}x${Math.round(box.height)}`;
    });
  const before = await shape();
  const face = () => page.locator(".shortcut .face:not(.off)").first().textContent();
  const firstFace = await face();

  // Past one full turn of the timer, so the thing under test has actually happened.
  await page.waitForTimeout(4600);
  await expect.poll(face).not.toBe(firstFace);

  await expect(menu(page), "the menu is still open").toBeVisible();
  expect(await shape(), "and has not moved or resized").toBe(before);
  await expect(menuRows(page), "and still holds its ten current rows").toHaveCount(10);
});

/**
 * **Picking a palette must not resize the thing you picked it from.**
 *
 * `appearance: base-select` — which is what lets the picker be themed at all — sizes a `<select>`
 * to its *chosen* option, where the native control sized to its longest one. Measured here: 51px
 * showing `Kohi`, 73px showing `Unsaved`. The menu was `width: max-content`, so that fed straight
 * into it and the whole menu changed width with the palette, which showed as a resize on every
 * pick and a flicker on any reflow while it was open.
 *
 * Both widths are stated now, and both are asserted: pinning only the menu leaves the control
 * itself still jumping inside it.
 */
test("the menu and its palette control keep one width whatever is chosen", async ({ page }) => {
  await openReader(page);
  await gear(page).click();

  const widths = async () =>
    page.evaluate(() => {
      const box = (sel: string) => Math.round(document.querySelector(sel)?.getBoundingClientRect().width ?? 0);
      return { menu: box(".menu"), select: box(".menu select") };
    });

  const first = await widths();
  expect(first.menu, "the menu has a width").toBeGreaterThan(0);
  expect(first.select, "and so does the control").toBeGreaterThan(0);

  // Names of visibly different lengths, which is what made the old behaviour show.
  for (const palette of ["Washi", "Matcha", "Kohi", "Sumi"]) {
    await page.locator(".menu select").first().selectOption(palette);
    expect(await widths(), `${palette} left both widths alone`).toEqual(first);
  }

  // **A name longer than the menu's own floor**, which is the case that tells the two fixes apart:
  // with the control pinned, the shipped names are all shorter than `17rem` and a `max-content`
  // menu would look fine anyway. Saved through the settings page so it is a real palette.
  await page.keyboard.press("Escape");
  await openSettings(page);
  await page.getByRole("button", { name: "Appearance", exact: true }).click();
  await page.getByRole("button", { name: /^Save as/ }).click();
  await page.getByRole("textbox", { name: "Call it" }).fill("A very long palette name indeed");
  await page.getByRole("button", { name: /^Save$/ }).click();
  await page.keyboard.press("Escape");

  await gear(page).click();
  await page.locator(".menu select").first().selectOption("A very long palette name indeed");
  expect(await widths(), "a long name does not stretch the menu either").toEqual(first);
});

/**
 * **A slider resolves only as finely as it is wide.**
 *
 * Every dial in the menu declares the same step — 1% — but beside its label each had 129px to work
 * in, and `Analyzed text` spans 0.6–2.5: 190 steps in those pixels, so dragging it advanced 1% or
 * 2% depending on where the pointer landed while `Definitions`, at 140 steps, mostly advanced 1%.
 * Identical steps, different resolution, and it reads as one dial behaving unlike the others.
 *
 * Asserted as steps-per-pixel rather than as a width, because the width is the fix and this is the
 * property the fix exists for — a narrower menu or a wider range would break it again.
 */
test("no dial in the menu advances more than one step per pixel", async ({ page }) => {
  await openReader(page);
  await gear(page).click();

  const dials = await page.locator(".menu input[type=range]").evaluateAll((els) =>
    els.map((el) => {
      const range = el as HTMLInputElement;
      const steps = (Number(range.max) - Number(range.min)) / Number(range.step);
      return {
        label: range.closest("label")?.textContent?.trim().split(/\s+/)[0] ?? "?",
        perPixel: steps / range.getBoundingClientRect().width,
      };
    }));

  expect(dials.length, "the reader's menu carries its size dials").toBeGreaterThan(2);
  for (const dial of dials) {
    expect(dial.perPixel, `${dial.label} advances at most one step per pixel`).toBeLessThanOrEqual(1);
  }
});

/**
 * **The one row here whose effect is not on the screen behind it.**
 *
 * Every other row in this menu changes something you can see while you change it — that is the
 * premise the whole menu was built on. The engine decides what the *next* capture reads with, so it
 * is judged against the passage the last one produced rather than against a live change. It belongs
 * with the reader anyway: a capture lands there, and the hint beside the passage already names the
 * engine that read it, so the setting and its readout sit together.
 */
test("the pipeline is offered where captures land, and not on the pages that never see one", async ({
  page,
}) => {
  await openReader(page);
  await gear(page).click();

  // The group, in its order: what reads a capture, what breaks it into words, what happens to its
  // lines. Below the sizes, which is where it was asked for and where the menu's own order puts it.
  const labels = await menuRows(page).locator("span").allInnerTexts();
  const pipelineLabels = [
    "Recognition",
    "Analyzer",
    "Keep line breaks",
    "Copy captures to clipboard",
  ];
  expect(labels.slice(-pipelineLabels.length)).toEqual(pipelineLabels);
  expect(labels.indexOf("Recognition")).toBeGreaterThan(labels.indexOf("Definitions"));

  await page.getByRole("button", { name: "Reference", exact: true }).click();
  await gear(page).click();
  for (const row of pipelineLabels) {
    await expect(menu(page).locator(".row", { hasText: row })).toHaveCount(0);
  }
});

/**
 * **A rule needs something on both sides of it.** The pipeline is ruled off from the sizes above
 * because it answers a different question — how the passage was made, not how it is drawn. On the
 * pages that never see a capture the group is absent entirely, and the rule has to go with it or it
 * rules off nothing and reads as a stray line above the actions.
 */
test("the rule above the pipeline exists only where the pipeline does", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  const ruleBefore = await menu(page).evaluate((el) => {
    const kids = [...el.children];
    const first = kids.findIndex((kid) => kid.textContent?.startsWith("Recognition"));
    return kids[first - 1].tagName;
  });
  expect(ruleBefore, "the pipeline opens with a rule").toBe("HR");

  // Sizes, then one rule, then the three actions each ruled from the next.
  await expect(menu(page).locator("hr")).toHaveCount(4);
  await page.getByRole("button", { name: "Reference", exact: true }).click();
  await gear(page).click();
  await expect(menu(page).locator("hr"), "no group, no rule for it").toHaveCount(3);
});

/**
 * **The list is one list.** `OcrSelect` on the settings page and the `<select>` here both read
 * `OCR_OPTIONS`, and this is what stops a fourth engine — or a reworded third — from arriving in
 * only one of them. The palette control needed a whole shared component to learn this lesson; the
 * engine list gets to skip that by never being copied in the first place.
 */
test("the menu and the settings page offer the same engines, in the same words", async ({
  page,
}) => {
  await openReader(page);
  await gear(page).click();
  const fromMenu = await menu(page).getByRole("combobox", { name: "Recognition" }).locator("option").allInnerTexts();

  await page.keyboard.press("Escape");
  await openSettings(page);
  await page.getByRole("button", { name: "Recognition", exact: true }).click();
  await page.getByRole("button", { name: "Recognition engine" }).click();
  const fromPage = await page.locator(".ocr-model-option").allInnerTexts();

  expect(fromMenu.map((text) => text.trim())).toEqual(fromPage.map((text) => text.trim()));
  expect(fromMenu.length, "all four engines, not a truncated list").toBe(4);
});

/// Picking in one surface is picking the setting, not a copy of it — the same property the palette
/// and the sliders are held to.
test("an engine picked in the menu is the engine the settings page shows", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  await menu(page).getByRole("combobox", { name: "Recognition" }).selectOption("manga");

  await page.keyboard.press("Escape");
  await openSettings(page);
  await page.getByRole("button", { name: "Recognition", exact: true }).click();
  await expect(page.getByRole("button", { name: "Recognition engine" })).toContainText("manga-ocr");
});

/// The analyzer list is shared the same way the engine list is, and for the same reason.
test("the menu and the settings page offer the same analyzers", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  const fromMenu = await menu(page).getByRole("combobox", { name: "Analyzer" }).locator("option").allInnerTexts();

  await page.keyboard.press("Escape");
  await openSettings(page);
  const fromPage = await page
    .locator(".row", { hasText: "Analyzer" })
    .locator("option")
    .allInnerTexts();

  expect(fromMenu.map((text) => text.trim())).toEqual(fromPage.map((text) => text.trim()));
  expect(fromMenu.length, "all three analyzers").toBe(3);
});

/// Picking in one surface is picking the setting, not a copy of it.
test("an analyzer picked in the menu is the analyzer the settings page shows", async ({ page }) => {
  await openReader(page);
  await gear(page).click();
  await menu(page).getByRole("combobox", { name: "Analyzer" }).selectOption("ichiran");

  await page.keyboard.press("Escape");
  await openSettings(page);
  await expect(page.locator(".row", { hasText: "Analyzer" }).locator("select")).toHaveValue(
    "ichiran",
  );
});

/**
 * **A palette you made has nothing to explain, so nothing is said.**
 *
 * The note under the picker carries a shipped palette's reason for existing — which light, which
 * mood. Saved palettes had no such line and were given the word "Yours.", which under a heading
 * already reading *Palette*, in a list already grouping them under *Custom*, said nothing three
 * times. The note is absent for those and present for the other two cases, which is what this
 * pins: a blurb that is missing must not fall through to the *unsaved* sentence either.
 */
test("the palette note appears only when there is something to say", async ({ page }) => {
  await openReader(page);
  await openSettings(page);
  await page.getByRole("button", { name: "Appearance", exact: true }).click();

  const note = page.locator(".row", { hasText: "Palette" }).locator("+ .note");
  await expect(note, "a shipped palette explains itself").toBeVisible();
  const shipped = await note.innerText();
  expect(shipped.trim().length).toBeGreaterThan(20);

  // Move off every saved palette: the dials now sit on nothing, which does need saying.
  await cloudStrength(page).fill("0.13");
  await expect(note).toBeVisible();
  await expect(note).toContainText("Save it to keep it");

  await page.getByRole("button", { name: /^Save as/ }).click();
  await page.getByRole("textbox", { name: "Call it" }).fill("Mine");
  await page.getByRole("button", { name: /^Save$/ }).click();

  // Saved, and now the selected palette is one of the learner's.
  await expect(note, "a palette of your own says nothing at all").toBeHidden();
});
