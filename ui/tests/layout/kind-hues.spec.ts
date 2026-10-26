import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { openSettings } from "./menu";

/**
 * **The dial that turns the part-of-speech colours, and the collision it can cause.**
 *
 * Six numbers move at once, so a degree readout says nothing a reader can act on — and the failure
 * it causes is invisible until you meet it on a card: a kind landing on the accent means a sense
 * label and a selected control wear one colour between them. The shipped palettes are measured
 * away from that; a hand-turned dial has to be told.
 */
async function openAppearance(page: Page, query = "") {
  await page.goto(`/?qa=reference&fixture=parts${query}`);
  await openSettings(page);
  await page.getByRole("button", { name: "Appearance", exact: true }).click();
  const dial = page.getByRole("slider", { name: /Kind hues/ });
  await expect(dial).toBeVisible();
  return dial;
}

const swatches = (page: Page) => page.locator(".kind-strip .swatch:not(.accent)");

test("the strip names all six kinds, each in its own colour", async ({ page }) => {
  await openAppearance(page);
  await expect(swatches(page)).toHaveCount(6);
  // **The names, not anonymous squares.** A colour on its own says which colours exist and
  // nothing about what they mean; the word is what makes the preview legible without hovering.
  expect(await swatches(page).allTextContents()).toEqual(
    ["Expression", "Noun", "Adjective", "Adverb", "Verb", "Function"],
  );
  const colours = await swatches(page).evaluateAll((els) =>
    els.map((el) => getComputedStyle(el).color));
  expect(new Set(colours).size, "six distinct colours").toBe(6);
});

test("turning the dial moves every swatch", async ({ page }) => {
  const dial = await openAppearance(page);
  const before = await swatches(page).evaluateAll((els) =>
    els.map((el) => getComputedStyle(el).color));

  await dial.fill("140");
  await expect.poll(async () =>
    (await swatches(page).evaluateAll((els) => els.map((el) => getComputedStyle(el).color)))[0],
  ).not.toBe(before[0]);

  const after = await swatches(page).evaluateAll((els) =>
    els.map((el) => getComputedStyle(el).color));
  for (const [at, colour] of after.entries()) {
    expect(colour, `swatch ${at} moved`).not.toBe(before[at]);
  }
  expect(new Set(after).size, "and they are still six distinct colours").toBe(6);
});

/**
 * **The way back to the default, offered only when there is one.**
 *
 * The dial used to carry a warning that named whichever kind had drifted onto the accent and
 * offered to turn the wheel clear of it. That went: it was a paragraph whose length followed the
 * kind's *name*, so dragging the accent flipped it between two lines and three and shifted every
 * row below it while the dial was still moving. What is left is the plainer thing a dial wants —
 * a reset — which is also the rule every colour well on this page already follows.
 */
test("the kind dial offers a way back to its default, and only when moved", async ({ page }) => {
  const dial = await openAppearance(page);
  const reset = page.getByRole("button", { name: "Set to default" });

  await dial.fill("0");
  await expect(reset, "nothing to undo at the default").toHaveCount(0);

  await dial.fill("140");
  await expect(reset).toBeVisible();
  await reset.click();

  await expect(dial).toHaveValue("0");
  await expect(reset, "and it goes again once there is nothing to undo").toHaveCount(0);
});

/**
 * **Dragging a dial must not move the page under it.**
 *
 * The crowding warning names the kind that collided and how far away it is, and the sentence's
 * length followed the name — "Expression" wrapped to three lines where "Noun" took two. So turning
 * the accent flipped the block between 37px and 54px and shifted every row below it *while the
 * dial was still being dragged*, which reads as the page coming apart under your hand.
 *
 * Measured on the scroll height of the whole pane rather than on the warning, because what matters
 * is that nothing else moved.
 */
test("turning the accent does not reflow the settings page", async ({ page }) => {
  await openAppearance(page);
  const height = () =>
    page.evaluate(() => document.querySelector(".pane")?.scrollHeight ?? 0);

  const accent = page.getByRole("slider", { name: /^Accent/ });
  await accent.fill("0");
  const settled = await height();
  expect(settled, "the pane has a height").toBeGreaterThan(0);

  // Hues that pick out kinds with names of very different lengths — Expression, Noun, Adverb.
  for (const hue of ["120", "200", "260", "330", "359"]) {
    await accent.fill(hue);
    expect(await height(), `accent ${hue}° left the page the same height`).toBe(settled);
  }
});
