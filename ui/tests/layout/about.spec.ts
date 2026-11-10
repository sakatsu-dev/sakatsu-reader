import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { assertContained } from "./contained";

/**
 * The About page: an identity card (name, purpose, version, author, links), then the credits
 * folded away.
 *
 * **The overflow scan is deliberately not a list of selectors.** It walks every element inside the
 * page and skips only the boxes that were asked to scroll, because the licence that once ran out
 * over its neighbour was on nobody's list. It runs with every fold open, so the credits are in it.
 *
 * Rooted at `.page-body` rather than `body`: the side rail's `data-tip` labels are positioned wider
 * than its buttons and inflate `scrollWidth` without being drawn over anything.
 */

type Qa = {
  commands: string[];
  failNext: (command: string, times?: number) => void;
  answerNext: (command: string, value: unknown) => void;
  holdNext: (command: string) => void;
  releaseCommand: (command: string) => boolean;
  creatorLinks: unknown[];
};

/** Run something against the QA harness in the page. */
async function qa<T>(page: Page, act: (harness: Qa) => T): Promise<T> {
  return page.evaluate((source) => {
    const harness = (window as unknown as { __SAKATSU_QA__: Qa }).__SAKATSU_QA__;
    return new Function("harness", `return (${source})(harness)`)(harness);
  }, act.toString());
}

/** The app up, with `arm` run against the harness before About is opened. */
async function about(page: Page, arm?: (harness: Qa) => void, query = "") {
  await page.goto(`/?qa=analysis${query}`);
  await expect(page.getByRole("button", { name: "About" })).toBeVisible();
  if (arm) await qa(page, arm);
  await page.getByRole("button", { name: "About" }).click();
  await expect(page.locator(".paper")).toBeVisible();
}

const fold = (page: Page, name: string) =>
  page.locator("details.fold", { has: page.locator("summary", { hasText: name }) });

async function openAll(page: Page) {
  for (const name of [
    "Software and references",
    "Dictionaries, models and data",
    "Installed dictionaries",
  ]) {
    const details = fold(page, name);
    if (!(await details.evaluate((element) => (element as HTMLDetailsElement).open))) {
      await details.locator("summary").click();
    }
    await expect(details).toHaveJSProperty("open", true);
  }
}

test("the identity card opens the page: name, purpose, version and contributor, with no placeholders", async ({
  page,
}) => {
  await about(page);
  await expect(page.getByRole("heading", { name: "About", level: 2 })).toBeVisible();
  await expect(page.locator(".identity")).toContainText("Sakatsu Reader");
  await expect(page.locator(".standfirst")).toHaveText(
    "Capture Japanese text, look up words, and study while you read.",
  );
  // The facts are a description list: each value is named by the term before it.
  await expect(page.locator(".facts")).toMatchAriaSnapshot(`
    - term: Version
    - definition:
      - text: 1.0.0
      - button "Check for updates"
    - term: Contributor
    - definition: sakatsu-dev
  `);
  await expect(page.getByText("To be written")).toHaveCount(0);
  await expect(page.getByText("Links to come")).toHaveCount(0);

  // The icon tile starts level with the title, not centred against the wrapped purpose below it.
  const tops = await page.evaluate(() =>
    [".tile", ".title"].map(
      (selector) => document.querySelector(selector)!.getBoundingClientRect().top,
    ),
  );
  expect(Math.abs(tops[0] - tops[1]), "the tile's top meets the title's").toBeLessThanOrEqual(1);

  // Inside the card, top to bottom: identity, facts, the creator's links.
  const order = await page.evaluate(() =>
    [".identity", ".facts", ".creator"].map(
      (selector) => document.querySelector(selector)!.getBoundingClientRect().top,
    ),
  );
  expect([...order].sort((a, b) => a - b)).toEqual(order);

  // The card is read before the credits whatever the layout; where it sits depends on the room.
  // From 46rem it is a sidebar on the right with the credits beside it, both from the top; below
  // that it stacks above them.
  const layout = await page.evaluate(() => {
    const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
    const card = document.querySelector(".card")!;
    const credits = document.querySelector(".credits")!;
    const a = card.getBoundingClientRect();
    const b = credits.getBoundingClientRect();
    return {
      wide: document.querySelector(".paper")!.getBoundingClientRect().width / rem >= 46,
      cardFirst: !!(card.compareDocumentPosition(credits) & Node.DOCUMENT_POSITION_FOLLOWING),
      beside: a.left >= b.right - 1 && Math.abs(a.top - b.top) <= 1,
      above: a.bottom <= b.top + 1,
    };
  });
  expect(layout.cardFirst, "the card comes first in the source").toBe(true);
  if (layout.wide) expect(layout.beside, "a sidebar to the right of the credits").toBe(true);
  else expect(layout.above, "stacked above the credits").toBe(true);
});

test("the creator links go to exactly their pages, safely, and are named by their labels", async ({
  page,
}) => {
  await about(page);
  const github = page.getByRole("link", { name: "GitHub", exact: true });
  const kofi = page.getByRole("link", { name: "Support on Ko-fi", exact: true });
  await expect(github).toHaveAttribute("href", "https://github.com/sakatsu-dev/sakatsu-reader");
  await expect(kofi).toHaveAttribute("href", "https://ko-fi.com/sevennightmares");
  for (const link of [github, kofi]) {
    await expect(link).toHaveAttribute("rel", "noreferrer noopener");
    await expect(link).toHaveAttribute("target", "_blank");
    // The mark is decoration; the label is the name.
    for (const image of await link.locator("img").all()) {
      await expect(image).toHaveAttribute("alt", "");
    }
  }
});

test("following a creator link asks the shell by name and leaves the page where it was", async ({
  page,
}) => {
  await about(page);
  const url = page.url();
  await page.getByRole("link", { name: "GitHub", exact: true }).click();
  // Enter on a focused link, and a middle click: every way of following one.
  await page.getByRole("link", { name: "Support on Ko-fi", exact: true }).focus();
  await page.keyboard.press("Enter");
  await page.getByRole("link", { name: "GitHub", exact: true }).click({ button: "middle" });
  await expect
    .poll(() => qa(page, (harness) => harness.creatorLinks))
    .toEqual(["github", "kofi", "github"]);

  expect(page.url(), "the webview did not navigate").toBe(url);
  expect(page.context().pages(), "and no second window opened").toHaveLength(1);
  await expect(page.locator(".paper")).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a browser that cannot be opened is said beside the links", async ({ page }) => {
  await about(page, (harness) => harness.failNext("open_creator_link"));
  await page.getByRole("link", { name: "Support on Ko-fi", exact: true }).click();
  const alert = page.locator(".creator").getByRole("alert");
  await expect(alert).toContainText("The browser could not be opened.");
  await expect(alert).toContainText("open_creator_link failed in this fixture");
});

/**
 * **From the keyboard, under the setting a keyboard reader uses.** By default a click's focus is
 * released on purpose (`onWindowClick` in `App.svelte`, `local documentation`), and Enter on
 * a summary is a click. Tab navigation keeps focus where it lands, so that is where Enter and
 * Space must work.
 */
test("the three credit folds start shut and open from the keyboard", async ({ page }) => {
  await about(page, undefined, "&keyboard=tab");
  const folds = page.locator("details.fold");
  await expect(folds).toHaveCount(3);
  for (const details of await folds.all()) {
    await expect(details).toHaveJSProperty("open", false);
  }
  await expect(page.getByRole("link", { name: "Microsoft Edge WebView2 Runtime" })).toBeHidden();

  const software = fold(page, "Software and references");
  const summary = software.locator("summary");
  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(software).toHaveJSProperty("open", true);
  await expect(summary).toBeFocused();
  await expect(page.getByRole("link", { name: "Microsoft Edge WebView2 Runtime" })).toBeVisible();
  await page.keyboard.press("Space");
  await expect(software).toHaveJSProperty("open", false);
  await expect(summary).toBeFocused();
});

/**
 * **Every attribution is still there, word for word, once the folds are open.** Credits from
 * `credits`, data from `catalog` with its per-source line, installed dictionaries with their own
 * attribution, and the notices line outside the folds.
 */
test("opening the folds shows every credit, source and attribution in full", async ({ page }) => {
  await about(page);
  await expect(page.getByRole("button", { name: "THIRD-PARTY-NOTICES.txt" })).toBeVisible();
  await openAll(page);

  // A licence belongs to the thing above it; the DOM is the only place that relationship is
  // written. Trimmed on purpose: aria matching is a subset match.
  await expect(fold(page, "Software and references").locator(".entries").first())
    .toMatchAriaSnapshot(`
    - list:
      - listitem:
        - paragraph:
          - link "TypeScript"
          - text: Apache-2.0
      - listitem:
        - paragraph:
          - link "Microsoft Edge WebView2 Runtime"
          - text: Microsoft Edge WebView2 Runtime distribution terms - redistribution permitted
  `);
  await expect(page.getByRole("link", { name: "Lindera" })).toBeVisible();
  await expect(
    page.getByText(
      "The consulted references are linked, not bundled. Their content is all rights reserved.",
    ),
  ).toBeVisible();

  const data = fold(page, "Dictionaries, models and data");
  await expect(data.getByRole("link", { name: "Translation" })).toHaveAttribute(
    "href",
    "https://huggingface.co/staka/fugumt-ja-en",
  );
  await expect(data.getByText("FuguMT — CC BY-SA 4.0", { exact: true })).toBeVisible();
  const sources = data.getByRole("list", { name: "Translation sources" });
  await expect(sources.getByRole("link", { name: "FuguMT model card" })).toBeVisible();
  await expect(sources).toContainText("CC BY-SA 4.0");
  await expect(data.getByText("Tofugu and WaniKani — CC BY-SA 4.0", { exact: true })).toBeVisible();

  const installed = fold(page, "Installed dictionaries");
  await expect(installed.getByRole("link", { name: "Jitendex.org" })).toHaveAttribute(
    "href",
    "https://jitendex.org/",
  );
  await expect(installed).toContainText("2026-07-09");
  await expect(installed).toContainText("JMdict/EDICT");

  await assertContained(page);
});

/**
 * With `flex-shrink: 0` the licence took the width it wanted and the name gave way, so "Microsoft
 * Edge WebView2 Runtime" came out as four stacked words. Two lines is comfortable.
 */
test("a long licence drops below the name instead of crushing it", async ({ page }) => {
  await about(page);
  await openAll(page);
  const link = page.getByRole("link", { name: "Microsoft Edge WebView2 Runtime" });
  const lineHeight = await link.evaluate((element) =>
    parseFloat(getComputedStyle(element).lineHeight || "0"),
  );
  const box = await link.boundingBox();
  expect(lineHeight).toBeGreaterThan(0);
  expect(box!.height).toBeLessThanOrEqual(lineHeight * 2.5);
});

test("a source still loading says so on its heading and in its fold", async ({ page }) => {
  await about(page, (harness) => harness.holdNext("catalog"));
  const data = fold(page, "Dictionaries, models and data");
  await expect(data.locator(".fold-state")).toHaveText("Loading");
  await data.locator("summary").click();
  await expect(data.getByText("Loading…")).toBeVisible();
  await qa(page, (harness) => harness.releaseCommand("catalog"));
  await expect(data.locator(".fold-state")).toHaveText("3 entries");
  await expect(data.getByRole("link", { name: "Translation" })).toBeVisible();
});

test("a failed source is said on its shut fold, and is retried from inside it", async ({
  page,
}) => {
  await about(page, (harness) => harness.failNext("catalog"));
  const data = fold(page, "Dictionaries, models and data");
  await expect(data).toHaveJSProperty("open", false);
  await expect(data.locator(".fold-state")).toHaveText("Could not load");
  // The other sources are unaffected: one failure is not three.
  await expect(fold(page, "Software and references").locator(".fold-state")).toHaveText(
    "3 entries",
  );
  await expect(fold(page, "Installed dictionaries").locator(".fold-state")).toHaveText("1 entry");

  await data.locator("summary").click();
  await expect(data).toContainText("This list could not be loaded. catalog failed in this fixture");
  await data.getByRole("button", { name: "Try again" }).click();
  await expect(data.locator(".fold-state")).toHaveText("3 entries");
  await expect(data.getByRole("link", { name: "Translation" })).toBeVisible();
});

test("a failed credits call loses the version and the credits, and retrying restores both", async ({
  page,
}) => {
  await about(page, (harness) => harness.failNext("credits"));
  const software = fold(page, "Software and references");
  await expect(software.locator(".fold-state")).toHaveText("Could not load");
  // The version comes with the credits, so it says it could not be had rather than guessing.
  await expect(page.locator(".edition .version")).toHaveText("Unavailable");
  await software.locator("summary").click();
  await software.getByRole("button", { name: "Try again" }).click();
  await expect(page.locator(".edition .version")).toHaveText("1.0.0");
  await expect(software.getByRole("link", { name: "TypeScript" })).toBeVisible();
});

test("an empty source is told apart from a failed one", async ({ page }) => {
  await about(page, (harness) => {
    harness.answerNext("dictionaries", []);
    harness.answerNext("credits", ["1.0.0", []]);
  });
  const installed = fold(page, "Installed dictionaries");
  await expect(installed.locator(".fold-state")).toHaveText("None installed");
  await installed.locator("summary").click();
  await expect(installed.getByText("No dictionaries are installed yet.")).toBeVisible();
  await expect(installed.getByRole("button", { name: "Try again" })).toHaveCount(0);

  const software = fold(page, "Software and references");
  await expect(software.locator(".fold-state")).toHaveText("None listed");
  await software.locator("summary").click();
  await expect(software.getByText("No credits are listed.")).toBeVisible();
});

test("the notices file opens from outside the folds, and a failure to open it is said", async ({
  page,
}) => {
  await about(page, (harness) => harness.failNext("open_notices"));
  const notices = page.getByRole("button", { name: "THIRD-PARTY-NOTICES.txt" });
  await notices.click();
  await expect(page.locator(".credits").getByRole("alert")).toContainText(
    "open_notices failed in this fixture",
  );
  await notices.click();
  await expect(page.locator(".credits").getByRole("alert")).toHaveCount(0);
  expect(
    (await qa(page, (harness) => harness.commands)).filter((command) => command === "open_notices"),
  ).toHaveLength(2);
});

/** Black on light, white on dark: the GitHub mark's only two colours here, never the accent. */
for (const theme of ["dark", "light"] as const) {
  test(`the GitHub mark is the ${theme === "dark" ? "white" : "black"} one on a ${theme} theme`, async ({
    page,
  }) => {
    await about(page, undefined, theme === "light" ? "&theme=light" : "");
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const github = page.getByRole("link", { name: "GitHub", exact: true });
    await expect(github.locator(theme === "dark" ? "img.on-dark" : "img.on-light")).toBeVisible();
    await expect(github.locator(theme === "dark" ? "img.on-light" : "img.on-dark")).toBeHidden();
    const loaded = await github
      .locator("img:visible")
      .evaluate((image) => (image as HTMLImageElement).naturalWidth);
    expect(loaded, "the bundled mark loaded").toBeGreaterThan(0);
    const cup = page.getByRole("link", { name: "Support on Ko-fi" }).locator("img");
    await expect
      .poll(() => cup.evaluate((image) => (image as HTMLImageElement).naturalWidth))
      .toBeGreaterThan(0);
  });
}

/** The smallest and largest Interface size, with everything open: nothing escapes, nothing scrolls sideways. */
for (const size of [12, 24]) {
  test(`at an interface size of ${size}px, the open page stays inside itself`, async ({ page }) => {
    await about(page);
    await page.evaluate((px) => {
      document.documentElement.style.fontSize = `${px}px`;
    }, size);
    await openAll(page);
    // Inherited text follows the setting too, not only the `rem` sizes around it.
    for (const selector of [".byline", ".entry-head", ".why"]) {
      const px = await page
        .locator(selector)
        .first()
        .evaluate((element) => parseFloat(getComputedStyle(element).fontSize));
      expect(px, `${selector} at ${size}px`).toBeCloseTo(size, 0);
    }
    await assertContained(page);
    const sideways = await page
      .locator(".page-body")
      .evaluate((element) => element.scrollWidth - element.clientWidth);
    expect(sideways).toBeLessThanOrEqual(1);
  });
}

/**
 * The credits' column count follows the room the credits themselves have, whichever layout they
 * are in: beside the sidebar they are narrower than the page, so the page's width is the wrong
 * question. One column under 36rem, then two, three from 62rem and four from 88rem — a stated
 * step a test can compute, never more than four.
 */
async function expectCreditColumns(page: Page) {
  const measured = await page.evaluate(() => {
    const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
    const entries = document.querySelector(".fold[data-fold='software'] .entries")!;
    return {
      room: document.querySelector(".credits")!.getBoundingClientRect().width / rem,
      columns: getComputedStyle(entries).columnCount,
    };
  });
  const expected =
    measured.room >= 88 ? "4" : measured.room >= 62 ? "3" : measured.room >= 36 ? "2" : null;
  if (expected)
    expect(measured.columns, `${measured.room.toFixed(1)}rem of credits`).toBe(expected);
  else expect(["auto", "1"]).toContain(measured.columns);
}

test("credits take more columns as they get more room, up to four", async ({ page }) => {
  await about(page);
  await openAll(page);
  await expectCreditColumns(page);
});

/**
 * **Full screen, at the resolutions people actually run.** The page was capped at 64rem, so at
 * 1920px the card floated in the middle of the page with the right half empty. Now the card is a
 * sidebar at the page's own right edge, the credits fill everything to its left, and nothing
 * scrolls sideways or escapes — at 1080p, 1440p and a 4K-wide window at 100%.
 *
 * The page is the whole window: the Column width setting that could narrow it is gone.
 */
for (const [width, height] of [
  [1920, 1080],
  [2560, 1440],
  [3840, 2160],
] as const) {
  test(`at ${width}×${height}, the card sits at the right edge and the credits fill the rest`, async ({
    page,
  }, info) => {
    test.skip(info.project.name !== "edge-900-100", "one device scale is enough for window sizes");
    await page.setViewportSize({ width, height });
    await about(page);
    const edges = await page.evaluate(() => {
      const paper = document.querySelector(".paper")!.getBoundingClientRect();
      const page = document.querySelector(".page-body")!;
      const body = page.getBoundingClientRect();
      const card = document.querySelector(".card")!.getBoundingClientRect();
      const credits = document.querySelector(".credits")!.getBoundingClientRect();
      // The scroller's content edge: its right padding and reserved scrollbar gutter are not room.
      const style = getComputedStyle(page);
      const contentRight = body.left + page.clientWidth - parseFloat(style.paddingRight);
      return {
        paperRight: paper.right,
        contentRight,
        cardRight: card.right,
        creditsLeft: credits.left,
        paperLeft: paper.left,
        beside: card.left >= credits.right - 1 && Math.abs(card.top - credits.top) <= 1,
      };
    });
    expect(edges.beside, "the card is a sidebar beside the credits").toBe(true);
    expect(
      Math.abs(edges.paperRight - edges.contentRight),
      "the page fills its width",
    ).toBeLessThanOrEqual(1);
    expect(
      Math.abs(edges.cardRight - edges.paperRight),
      "the card meets the right edge",
    ).toBeLessThanOrEqual(1);
    expect(
      Math.abs(edges.creditsLeft - edges.paperLeft),
      "the credits start at the left edge",
    ).toBeLessThanOrEqual(1);

    await openAll(page);
    await expectCreditColumns(page);
    await assertContained(page);
    const sideways = await page
      .locator(".page-body")
      .evaluate((element) => element.scrollWidth - element.clientWidth);
    expect(sideways).toBeLessThanOrEqual(1);
  });
}
