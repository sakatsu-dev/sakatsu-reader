import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { openHelp, openSettings } from "./menu";
import { assertContained } from "./contained";

const root = (page: Page) => page.locator("html");

/**
 * A section id this build does not have — what a settings file written by a newer version hands
 * an older one. The page must say so and stay usable, not render a nav with nothing beside it,
 * and not fall over reading a title for a section it cannot find.
 */
test("a section from a newer version is named as missing, and the nav still works", async ({
  page,
}) => {
  await page.goto("/?qa=analysis&section=from-a-newer-version");
  await expect(page.getByPlaceholder("Paste Japanese text")).toBeVisible();
  // Collected from here: what is under test is the settings page rendering an unknown section,
  // not the app's start-up, which has noise of its own under the QA mock.
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await openSettings(page);

  await expect(
    page.getByText("That section is not in this version. Pick one from the list."),
  ).toBeVisible();
  await expect(page.locator(".pane-head")).toHaveCount(0);
  await expect(page.locator("nav.sections [aria-current]")).toHaveCount(0);
  await assertContained(page);

  await page.locator("nav.sections").getByRole("button", { name: "Capture", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Capture", exact: true, level: 3 })).toBeVisible();
  await expect(page.getByText("That section is not in this version.")).toHaveCount(0);
  expect(errors).toEqual([]);
});

/** Choose a mode the way a reader does. */
async function chooseMode(page: Page, mode: "Light" | "Dark" | "System") {
  await page
    .locator("nav.sections")
    .getByRole("button", { name: "Appearance", exact: true })
    .click();
  // The label, as a pointer would: the radio inside it is drawn by the label rather than itself.
  await page.getByRole("radiogroup", { name: "Mode" }).locator("label", { hasText: mode }).click();
  await expect(page.getByRole("radio", { name: mode, exact: true })).toBeChecked();
}

/**
 * **System follows the desktop, and only System does.** The desktop is asked through the window,
 * not `prefers-color-scheme` (see `theme-live.ts`), so the QA mock answers that question and
 * can change its answer mid-session the way Windows does.
 */
test("System follows the desktop while it is chosen, and Light ignores it", async ({ page }) => {
  await page.goto("/?qa=analysis&os=light");
  await openSettings(page);

  await chooseMode(page, "System");
  await expect(root(page)).toHaveAttribute("data-theme", "light");

  await page.evaluate(() =>
    (
      window as unknown as { __SAKATSU_QA__: { setOsTheme: (t: string) => void } }
    ).__SAKATSU_QA__.setOsTheme("dark"),
  );
  await expect(root(page)).toHaveAttribute("data-theme", "dark");

  await chooseMode(page, "Light");
  await expect(root(page)).toHaveAttribute("data-theme", "light");
  await page.evaluate(() =>
    (
      window as unknown as { __SAKATSU_QA__: { setOsTheme: (t: string) => void } }
    ).__SAKATSU_QA__.setOsTheme("dark"),
  );
  // Long enough for a repaint to have landed if one were coming.
  await page.waitForTimeout(250);
  await expect(root(page)).toHaveAttribute("data-theme", "light");
});

const SECTIONS = [
  "General",
  "Appearance",
  "Capture",
  "Search",
  "Recognition",
  "Anki",
  "Diagnostics",
  "Updates",
];

/**
 * Help lists the sections and opens each one. It never says what a row does — the row does —
 * so what is checked here is the way in, and that the way back out still names the page you
 * were reading rather than Help, which is a detour of its own.
 */
test("Help opens each settings section, and the way back still names the page you came from", async ({
  page,
}) => {
  await page.goto("/?qa=analysis");
  await openHelp(page);
  await expect(page.getByRole("heading", { name: "Settings, section by section" })).toBeVisible();
  for (const label of SECTIONS) {
    await expect(
      page.getByRole("button", { name: `Open ${label} settings`, exact: true }),
    ).toBeVisible();
  }
  await expect(page.getByRole("button", { name: /^Open .* settings$/ })).toHaveCount(
    SECTIONS.length,
  );
  await assertContained(page);

  await page.getByRole("button", { name: "Open Capture settings", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Settings", exact: true })).toBeVisible();
  await expect(
    page.locator("nav.sections").getByRole("button", { name: "Capture", exact: true }),
  ).toHaveAttribute("aria-current", "page");
  await expect(page.locator("button.exit")).toHaveAttribute("aria-label", /Reading$/);
});

/** Go to one section through its nav button. */
async function section(page: Page, label: string) {
  await page.locator("nav.sections").getByRole("button", { name: label, exact: true }).click();
  await expect(page.getByRole("heading", { name: label, exact: true, level: 3 })).toBeVisible();
}

/**
 * Every section has a real header — its name as the pane's heading and one line on what it is
 * for — and stays inside itself at every window size. Containment for every section is new
 * coverage: the older specs checked the sections they happened to visit.
 */
test("each section has its title and description, and stays inside itself", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  for (const label of SECTIONS) {
    await section(page, label);
    await expect(page.locator(".pane-head p")).not.toBeEmpty();
    // No section may show the QA harness's own gaps: a command the mock does not answer prints
    // "no IPC fixture" into the page, which is a fault in the harness posing as one in the app.
    await expect(page.locator(".pane")).not.toContainText("no IPC fixture");
    await assertContained(page);
  }
});

/**
 * The nav's icons are decoration: each button keeps exactly the name it had, and its one drawing
 * is hidden from assistive technology.
 *
 * **Centred by measurement.** `traps.md` records rail icons that shipped a unit off and a gear that
 * sat visibly left of its button; this unions each icon's ink, grows it by half the stroke, and
 * holds the centre to half a unit of the 24-unit box. Size is checked too, because the glossary
 * icon once read light beside its neighbours with half their ink.
 */
test("the section nav has one hidden icon per section, centred in its box", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  const nav = page.locator("nav.sections");
  for (const label of SECTIONS) {
    const button = nav.getByRole("button", { name: label, exact: true });
    await expect(button).toHaveCount(1);
    await expect(button.locator("svg")).toHaveCount(1);
    await expect(button.locator("svg")).toHaveAttribute("aria-hidden", "true");
  }
  const ink = await nav.locator("svg").evaluateAll((icons) =>
    icons.map((svg) => {
      const stroke = parseFloat(svg.getAttribute("stroke-width") ?? "0") / 2;
      let [left, top, right, bottom] = [Infinity, Infinity, -Infinity, -Infinity];
      for (const shape of svg.querySelectorAll("path")) {
        const box = (shape as SVGGraphicsElement).getBBox();
        left = Math.min(left, box.x - stroke);
        top = Math.min(top, box.y - stroke);
        right = Math.max(right, box.x + box.width + stroke);
        bottom = Math.max(bottom, box.y + box.height + stroke);
      }
      return { x: (left + right) / 2, y: (top + bottom) / 2, w: right - left, h: bottom - top };
    }),
  );
  expect(ink).toHaveLength(SECTIONS.length);
  for (const [index, box] of ink.entries()) {
    const name = SECTIONS[index];
    expect.soft(Math.abs(box.x - 12), `${name} is off-centre across`).toBeLessThanOrEqual(0.5);
    expect.soft(Math.abs(box.y - 12), `${name} is off-centre down`).toBeLessThanOrEqual(0.5);
    expect.soft(Math.max(box.w, box.h), `${name} is too small`).toBeGreaterThanOrEqual(15);
    expect.soft(Math.max(box.w, box.h), `${name} is too large`).toBeLessThanOrEqual(20);
  }
});

/**
 * **Mode is a radio group from the keyboard, not only to a screen reader.** One Tab stop, on the
 * chosen mode; the arrow keys move *and* choose, and wrap; the next Tab leaves the group. That is
 * the W3C radio-group pattern, and native radios are what supply it — this checks they still do.
 *
 * `?keyboard=tab`, because the reader-first default ignores Tab altogether.
 */
test("Mode is one Tab stop, the arrows move and choose, and the focus ring shows", async ({
  page,
}) => {
  await page.goto("/?qa=analysis&keyboard=tab");
  await openSettings(page);
  await section(page, "Appearance");

  const group = page.getByRole("radiogroup", { name: "Mode" });
  await expect(group.getByRole("radio")).toHaveCount(3);
  const dark = group.getByRole("radio", { name: "Dark", exact: true });
  const light = group.getByRole("radio", { name: "Light", exact: true });
  const system = group.getByRole("radio", { name: "System", exact: true });
  await expect(dark).toBeChecked();

  // Into the group from the last control before it that takes focus — Rename and Forget are
  // disabled until a palette of your own is chosen — which lands on the checked radio only.
  await page.getByRole("button", { name: "Save as…" }).focus();
  await page.keyboard.press("Tab");
  await expect(dark).toBeFocused();
  const ring = await group
    .locator("label", { has: page.getByRole("radio", { name: "Dark", exact: true }) })
    .evaluate((label) => getComputedStyle(label).outlineStyle);
  expect(ring).not.toBe("none");

  await page.keyboard.press("ArrowRight");
  await expect(system).toBeFocused();
  await expect(system).toBeChecked();
  await page.keyboard.press("ArrowRight");
  await expect(light).toBeChecked();
  await expect(root(page)).toHaveAttribute("data-theme", "light");
  await page.keyboard.press("ArrowLeft");
  await expect(system).toBeChecked();
  await page.keyboard.press("ArrowUp");
  await expect(dark).toBeChecked();
  await expect(root(page)).toHaveAttribute("data-theme", "dark");

  // And out again with one press: the other two are not stops of their own.
  await page.keyboard.press("Tab");
  await expect(group.locator("input:focus")).toHaveCount(0);
});

/**
 * **Every More is named after its setting.** Forty "More"s in a list of buttons are forty
 * identical entries; each carries hidden text naming the setting it explains, so its name is
 * "More about …" — unique, and the same label the search results use. It starts shut, and opening
 * it shows the reasoning.
 */
test("every More names its setting, starts shut, and opens", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  const names: string[] = [];
  for (const label of SECTIONS) {
    await section(page, label);
    for (const summary of await page.locator("details.more > summary").all()) {
      if (!(await summary.isVisible())) continue;
      const text = (await summary.textContent())!.trim();
      expect(text).toMatch(/^More about \S/);
      await expect(summary).toHaveAccessibleName(text);
      names.push(text);
    }
  }
  expect(names.length).toBeGreaterThanOrEqual(25);
  const repeated = names.filter((name, index) => names.indexOf(name) !== index);
  expect(repeated).toEqual([]);

  await section(page, "Capture");
  const fold = page.locator("details.more", {
    hasText: "More about Hide the frame while capturing",
  });
  const body = fold.getByText("transparent middle keeps it out of its own screenshot");
  await expect(body).toBeHidden();
  await fold.locator("summary").click();
  await expect(body).toBeVisible();
});

/**
 * **A search leaves the rows it matched and the cards around them, and nothing else.** "contrast"
 * is one item in Appearance; its card stays and every other card goes, heading included.
 */
test("a search leaves only the matching item and its card", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  await section(page, "Appearance");
  await page.getByRole("searchbox", { name: "Search settings" }).fill("contrast");
  await expect(page.locator(".cluster:visible")).toHaveCount(1);
  await expect(page.locator(".item:visible")).toHaveCount(1);
  await expect(page.getByRole("slider", { name: /^Contrast/ })).toBeVisible();
  await expect(page.locator(".block:visible h4")).toHaveText("Adjust the palette");
});

/**
 * **The smallest and largest Interface size, with everything open and the longest content the
 * page can be given** — a path with nowhere to break, the warnings, a deck name from a real
 * collection, the index offer. 110 characters is a writing limit, not a promise of one line:
 * what is checked here is that wrapped text stays whole — nothing clipped, nothing ellipsised,
 * nothing pushing the pane sideways.
 */
for (const size of [12, 24]) {
  test(`at an Interface size of ${size}px, every section and Help stay whole with everything open`, async ({
    page,
  }) => {
    await page.goto("/?qa=analysis&fixture=long-paths&upkeep=missing");
    await openSettings(page);
    // The Interface slider itself, at its own minimum and maximum: setting the root's font size
    // by hand is undone when the app next applies the stored size, and this is what a reader does.
    await page.getByRole("slider", { name: /^Interface/ }).fill(String(size));
    await expect
      .poll(() =>
        page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize)),
      )
      .toBe(size);
    for (const label of SECTIONS) {
      await section(page, label);
      await page.locator(".pane details").evaluateAll((folds) => {
        for (const fold of folds) (fold as HTMLDetailsElement).open = true;
      });
      await assertContained(page);
      const sideways = await page
        .locator(".pane")
        .evaluate((pane) => pane.scrollWidth - pane.clientWidth);
      expect(sideways, `${label} scrolls sideways`).toBeLessThanOrEqual(1);
      const clipped = await page.locator(".pane").evaluate((pane) =>
        [
          ...pane.querySelectorAll(
            ".note, .more p, .row > span:first-child, .pane-head p, h4, code",
          ),
        ]
          .filter((element) => element.getClientRects().length > 0)
          .flatMap((element) => {
            const style = getComputedStyle(element);
            const faults = [];
            if (element.clientWidth > 0 && element.scrollWidth > element.clientWidth + 1)
              faults.push("overflows");
            if (style.textOverflow === "ellipsis") faults.push("ellipsis");
            if (style.whiteSpace === "nowrap" || style.whiteSpace === "pre") faults.push("nowrap");
            if (style.getPropertyValue("-webkit-line-clamp") !== "none") faults.push("line-clamp");
            return faults.map((fault) => `${fault}: ${element.textContent!.trim().slice(0, 60)}`);
          }),
      );
      expect(clipped, `${label} at ${size}px`).toEqual([]);
    }
    // The warnings and the long path are actually on screen, or this proved nothing.
    await section(page, "Anki");
    await expect(page.locator(".note.warn").first()).toBeVisible();
    await section(page, "Diagnostics");
    await expect(page.locator(".note.path").first()).toContainText("NeverBreaks");

    await page.locator("button.exit").click();
    await openHelp(page);
    await assertContained(page);
  });
}
