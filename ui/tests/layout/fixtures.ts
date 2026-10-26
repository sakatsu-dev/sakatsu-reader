import { test as base, expect } from "@playwright/test";

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
 * Segoe UI, Yu Gothic UI and Consolas are on any Windows install including the GitHub runner, so
 * both machines can reach them. The suite therefore measures the app in fonts a reader may not
 * see, which is the right trade: these tests are about **layout**, and layout is only meaningful
 * when it is reproducible.
 *
 * `!important` because `theme.css` declares the same custom properties on `:root`; equal
 * specificity is decided by order, and Vite's dev server injects the app's CSS from JavaScript,
 * so the order is not something to depend on.
 */
const PINNED = `:root {
  --font: "Segoe UI", sans-serif !important;
  --font-jp: "Yu Gothic UI", sans-serif !important;
  --font-mono: "Consolas", monospace !important;
  --font-display: "Georgia", serif !important;
  --font-jp-serif: "MS Mincho", serif !important;
  --font-kanji: "MS Mincho", serif !important;
}`;

export const test = base.extend({
  page: async ({ page }, use, testInfo) => {
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
