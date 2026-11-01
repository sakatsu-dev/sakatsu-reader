import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { assertContained, assertPageDoesNotScrollSideways } from "./contained";
import { openSettings } from "./menu";

type Harness = {
  commands: string[];
  pendingRequests: () => string[];
  releaseRequest: (key: string, how?: "complete" | "fail") => boolean;
  answerNext: (command: string, value: unknown) => void;
};

async function qa<T>(page: Page, action: (h: Harness) => T) {
  return page.evaluate(
    action,
    await page.evaluateHandle(
      () => (window as Window & { __SAKATSU_QA__: Harness }).__SAKATSU_QA__,
    ),
  );
}

async function open(page: Page, held = false) {
  await page.goto(`/?qa=reference&fixture=parts${held ? "&hold-search" : ""}`);
  await page.getByRole("button", { name: "Reference", exact: true }).click();
  return page.getByRole("textbox", { name: "Search Reference" });
}

async function release(page: Page, query: string, how: "complete" | "fail" = "complete") {
  const key = `search:${query}`;
  await expect.poll(() => qa(page, (h) => h.pendingRequests())).toContain(key);
  expect(
    await page.evaluate(
      ({ key, how }) =>
        (window as Window & { __SAKATSU_QA__: Harness }).__SAKATSU_QA__.releaseRequest(key, how),
      { key, how },
    ),
  ).toBe(true);
}

test("typing prefetches without submitting; Search and Clear keep separate purposes", async ({
  page,
}) => {
  const input = await open(page);
  await expect(page.getByText("Also finds grammar patterns", { exact: false })).toBeVisible();
  await input.fill("first");
  await expect(page.getByText("Press Enter to search", { exact: true })).toBeVisible();
  await expect
    .poll(() => qa(page, (h) => h.commands.filter((c) => c === "prefetch_reference").length))
    .toBeGreaterThan(0);
  expect(await qa(page, (h) => h.commands.filter((c) => c === "search_words").length)).toBe(0);
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(page.getByText("Results for “first”", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Search", exact: true })).toBeVisible();
  await input.fill("second");
  await expect(page.getByText("Results for “first”", { exact: true })).toBeVisible();
  await expect(page.getByText("Press Enter to search", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Clear search", exact: true }).click();
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("");
  await expect(page.getByRole("list", { name: "Reference results" })).toHaveCount(0);
});

test("Japanese composition does not submit until composition ends", async ({ page }) => {
  const input = await open(page);
  await input.fill("に");
  await input.dispatchEvent("compositionstart");
  await input.press("Enter");
  expect(await qa(page, (h) => h.commands.filter((c) => c === "search_words").length)).toBe(0);
  await input.dispatchEvent("compositionend");
  await input.press("Enter");
  await expect(page.getByText("Results for “に”", { exact: true })).toBeVisible();
});

test("a failed replacement keeps the old answer and retries the failed question", async ({
  page,
}) => {
  const input = await open(page, true);
  await input.fill("first");
  await input.press("Enter");
  await release(page, "first");
  const first = page.locator(".result > .disclose").first();
  await first.focus();
  await first.press("Enter");
  await input.fill("broken");
  await input.press("Enter");
  await expect(page.getByText("Searching for “broken”…", { exact: true })).toBeVisible();
  await expect(page.getByText("Results for “first”", { exact: true })).toBeVisible();
  await release(page, "broken", "fail");
  await expect(page.getByRole("alert")).toContainText("broken");
  await expect(first).toHaveAttribute("aria-expanded", "true");
  await input.fill("unsubmitted");
  await page.getByRole("button", { name: "Retry search" }).click();
  await release(page, "broken");
  await expect(page.getByText("Results for “broken”", { exact: true })).toBeVisible();
  await expect(input).toHaveValue("unsubmitted");
});

test("clearing an initial request refuses its late reply", async ({ page }) => {
  const input = await open(page, true);
  await input.fill("late");
  await input.press("Enter");
  await expect(page.getByText("Searching for “late”…", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Clear search", exact: true }).click();
  await release(page, "late");
  await expect(page.locator(".search-status")).toBeEmpty();
  await expect(page.getByRole("list", { name: "Reference results" })).toHaveCount(0);
  await expect(input).toBeFocused();
});

test("rapid searches, switching tabs and closing tabs keep replies with their owner", async ({
  page,
}) => {
  const input = await open(page, true);
  await input.fill("older");
  await input.press("Enter");
  await input.fill("newer");
  await input.press("Enter");
  await release(page, "newer");
  await release(page, "older");
  await expect(page.getByText("Results for “newer”", { exact: true })).toBeVisible();
  await input.fill("background");
  await input.press("Enter");
  await page.getByRole("button", { name: "New search" }).click();
  await input.fill("front");
  await input.press("Enter");
  await release(page, "front");
  await release(page, "background");
  await expect(page.getByText("Results for “front”", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "background", exact: true }).click();
  await expect(page.getByText("Results for “background”", { exact: true })).toBeVisible();
  await input.fill("closed");
  await input.press("Enter");
  await input.press("Control+w");
  await release(page, "closed");
  await expect(page.getByText("Results for “front”", { exact: true })).toBeVisible();
  await input.press("Control+Shift+t");
  await expect(page.getByText("Results for “closed”", { exact: true })).toBeVisible();
});

test("repeating a successful search preserves selection, filters and scroll", async ({ page }) => {
  const input = await open(page);
  await input.fill("first");
  await input.press("Enter");
  const collection = page.getByRole("list", { name: "Reference results" });
  const selected = collection.locator(".disclose").nth(1);
  await selected.focus();
  await selected.press("Enter");
  const before = await qa(page, (h) => h.commands.filter((c) => c === "search_words").length);
  const scroll = await collection.evaluate((el) => el.scrollTop);
  await input.press("Enter");
  await expect(selected).toHaveAttribute("aria-expanded", "true");
  expect(await qa(page, (h) => h.commands.filter((c) => c === "search_words").length)).toBe(before);
  expect(await collection.evaluate((el) => el.scrollTop)).toBe(scroll);
});

test("filters start shut, keep active scope visible, and recover empty results locally", async ({
  page,
}) => {
  const input = await open(page);
  await input.fill("first");
  await input.press("Enter");
  const details = page.locator(".word-filter-bar details");
  await expect(details).not.toHaveAttribute("open");
  await details.locator("summary").click();
  await page.getByRole("button", { name: /^Noun 2$/ }).click();
  await details.locator("summary").click();
  await expect(page.locator(".active-filters")).toContainText("Words only: Noun");
  await page.getByRole("button", { name: "New search" }).click();
  await page.getByRole("button", { name: "first", exact: true }).click();
  await expect(page.locator(".active-filters")).toBeVisible();
  const before = await qa(page, (h) => h.commands.filter((c) => c === "search_words").length);
  await page.getByRole("radio", { name: /^Grammar patterns 0$/ }).click();
  await expect(
    page.getByText("No results remain under these filters.", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Clear filters", exact: true }).click();
  await expect(page.locator(".result")).toHaveCount(5);
  expect(await qa(page, (h) => h.commands.filter((c) => c === "search_words").length)).toBe(before);
});

test("Glossary names and aliases filter immediately and empty state offers recovery", async ({
  page,
}) => {
  await page.goto("/?qa=reference&fixture=glossary");
  await page.getByRole("button", { name: "Glossary", exact: true }).click();
  const input = page.getByRole("textbox", { name: "Find a glossary term" });
  await input.fill("te-form");
  await expect(page.locator(".term")).toHaveCount(1);
  await expect(page.getByText("Matched “te-form”", { exact: true })).toBeVisible();
  await input.fill("unmatched");
  await expect(
    page.getByText("No term name or alias matches “unmatched”.", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Clear filter", exact: true }).click();
  await expect(page.locator(".term")).toHaveCount(4);
});

test("a successful empty answer is distinct from failure and filtered emptiness", async ({
  page,
}) => {
  const input = await open(page);
  await qa(page, (h) =>
    h.answerNext("search_words", {
      words: [],
      japanese: false,
      ranked_by: null,
      needs_gloss_index: false,
      capped: false,
    }),
  );
  await input.fill("unmatched");
  await input.press("Enter");
  await expect(page.getByText("Nothing matching", { exact: false })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Clear filters", exact: true })).toHaveCount(0);
});

test("the search glyph and attached controls share one continuous focus border", async ({
  page,
}) => {
  const input = await open(page);
  const submit = page.getByRole("button", { name: "Search", exact: true });
  await expect(submit).toHaveText("");
  await expect(submit.locator("svg circle")).toHaveCount(1);
  await input.fill("first");
  await submit.click();
  await page.getByRole("button", { name: "New search", exact: true }).click();
  await input.fill("second");
  await input.press("Enter");
  await input.focus();
  const shape = () =>
    page.locator(".search-field").evaluate((field) => {
      const bar = field.querySelector(".bar")!;
      const tab = field.querySelector(".tab.current")!;
      const close = field.querySelector(".clear-all")!;
      const b = bar.getBoundingClientRect(),
        t = tab.getBoundingClientRect(),
        c = close.getBoundingClientRect();
      const bs = getComputedStyle(bar),
        ts = getComputedStyle(tab),
        cs = getComputedStyle(close);
      return {
        tabJoin: t.top - b.bottom,
        closeJoin: c.top - b.bottom,
        right: c.right - b.right,
        edge: bs.borderBottomColor,
        tabEdge: ts.borderLeftColor,
        closeEdge: cs.borderRightColor,
        fill: bs.backgroundColor,
        tabSeam: ts.borderTopColor,
        closeSeam: cs.borderTopColor,
        outline: bs.outlineStyle,
        leftCorner: bs.borderBottomLeftRadius,
        rightCorner: bs.borderBottomRightRadius,
      };
    });
  await expect
    .poll(async () => {
      const s = await shape();
      return s.edge === s.tabEdge && s.edge === s.closeEdge;
    })
    .toBe(true);
  const s = await shape();
  expect(s.tabJoin).toBeCloseTo(-1, 0);
  expect(s.closeJoin).toBeCloseTo(-1, 0);
  expect(s.right).toBeCloseTo(0, 0);
  expect(s.tabSeam).toBe(s.fill);
  expect(s.closeSeam).toBe(s.fill);
  expect(s.outline).toBe("none");
  expect(s.leftCorner).toBe("0px");
  expect(s.rightCorner).toBe("0px");
  const close = page.getByRole("button", { name: "Close all searches", exact: true });
  await expect(close).toHaveText("Close all");
  await close.click();
  await expect(page.locator(".tabs .tab")).toHaveCount(1);
  await expect(input).toHaveValue("");
  await input.press("Control+Shift+t");
  await expect(page.getByText("Results for “second”", { exact: true })).toBeVisible();
});

test("empty search controls use their natural height without a private scrollbar", async ({
  page,
}, info) => {
  await open(page);
  await page.getByLabel("New search", { exact: true }).click();
  const controls = page.getByRole("region", { name: "Reference search controls" });
  const geometry = await controls.evaluate((el) => ({
    overflow: getComputedStyle(el).overflowY,
    gutter: getComputedStyle(el).scrollbarGutter,
    // Tooltips may overflow visibly; measure the form, not pseudo-element scroll bounds.
    formBottom: el.querySelector("form")!.getBoundingClientRect().bottom,
    bottom: el.getBoundingClientRect().bottom,
  }));
  expect(geometry.overflow).toBe("visible");
  expect(geometry.formBottom).toBeLessThanOrEqual(geometry.bottom + 1);
  expect(geometry.gutter).toBe("auto");
  expect(
    await page.locator(".reference").evaluate((el) => el.scrollHeight - el.clientHeight),
  ).toBeLessThanOrEqual(1);
  await expect(page.getByText("Also finds grammar patterns", { exact: false })).toBeInViewport();
  await expect(page.getByRole("textbox", { name: "Search Reference" })).toBeInViewport();
  await assertPageDoesNotScrollSideways(page);
  await page.mouse.move(12, 12);
  await page.screenshot({ path: info.outputPath("empty-search.png") });
});

test("short windows scroll the page rather than squeeze search controls", async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 480, height: 400 });
  await open(page);
  await openSettings(page);
  await page.getByRole("slider", { name: /^Interface/ }).fill("24");
  await page.getByRole("button", { name: "Reference", exact: true }).click();
  const input = page.getByRole("textbox", { name: "Search Reference" });
  await input.fill("first");
  await input.press("Enter");
  await page.getByText("Word filters", { exact: true }).click();
  const controls = page.getByRole("region", { name: "Reference search controls" });
  expect(await controls.evaluate((el) => getComputedStyle(el).overflowY)).toBe("visible");
  expect(await controls.evaluate((el) => el.scrollHeight - el.clientHeight)).toBeLessThanOrEqual(1);
  const collection = page.getByRole("list", { name: "Reference results" });
  expect(await collection.evaluate((el) => el.clientHeight)).toBeGreaterThanOrEqual(100);
  const reference = page.locator(".reference");
  expect(await reference.evaluate((el) => el.scrollHeight - el.clientHeight)).toBeGreaterThan(0);
  await reference.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await expect(collection).toBeInViewport();
  await assertPageDoesNotScrollSideways(page);
  await page.screenshot({ path: info.outputPath("short-window-results.png") });
  await input.focus();
  await expect(input).toBeInViewport();
  expect(await page.locator(".app").evaluate((el) => el.scrollTop)).toBe(0);
});

for (const theme of ["dark", "light"]) {
  for (const size of [12, 24]) {
    test(`joined search chrome remains whole in ${theme} at ${size}px`, async ({ page }, info) => {
      await page.goto(`/?qa=reference&fixture=parts&theme=${theme}`);
      await openSettings(page);
      await page.getByRole("slider", { name: /^Interface/ }).fill(String(size));
      await page.getByRole("button", { name: "Reference", exact: true }).click();
      const input = page.getByRole("textbox", { name: "Search Reference" });
      await input.fill("first");
      await input.press("Enter");
      await page.getByRole("button", { name: "New search", exact: true }).click();
      await input.fill("second");
      await input.press("Enter");
      await input.focus();
      const controls = page.getByRole("region", { name: "Reference search controls" });
      expect(await controls.evaluate((el) => getComputedStyle(el).overflowY)).toBe("visible");
      expect(
        await controls.evaluate((el) => el.scrollHeight - el.clientHeight),
      ).toBeLessThanOrEqual(1);
      await assertContained(page);
      await assertPageDoesNotScrollSideways(page);
      const close = page.getByRole("button", { name: "Close all searches", exact: true });
      await expect(close).toBeVisible();
      await expect(page.getByRole("button", { name: "Search", exact: true })).toHaveAccessibleName(
        "Search",
      );
      await page.mouse.move(page.viewportSize()!.width / 2, 12);
      await page.locator(".search-field").screenshot({
        path: info.outputPath(`search-${theme}-${size}.png`),
        animations: "disabled",
      });
    });
  }
}

for (const theme of ["dark", "light"]) {
  for (const size of [12, 24]) {
    test(`editorial pages remain readable in ${theme} at ${size}px`, async ({ page }, testInfo) => {
      await page.goto(`/?qa=reference&fixture=parts&theme=${theme}`);
      await openSettings(page);
      await page.getByRole("slider", { name: /^Interface/ }).fill(String(size));
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      await expect
        .poll(() => page.evaluate(() => getComputedStyle(document.documentElement).fontSize))
        .toBe(`${size}px`);
      await page.getByRole("button", { name: "Reference", exact: true }).click();
      const input = page.getByRole("textbox", { name: "Search Reference" });
      await input.fill("first");
      await input.press("Enter");
      const collection = page.getByRole("list", { name: "Reference results" });
      await expect(collection).toBeVisible();
      for (const expanded of [false, true]) {
        if (expanded) {
          const opener = collection.locator(".disclose").first();
          await opener.focus();
          await opener.press("Enter");
        }
        // A contained but near-zero-height list is still unusable.
        expect(await collection.evaluate((el) => el.clientHeight)).toBeGreaterThanOrEqual(80);
        await assertContained(page);
        await assertPageDoesNotScrollSideways(page);
        const capture = testInfo.outputPath(`reference-${expanded ? "open" : "closed"}.png`);
        await page.mouse.move(page.viewportSize()!.width / 2, 12);
        await page.screenshot({ path: capture, fullPage: true });
        await testInfo.attach("Reference", { path: capture, contentType: "image/png" });
      }
      await page.goto(`/?qa=reference&fixture=glossary&theme=${theme}`);
      await openSettings(page);
      await page.getByRole("slider", { name: /^Interface/ }).fill(String(size));
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      await expect
        .poll(() => page.evaluate(() => getComputedStyle(document.documentElement).fontSize))
        .toBe(`${size}px`);
      await page.getByRole("button", { name: "Glossary", exact: true }).click();
      await expect(page.getByRole("navigation", { name: "Glossary terms" })).toBeVisible();
      for (const expanded of [false, true]) {
        if (expanded) {
          const opener = page.locator(".term > .disclose").first();
          await opener.focus();
          await opener.press("Enter");
        }
        await assertContained(page);
        await assertPageDoesNotScrollSideways(page);
        const capture = testInfo.outputPath(`glossary-${expanded ? "open" : "closed"}.png`);
        await page.mouse.move(page.viewportSize()!.width / 2, 12);
        await page.screenshot({ path: capture, fullPage: true });
        await testInfo.attach("Glossary", { path: capture, contentType: "image/png" });
      }
    });
  }
}
