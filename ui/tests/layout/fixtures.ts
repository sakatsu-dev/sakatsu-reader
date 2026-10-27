import { test as base, expect, type Browser } from "@playwright/test";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

/**
 * The layout suite's `test`, which differs from Playwright's in one way: **the fonts are pinned.**
 *
 * # Why every test and not just the screenshots
 *
 * Font metrics decide how wide text is, which decides where it wraps, which decides the geometry
 * every check in this folder asserts. Pinning only the screenshots would leave the overflow scan
 * and the bounding-box measurements free to disagree between two machines, and disagree quietly:
 * a row that fits by four pixels here is a row that overflows somewhere else.
 *
 * That four-pixel margin is not hypothetical. `Settings.svelte`'s size rows needed 188-204px in a
 * 184px pane, and the closest of them missed by four.
 *
 * # Why these fonts
 *
 * `theme.css` asks for `Inter` and `Noto Sans JP` first, and **neither ships with Windows** — this
 * repository bundles no font files, so both are present only if somebody installed them. A
 * developer machine with Inter and a bare CI runner without it render different typefaces, not
 * merely different pixels, and that is what put three baselines permanently red: their diff is
 * every glyph on the page outlined, with no box or border moved.
 *
 * Japanese serif faces are optional Windows components. A font-family declaration does not prove
 * that the requested face rendered anything. All Japanese test roles therefore use Yu Gothic UI,
 * verified through Chromium's actual platform-font report before a worker starts its tests.
 * This changes test rendering only; the app retains its own typography.
 *
 * `!important` because `theme.css` declares the same custom properties on `:root`; equal
 * specificity is decided by order, and Vite's dev server injects the app's CSS from JavaScript,
 * so the order is not something to depend on.
 */
const PINNED = `:root {
  --font: "Segoe UI", "Yu Gothic UI", sans-serif !important;
  --font-jp: "Yu Gothic UI", sans-serif !important;
  --font-mono: "Consolas", monospace !important;
  --font-display: "Georgia", serif !important;
  --font-jp-serif: "Yu Gothic UI", sans-serif !important;
  --font-kanji: "Yu Gothic UI", sans-serif !important;
}`;

type PlatformFont = {
  familyName: string;
  postScriptName: string;
  glyphCount: number;
  isCustomFont: boolean;
};
export type FontEnvironment = {
  browserVersion: string;
  runnerImage: string;
  runnerImageVersion: string;
  fonts: Record<string, { requested: string; rendered: PlatformFont[] }>;
};

export async function probeFonts(
  browser: Browser,
  japaneseFamily?: string,
): Promise<FontEnvironment> {
  const page = await browser.newPage();
  try {
    await page.setContent("<main></main>");
    await page.addStyleTag({ content: PINNED });
    const samples = {
      japanese: {
        family: japaneseFamily ?? "Yu Gothic UI",
        css: japaneseFamily ?? "var(--font-jp)",
        text: "日本語漢字図書館",
      },
      japaneseInProse: { family: "Yu Gothic UI", css: "var(--font)", text: "日本語漢字図書館" },
      previousSerif: { family: "MS Mincho", css: '"MS Mincho"', text: "日本語漢字図書館" },
      previousProseFallback: { family: "Segoe UI", css: '"Segoe UI", sans-serif', text: "つなぎ" },
      latin: { family: "Segoe UI", css: "var(--font)", text: "Reading" },
      display: { family: "Georgia", css: "var(--font-display)", text: "Reading" },
      monospace: { family: "Consolas", css: "var(--font-mono)", text: "Reading" },
    };
    await page.evaluate((values) => {
      for (const [id, sample] of Object.entries(values)) {
        const node = document.createElement("span");
        node.id = id;
        node.style.fontSize = "16px";
        node.style.fontFamily = sample.css;
        node.textContent = sample.text;
        document.querySelector("main")!.append(node);
      }
    }, samples);
    await page.evaluate(() => document.fonts.ready);
    const session = await page.context().newCDPSession(page);
    await session.send("DOM.enable");
    await session.send("CSS.enable");
    const { root } = await session.send("DOM.getDocument");
    const fonts: FontEnvironment["fonts"] = {};
    for (const [id, sample] of Object.entries(samples)) {
      const { nodeId } = await session.send("DOM.querySelector", {
        nodeId: root.nodeId,
        selector: `#${id}`,
      });
      const result = await session.send("CSS.getPlatformFontsForNode", { nodeId });
      fonts[id] = { requested: sample.family, rendered: result.fonts };
    }
    return {
      browserVersion: browser.version(),
      runnerImage: process.env.ImageOS ?? process.platform,
      runnerImageVersion: process.env.ImageVersion ?? "local",
      fonts,
    };
  } finally {
    await page.close();
  }
}

export function requireJapaneseFont(environment: FontEnvironment) {
  for (const role of ["japanese", "japaneseInProse"]) {
    const { requested, rendered: fonts } = environment.fonts[role];
    const rendered = fonts.filter((font) => font.glyphCount > 0);
    if (
      requested !== "Yu Gothic UI" ||
      rendered.length === 0 ||
      rendered.some((font) => font.familyName !== "Yu Gothic UI")
    ) {
      throw new Error(
        `Layout requires Yu Gothic UI for ${role}; actual fonts: ${JSON.stringify(rendered)}`,
      );
    }
  }
}

export const test = base.extend<{}, { fontEnvironment: FontEnvironment }>({
  fontEnvironment: [
    async ({ browser }, use, workerInfo) => {
      const environment = await probeFonts(browser);
      await mkdir(workerInfo.project.outputDir, { recursive: true });
      const filename = `font-environment-${workerInfo.project.name}-${workerInfo.workerIndex}.json`;
      await writeFile(
        join(workerInfo.project.outputDir, filename),
        JSON.stringify(environment, null, 2),
      );
      requireJapaneseFont(environment);
      await use(environment);
    },
    { scope: "worker", auto: true },
  ],
  page: async ({ page, fontEnvironment }, use, testInfo) => {
    // Keep startup evidence when the first control never appears. A later isolated retry
    // replaces Playwright's output, so the failed run must be preserved before retrying.
    const startup: string[] = [];
    const record = (message: string) => {
      if (startup.length < 100) startup.push(message);
    };
    page.on("pageerror", (error) => record(`page error: ${error.stack ?? error.message}`));
    page.on("console", (message) => {
      if (message.type() === "error") record(`console error: ${message.text()}`);
    });
    page.on("requestfailed", (request) =>
      record(`request failed: ${request.url()} ${request.failure()?.errorText ?? ""}`),
    );
    // `addInitScript` rather than `addStyleTag`, so it survives every navigation a test makes
    // instead of having to be reapplied after each `goto`.
    await page.addInitScript((css: string) => {
      const install = () => {
        const style = document.createElement("style");
        style.id = "pinned-fonts";
        style.textContent = css;
        document.head.append(style);
      };
      if (document.head) install();
      else document.addEventListener("DOMContentLoaded", install, { once: true });
    }, PINNED);
    await use(page);
    if (testInfo.status !== testInfo.expectedStatus) {
      await testInfo.attach("font-environment", {
        body: JSON.stringify(fontEnvironment, null, 2),
        contentType: "application/json",
      });
      await testInfo.attach("startup-diagnostics", {
        body: `URL: ${page.url()}\n${startup.join("\n")}`,
        contentType: "text/plain",
      });
      if (!page.isClosed()) {
        try {
          await testInfo.attach("page-at-failure", {
            body: await page.content(),
            contentType: "text/html",
          });
        } catch {
          // A closed or crashed renderer must not replace the original failure.
        }
      }
    }
  },
});

export { expect };
