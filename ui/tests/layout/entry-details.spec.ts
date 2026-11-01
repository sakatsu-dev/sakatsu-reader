import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { assertContained, assertPageDoesNotScrollSideways } from "./contained";
import { openSettings } from "./menu";

type Harness = {
  commands: string[];
  failNext: (command: string) => void;
  releaseRequest: (key: string) => boolean;
  pendingRequests: () => string[];
};
async function count(page: Page, command: string) {
  return page.evaluate(
    (command) =>
      (window as Window & { __SAKATSU_QA__: Harness }).__SAKATSU_QA__.commands.filter(
        (c) => c === command,
      ).length,
    command,
  );
}
async function reference(page: Page, extra = "") {
  await page.goto(`/?qa=reference${extra}`);
  await page.getByRole("button", { name: "Reference", exact: true }).click();
  const input = page.getByRole("textbox", { name: "Search Reference" });
  await input.fill("図書館");
  await input.press("Enter");
  const detail = page.getByRole("region", { name: "図書館 detail" });
  await expect(detail).toBeVisible();
  return detail;
}

async function entryContrast(page: Page, selector: string) {
  return page.locator(selector).evaluate((el) => {
    const style = getComputedStyle(el);
    const root = getComputedStyle(document.documentElement);
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const context = canvas.getContext("2d")!;
    function luminance(color: string) {
      context.clearRect(0, 0, 1, 1);
      context.fillStyle = color;
      context.fillRect(0, 0, 1, 1);
      const rgb = [...context.getImageData(0, 0, 1, 1).data].slice(0, 3).map((v) => {
        const c = v / 255;
        return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
      });
      return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
    }
    function contrast(a: string, b: string) {
      const x = luminance(a),
        y = luminance(b);
      return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
    }
    const ground = root.getPropertyValue("--bg").trim();
    return {
      rule: contrast(style.borderBottomColor, ground),
      decorativeRule: contrast(root.getPropertyValue("--border").trim(), ground),
      fill: style.backgroundColor,
      shadow: style.boxShadow,
      text: contrast(root.getPropertyValue("--text-dim").trim(), style.backgroundColor),
      height: el.getBoundingClientRect().height,
      headerHeight: el.querySelector(".preview")!.getBoundingClientRect().height,
    };
  });
}

for (const motion of ["on", "off"]) {
  test(`hover previews the target without opening and respects motion ${motion}`, async ({
    page,
  }, info) => {
    for (const view of ["Reference", "Glossary"]) {
      const fixture = view === "Reference" ? "parts" : "glossary";
      await page.goto(`/?qa=reference&fixture=${fixture}&motion=${motion}`);
      await page.getByRole("button", { name: view, exact: true }).click();
      if (view === "Reference") {
        const input = page.getByRole("textbox", { name: "Search Reference" });
        await input.fill("first");
        await input.press("Enter");
      }
      const collection = page.locator(view === "Reference" ? ".result-collection" : ".collection");
      const item = collection.locator(view === "Reference" ? ":scope > .result" : ".term").nth(1);
      const preview = item.locator(".preview");
      const opener = item.locator("> .disclose");
      await page.mouse.move(12, 12);
      const before = await preview.evaluate((el) => getComputedStyle(el).backgroundColor);
      const duration = await preview.evaluate((el) =>
        parseFloat(getComputedStyle(el).transitionDuration),
      );
      expect(duration > 0).toBe(motion === "on");
      await expect(opener).toHaveAttribute("aria-expanded", "false");
      await preview.hover();
      await expect
        .poll(() => preview.evaluate((el) => getComputedStyle(el).backgroundColor))
        .not.toBe(before);
      await expect(opener).toHaveAttribute("aria-expanded", "false");
      await expect(item).not.toHaveAttribute("data-focused", "");
      await expect(collection.locator(".inline-detail")).toHaveCount(0);
      expect(await count(page, "explain_tags")).toBe(0);
      expect(await count(page, "patterns_for_term")).toBe(0);
      await page.screenshot({
        path: info.outputPath(`${view}-hover-${motion}.png`),
        animations: "disabled",
      });
      await preview.click({ position: { x: 4, y: 4 } });
      await expect(opener).toHaveAttribute("aria-expanded", "true");
      await preview.click({ position: { x: 4, y: 4 } });
      await expect(opener).toHaveAttribute("aria-expanded", "false");
    }
  });
}

for (const theme of ["dark", "light"]) {
  test(`entry separation and full expanded extent remain clear in ${theme}`, async ({
    page,
  }, info) => {
    await reference(page, `&theme=${theme}`);
    const word = await entryContrast(page, ".result.expanded > .card");
    expect(word.rule).toBeGreaterThan(word.decorativeRule * 1.3);
    expect(word.fill).not.toBe("rgba(0, 0, 0, 0)");
    expect(word.shadow).not.toBe("none");
    expect(word.text).toBeGreaterThanOrEqual(4.5);
    expect(word.height).toBeGreaterThan(word.headerHeight);
    await page.screenshot({ path: info.outputPath(`reference-contrast-${theme}.png`) });

    await page.goto(`/?qa=reference&fixture=glossary&theme=${theme}`);
    await page.getByRole("button", { name: "Glossary", exact: true }).click();
    const opener = page.locator('.disclose[data-term="conjunctive"]');
    await opener.focus();
    await opener.press("Enter");
    const term = await entryContrast(page, ".term.expanded");
    expect(term.rule).toBeGreaterThan(term.decorativeRule * 1.3);
    expect(term.fill).not.toBe("rgba(0, 0, 0, 0)");
    expect(term.shadow).not.toBe("none");
    expect(term.text).toBeGreaterThanOrEqual(4.5);
    expect(term.height).toBeGreaterThan(term.headerHeight);
    const hierarchy = await page.locator(".term.expanded").evaluate((el) => ({
      title: parseFloat(getComputedStyle(el.querySelector(".preview strong")!).fontSize),
      weight: Number(getComputedStyle(el.querySelector(".preview strong")!).fontWeight),
      body: parseFloat(getComputedStyle(el.querySelector(".term-detail > .detail")!).fontSize),
      otherTitle: parseFloat(
        getComputedStyle(document.querySelector(".term:not(.expanded) .preview strong")!).fontSize,
      ),
    }));
    expect(hierarchy.title).toBeGreaterThan(hierarchy.body);
    expect(hierarchy.title).toBeGreaterThan(hierarchy.otherTitle);
    expect(hierarchy.weight).toBeGreaterThan(600);
    await assertContained(page);
    await page.screenshot({ path: info.outputPath(`glossary-contrast-${theme}.png`) });
  });
}

test("the word answer stays primary and character resources are requested only on demand", async ({
  page,
}) => {
  const detail = await reference(page);
  await expect(detail.locator(".word-column")).toContainText("library");
  await expect(detail.getByRole("button", { name: /^Study / })).toHaveCount(3);
  await expect(detail.locator(".character")).toHaveCount(0);
  for (const command of ["kanji_diagram", "kanji_compounds"])
    expect(await count(page, command)).toBe(0);
  await detail.getByRole("button", { name: "Study 図", exact: true }).click();
  await expect(detail.locator(".character")).toHaveCount(1);
  for (const command of ["kanji_diagram", "kanji_compounds"])
    await expect.poll(() => count(page, command)).toBe(1);
  await expect(detail.locator(".word-column")).toContainText("library");
  await detail.getByRole("button", { name: "Study 書", exact: true }).click();
  await expect(detail.locator(".character")).toHaveCount(1);
  await expect(detail.getByRole("complementary", { name: "Details for 書" })).toBeVisible();
  await expect(detail.getByRole("complementary", { name: "Details for 図" })).toHaveCount(0);
});

test("character choices support keyboard operation, closing and per-search state", async ({
  page,
}) => {
  const detail = await reference(page, "&keyboard=tab");
  const choice = detail.getByRole("button", { name: "Study 図", exact: true });
  await choice.focus();
  await choice.press("Enter");
  await expect(choice).toHaveAttribute("aria-expanded", "true");
  await page.getByRole("button", { name: "New search" }).click();
  await page.getByRole("button", { name: "図書館", exact: true }).click();
  await expect(choice).toHaveAttribute("aria-expanded", "true");
  await detail.getByRole("button", { name: "Close character details" }).click();
  await expect(choice).toBeFocused();
  await expect(choice).toBeInViewport();
  await choice.press("Space");
  await expect(choice).toHaveAttribute("aria-expanded", "true");
  const input = page.getByRole("textbox", { name: "Search Reference" });
  await input.fill("another");
  await input.press("Enter");
  await expect(detail.locator(".character-inspector")).toHaveCount(0);
});

test("a late diagram cannot replace the character selected afterward", async ({ page }) => {
  const detail = await reference(page, "&hold-diagram");
  await detail.getByRole("button", { name: "Study 図", exact: true }).click();
  await detail.getByRole("button", { name: "Study 書", exact: true }).click();
  for (const key of ["diagram:書", "diagram:図"]) {
    await expect
      .poll(() =>
        page.evaluate(() =>
          (window as Window & { __SAKATSU_QA__: Harness }).__SAKATSU_QA__.pendingRequests(),
        ),
      )
      .toContain(key);
    expect(
      await page.evaluate(
        (key) =>
          (window as Window & { __SAKATSU_QA__: Harness }).__SAKATSU_QA__.releaseRequest(key),
        key,
      ),
    ).toBe(true);
  }
  await expect(detail.getByRole("img", { name: "Stroke-order diagram for 書" })).toBeVisible();
  await expect(detail.getByRole("img", { name: "Stroke-order diagram for 図" })).toHaveCount(0);
});

test("the Reference and Glossary surfaces use a wide window without restoring a page cap", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  const detail = await reference(page);
  await detail.getByRole("button", { name: "Study 図", exact: true }).click();
  const word = await detail.locator(".word-column").boundingBox();
  const inspector = await detail.locator(".character-inspector").boundingBox();
  expect(inspector!.x).toBeGreaterThanOrEqual(word!.x + word!.width);
  expect(inspector!.x - word!.x - word!.width).toBeLessThan(64);
  const choices = await detail.locator(".character-choices").boundingBox();
  expect(choices!.y - word!.y - word!.height).toBeLessThan(48);
  expect(await page.locator(".reference").evaluate((el) => el.clientWidth)).toBeGreaterThan(1600);
  await assertContained(page);
  await assertPageDoesNotScrollSideways(page);
  await page.goto("/?qa=reference&fixture=glossary");
  await page.getByRole("button", { name: "Glossary", exact: true }).click();
  expect(await page.locator(".glossary").evaluate((el) => el.clientWidth)).toBeGreaterThan(1600);
});

test("structured personal-name evidence replaces the unhelpful Other label", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=personal-name");
  await page.getByRole("button", { name: "Reference", exact: true }).click();
  const input = page.getByRole("textbox", { name: "Search Reference" });
  await input.fill("山田太郎");
  await input.press("Enter");
  await expect(page.locator(".classification")).toHaveText("Personal name");
  await expect(page.locator(".word-column")).toContainText("Yamada Tarō");
});

test("Reference keeps connected patterns optional, lazy and retryable", async ({ page }) => {
  await page.goto("/?qa=reference&fixture=mixed");
  await page.getByRole("button", { name: "Reference", exact: true }).click();
  const input = page.getByRole("textbox", { name: "Search Reference" });
  await input.fill("te form");
  await input.press("Enter");
  await page.getByRole("radio", { name: /^Glossary terms/ }).click();
  const opener = page.locator(".result-collection .disclose").first();
  await opener.focus();
  await opener.press("Enter");
  const detail = page.getByRole("region", { name: "Conjunctive form detail" });
  expect(await count(page, "patterns_for_term")).toBe(0);
  await expect(detail.getByText("Try eating it.", { exact: true })).toBeVisible();
  await page.evaluate(() =>
    (window as Window & { __SAKATSU_QA__: Harness }).__SAKATSU_QA__.failNext("patterns_for_term"),
  );
  await detail.locator(".pattern-disclosure summary").click();
  await expect(detail.getByRole("alert")).toHaveText("Could not load connected patterns.");
  await detail.getByRole("button", { name: "Retry patterns" }).click();
  await expect(detail.getByText("〜てくる", { exact: true })).toBeVisible();
  await detail.locator(".pattern-disclosure summary").click();
  await expect(detail.getByText("〜てくる", { exact: true })).toHaveCount(0);
});

for (const theme of ["dark", "light"]) {
  test(`requested character detail is readable across text sizes in ${theme}`, async ({
    page,
  }, info) => {
    for (const size of [12, 24]) {
      await reference(page, `&theme=${theme}`);
      await openSettings(page);
      await page.getByRole("slider", { name: /^Interface/ }).fill(String(size));
      await page.getByRole("button", { name: "Reference", exact: true }).click();
      await page.getByRole("button", { name: "Study 図", exact: true }).click();
      const detail = page.getByRole("region", { name: "図書館 detail" });
      await expect(detail.locator(".character-inspector")).toBeVisible();
      await expect(detail.locator(".character-inspector header")).toBeInViewport();
      await assertContained(page);
      await assertPageDoesNotScrollSideways(page);
      await page.mouse.move(page.viewportSize()!.width / 2, 12);
      await page.screenshot({
        path: info.outputPath(`character-${theme}-${size}.png`),
        fullPage: true,
      });
    }
  });
}
