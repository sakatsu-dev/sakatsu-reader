import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { gear, openSettings } from "./menu";

/**
 * **The reading page fills the window, at any size.**
 *
 * It had a Column width setting, 50–200rem. 200rem was 3,000px at the default interface size:
 * wider than most windows and narrower than a 4K one. Readers kept it at the top, so the setting
 * was removed (2026-10-23) and the page has no limit of its own.
 */
test("the reading page fills even a 4K window", async ({ page }, info) => {
  test.skip(info.project.name !== "edge-900-100", "window sizes are set here, not by the project");
  await page.setViewportSize({ width: 3840, height: 2160 });
  await page.goto("/?qa=analysis");
  await expect(page.getByPlaceholder("Paste Japanese text")).toBeVisible();
  // Edge to edge: from just past the rail on the left to the window's own right edge. (Its parent
  // also holds the rail, so comparing widths with it would count the rail as missing space.)
  const edges = await page.evaluate(() => {
    const main = document.querySelector("main")!.getBoundingClientRect();
    return {
      left: Math.round(main.left),
      right: Math.round(main.right),
      window: window.innerWidth,
    };
  });
  expect(edges.right, "main reaches the window's right edge").toBe(edges.window);
  expect(edges.left, "and starts just past the rail").toBeLessThanOrEqual(60);
});

test("Settings no longer offers a column width", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  await page.locator("nav.sections").getByRole("button", { name: "General", exact: true }).click();
  await expect(page.getByRole("slider", { name: /^Interface/ })).toBeVisible();
  await expect(page.getByRole("slider", { name: /column width/i })).toHaveCount(0);
});

/**
 * **The text pages keep their own measure.** Glossary and Settings are lists of short lines;
 * spread across a full-width page, their cards ran 1,814px and 1,624px wide at 1920px, with each
 * label a screen away from its control. Each stops at 64rem, centred in the page.
 */
for (const view of ["Glossary", "Settings"] as const) {
  test(`${view} keeps a 64rem measure, centred, in a wide window`, async ({ page }, info) => {
    test.skip(
      info.project.name !== "edge-900-100",
      "window sizes are set here, not by the project",
    );
    await page.setViewportSize({ width: 1920, height: 1080 });
    if (view === "Glossary") {
      await page.goto("/?qa=reference&fixture=glossary");
      await page.getByRole("button", { name: "Glossary" }).click();
      await expect(page.getByRole("navigation", { name: "Glossary terms" })).toBeVisible();
    } else {
      await page.goto("/?qa=analysis");
      await openSettings(page);
    }
    const geometry = await page.evaluate(
      (selector) => {
        const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
        const main = document.querySelector("main")!.getBoundingClientRect();
        const box = document.querySelector(selector)!.getBoundingClientRect();
        const padding = parseFloat(getComputedStyle(document.querySelector("main")!).paddingLeft);
        return {
          rem: box.width / rem,
          left: box.left - (main.left + padding),
          right: main.right - padding - box.right,
        };
      },
      view === "Glossary" ? ".page.glossary" : ".page.settings",
    );
    expect(geometry.rem, "no wider than its measure").toBeLessThanOrEqual(64.05);
    expect(geometry.rem, "and actually at it, with this much room").toBeGreaterThanOrEqual(63.95);
    expect(Math.abs(geometry.left - geometry.right), "centred").toBeLessThanOrEqual(2);
  });
}

const rootSize = (page: Page) =>
  page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize));

/** Both places the Interface size is set: the settings page and the app menu. */
const SURFACES = ["Settings", "the app menu"] as const;

async function interfaceSlider(page: Page, surface: (typeof SURFACES)[number]) {
  await page.goto("/?qa=analysis");
  if (surface === "Settings") {
    await openSettings(page);
    await page
      .locator("nav.sections")
      .getByRole("button", { name: "General", exact: true })
      .click();
  } else {
    await gear(page).click();
  }
  const slider = page.getByRole("slider", { name: /^Interface/ });
  await slider.scrollIntoViewIfNeeded();
  return slider;
}

for (const surface of SURFACES) {
  /**
   * **Dragging the Interface slider is not a tug-of-war.** The slider is sized in `rem`, so when
   * every step rescaled the interface, the track moved under the pointer mid-drag and the value
   * chased it. The readout follows the drag; the interface follows the release.
   */
  test(`in ${surface}, the Interface slider stays put while it is dragged, and applies on release`, async ({
    page,
  }) => {
    const slider = await interfaceSlider(page, surface);
    const before = await rootSize(page);
    const track = (await slider.boundingBox())!;
    const y = track.y + track.height / 2;

    await page.mouse.move(track.x + track.width * 0.05, y);
    await page.mouse.down();
    await page.mouse.move(track.x + track.width * 0.95, y, { steps: 8 });

    expect(await rootSize(page), "nothing rescales mid-drag").toBe(before);
    const during = (await slider.boundingBox())!;
    expect(Math.abs(during.x - track.x), "the track has not moved").toBeLessThanOrEqual(0.5);
    expect(Math.abs(during.width - track.width), "or changed size").toBeLessThanOrEqual(0.5);
    const dragged = Number(await slider.inputValue());
    expect(dragged, "the drag reached the far end").toBeGreaterThanOrEqual(23);
    await expect(slider, "the readout follows the drag").toHaveAccessibleName(
      new RegExp(`${dragged}px`),
    );

    await page.mouse.up();
    await expect.poll(() => rootSize(page), { message: "applied on release" }).toBe(dragged);
  });

  test(`in ${surface}, the Interface slider still applies each arrow key at once`, async ({
    page,
  }) => {
    const slider = await interfaceSlider(page, surface);
    const before = await rootSize(page);
    await slider.focus();
    await page.keyboard.press("ArrowLeft");
    await expect.poll(() => rootSize(page)).toBe(before - 1);
    await expect(slider).toHaveAccessibleName(new RegExp(`${before - 1}px`));
  });
}
