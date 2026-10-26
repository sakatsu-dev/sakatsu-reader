import { expect, type Page } from "@playwright/test";

/**
 * One overflow check for every page, replacing the per-spec selector lists.
 *
 * **The lists were the bug.** Each spec used to name the elements it expected to be at risk —
 * `["html", "body", "main", ".actions", ...]` — which covers only what somebody already thought
 * of. A licence string overflowed its card on the About page and broke nothing, because `.card`
 * and `.head` were on nobody's list. A net woven from guesses catches what you already knew.
 *
 * This walks everything inside the content region instead, and decides what to skip from the
 * stylesheet rather than from a list.
 */

/**
 * Where a page's own content lives.
 *
 * `<main>` rather than `<body>`, because `Sidebar.svelte` is a sibling of it: the rail is a fixed
 * strip whose own geometry is not what these checks are about, and
 * {@link assertPageDoesNotScrollSideways} covers the window itself.
 */
const CONTENT = "main";

/**
 * Tooltips are overlays, not layout, and measuring with them is measuring the wrong thing.
 *
 * `theme.css` draws every `data-tip` as an absolutely positioned `::after` with `width:
 * max-content` — invisible at `opacity: 0` until hovered, but still part of its ancestors'
 * scrollable overflow. That inflated `scrollWidth` on every control carrying one and on every
 * container above it, which reported three "overflows" on all six pages that are nothing at all.
 *
 * Suppressed for the measurement and restored immediately — they are invisible either way.
 *
 * **This is not a new discovery.** `traps.md` already records tooltips putting a scrollbar
 * under the settings page, and prescribes this exact snippet as the way to find it by hand:
 * insert the rule, wait two frames, read `scrollWidth`. The only thing added here is that
 * a recipe someone has to remember is now a check that runs.
 */
const WITHOUT_TOOLTIPS = "[data-tip]::after, [data-tip]::before { content: none !important; }";

/**
 * Every element inside `root` whose content is wider than the box holding it.
 *
 * An `overflow-x` of `auto`, `scroll` or `hidden` is the stylesheet saying "content may exceed
 * this and that is handled" — a code block, a wide table, a scroller. Those are excluded by what
 * the CSS declares, so a new one needs no change here. Anything else wider than its own client
 * box is content escaping a container that never agreed to it.
 *
 * The one-pixel tolerance is subpixel rounding, and matches the per-page checks this replaced.
 */
export async function escapedContent(page: Page, root: string = CONTENT) {
  return page.evaluate(
    async ({ selector, hideTips }) => {
      const style = document.createElement("style");
      style.textContent = hideTips;
      document.head.append(style);
      // Two frames before measuring. `traps.md` records that one is not always enough
      // for layout to settle after a style insert, and a check that measures too early
      // reports the overflow it was inserted to remove.
      await new Promise((settled) =>
        requestAnimationFrame(() => requestAnimationFrame(() => settled(null))),
      );
      try {
        const scope = [...document.querySelectorAll<HTMLElement>(selector)];
        const within = scope.flatMap((element) => [
          element,
          ...element.querySelectorAll<HTMLElement>("*"),
        ]);
        return within
          .filter((element) => {
            // A text field scrolls its own value by design — a path longer than the box
            // is the field working, not content escaping it. Same standing as an
            // explicit `overflow-x: auto`, and browsers clip these regardless.
            if (["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName)) return false;
            const overflow = getComputedStyle(element).overflowX;
            if (overflow === "auto" || overflow === "scroll" || overflow === "hidden") return false;
            return element.scrollWidth > element.clientWidth + 1;
          })
          .map((element) => {
            const classes = typeof element.className === "string" ? element.className : "";
            // Svelte's scoping hash is noise in a failure message and changes on every edit.
            const name = classes
              .replace(/\bs-[A-Za-z0-9-]+\b/g, "")
              .trim()
              .replace(/\s+/g, ".");
            const tag = element.tagName.toLowerCase();
            // The text as well as the selector: a page has many `.row`s and the failure is
            // useless without knowing which one.
            const text = (element.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 60);
            const where = `${tag}${name ? "." + name : ""}`;
            const size = `${element.scrollWidth} > ${element.clientWidth}`;
            return text ? `${where}: ${size} — "${text}"` : `${where}: ${size}`;
          });
      } finally {
        style.remove();
      }
    },
    { selector: root, hideTips: WITHOUT_TOOLTIPS },
  );
}

/** Assert nothing inside the content region escapes its container. */
export async function assertContained(page: Page, root: string = CONTENT) {
  expect(await escapedContent(page, root)).toEqual([]);
}

/**
 * The window itself must never scroll sideways, whatever the page holds.
 *
 * Separate from the scan above because `<html>` and `<body>` sit outside the content region, and
 * because this is the failure a reader actually feels.
 */
export async function assertPageDoesNotScrollSideways(page: Page) {
  const overflowing = await page.evaluate(() =>
    ["html", "body"]
      .map((selector) => document.querySelector<HTMLElement>(selector))
      .filter((element): element is HTMLElement => element !== null)
      .filter((element) => element.scrollWidth > element.clientWidth + 1)
      .map(
        (element) =>
          `${element.tagName.toLowerCase()}: ${element.scrollWidth} > ${element.clientWidth}`,
      ),
  );
  expect(overflowing).toEqual([]);
}
