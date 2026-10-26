import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { gear, menu, openSettings } from "./menu";
import { assertContained } from "./contained";

/**
 * **The watermarks switch off and on without losing how they were tuned.**
 *
 * The strength used to be the only control, so "off" meant dragging it to zero and "on" meant
 * finding the old value again by eye. A switch now sits above each motif: off paints nothing, and
 * on paints exactly what was there. The quick menu carries the same two switches so this is one
 * press from any page.
 *
 * What is checked is what the motifs read — the `--decor-cloud` and `--decor-sakura` tokens on the
 * root — not merely the checkbox, because a switch that ticked without changing the token would
 * look fine in the menu and do nothing on screen.
 */

const token = (page: Page, name: string) =>
  page.evaluate((property) => document.documentElement.style.getPropertyValue(property), name);

/** The Settings item a motif's switch heads. */
const motif = (page: Page, name: string) =>
  page.locator(".item", { has: page.getByRole("checkbox", { name, exact: true }) });

async function appearance(page: Page) {
  await openSettings(page);
  await page
    .locator("nav.sections")
    .getByRole("button", { name: "Appearance", exact: true })
    .click();
}

test("switching the clouds off paints nothing and keeps the strength for later", async ({
  page,
}) => {
  await page.goto("/?qa=analysis");
  await appearance(page);
  const clouds = motif(page, "Clouds");
  const strength = clouds.getByRole("slider", { name: /^Strength/ });

  await strength.fill("0.1");
  await expect.poll(() => token(page, "--decor-cloud")).toBe("0.1");

  await clouds.getByRole("checkbox", { name: "Clouds", exact: true }).uncheck();
  await expect.poll(() => token(page, "--decor-cloud")).toBe("0");
  // Off removes the clouds' tuning from the card rather than greying it out — and only theirs.
  await expect(clouds.getByRole("slider"), "the clouds' rows are gone while off").toHaveCount(0);
  await expect(clouds.getByRole("combobox")).toHaveCount(0);
  await expect(
    motif(page, "Sakura in the rail").getByRole("slider", { name: /^Strength/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("slider", { name: /^Pattern size/ }),
    "shared rows stay while one is on",
  ).toBeVisible();
  // Nothing drawn needs no softening, so a switched-off motif costs no filter either.
  await expect.poll(() => token(page, "--decor-cloud-blur")).toBe("none");

  await clouds.getByRole("checkbox", { name: "Clouds", exact: true }).check();
  await expect
    .poll(() => token(page, "--decor-cloud"), "on again, at the same strength")
    .toBe("0.1");
  await expect(strength, "and the row comes back holding the kept strength").toHaveValue("0.1");
  await expect(strength).toBeEnabled();
});

/** The rows both motifs share go only when neither motif is on. */
test("with both switched off, the shared rows go too and the explanation stays", async ({
  page,
}) => {
  await page.goto("/?qa=analysis");
  await appearance(page);
  await page.getByRole("checkbox", { name: "Clouds", exact: true }).uncheck();
  await page.getByRole("checkbox", { name: "Sakura in the rail", exact: true }).uncheck();
  await expect(page.getByRole("slider", { name: /^Pattern size/ })).toHaveCount(0);
  await expect(page.getByRole("checkbox", { name: "Soften the watermarks" })).toHaveCount(0);
  await expect(page.locator("details.more", { hasText: "More about Decoration" })).toBeVisible();
  await assertContained(page);

  await page.getByRole("checkbox", { name: "Sakura in the rail", exact: true }).check();
  await expect(page.getByRole("slider", { name: /^Pattern size/ })).toBeVisible();
});

test("the sakura switches the same way, and leaves the clouds alone", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await appearance(page);
  const sakura = motif(page, "Sakura in the rail");
  await sakura.getByRole("slider", { name: /^Strength/ }).fill("0.2");
  await motif(page, "Clouds")
    .getByRole("slider", { name: /^Strength/ })
    .fill("0.05");

  await sakura.getByRole("checkbox", { name: "Sakura in the rail", exact: true }).uncheck();
  await expect.poll(() => token(page, "--decor-sakura")).toBe("0");
  await expect
    .poll(() => token(page, "--decor-cloud"), "the other motif is untouched")
    .toBe("0.05");
});

/**
 * **One press from any page.** The quick menu's two switches are the same settings, so flipping
 * one there is seen by the settings page and by the token — not a second copy that drifts.
 */
test("the quick menu switches both, from a page that is not Settings", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await appearance(page);
  await motif(page, "Clouds")
    .getByRole("slider", { name: /^Strength/ })
    .fill("0.08");
  await motif(page, "Sakura in the rail")
    .getByRole("slider", { name: /^Strength/ })
    .fill("0.15");
  await page.locator("button.exit").click();

  await page.getByRole("button", { name: "Reference", exact: true }).click();
  await gear(page).click();
  const clouds = menu(page).getByRole("checkbox", { name: "Clouds", exact: true });
  const sakura = menu(page).getByRole("checkbox", { name: "Sakura", exact: true });
  await expect(clouds).toBeChecked();
  await expect(sakura).toBeChecked();
  await assertContained(page);

  await clouds.uncheck();
  await sakura.uncheck();
  await expect.poll(() => token(page, "--decor-cloud")).toBe("0");
  await expect.poll(() => token(page, "--decor-sakura")).toBe("0");

  await page.keyboard.press("Escape");
  await appearance(page);
  await expect(page.getByRole("checkbox", { name: "Clouds", exact: true })).not.toBeChecked();
  await expect(
    page.getByRole("checkbox", { name: "Sakura in the rail", exact: true }),
  ).not.toBeChecked();
  await expect(motif(page, "Clouds").getByRole("slider")).toHaveCount(0);

  await gear(page).click();
  await menu(page).getByRole("checkbox", { name: "Clouds", exact: true }).check();
  await expect.poll(() => token(page, "--decor-cloud"), "back at its strength").toBe("0.08");
  await page.keyboard.press("Escape");
  await expect(motif(page, "Clouds").getByRole("slider", { name: /^Strength/ })).toHaveValue(
    "0.08",
  );
});
