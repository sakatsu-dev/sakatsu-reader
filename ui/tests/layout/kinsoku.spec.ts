import { expect, test } from "./fixtures";

/**
 * **A closing mark never begins a line, and an opening mark never ends one** — *kinsoku shori*,
 * the line-breaking rule every Japanese typesetter applies.
 *
 * Each word in the reader is a `<button>`, an atomic inline the browser will break beside, so the
 * rule never applied on its own: in the README screenshots of 2026-10-19 a long passage sent its
 * final 。 down to a line by itself. `lineUnits` (`passage.js`) now keeps each mark with its word.
 *
 * Checked by position, at every reader size the slider offers in steps, because where a passage
 * wraps depends on the size — one size proves only the wrap points it happens to have.
 */
const CLOSING = /^[、。，．・：；？！…‥」』）］｝〕〉》】]/;
const OPENING = /[「『（［｛〔〈《【]$/;

test("closing marks stay on their word's row at every reader size", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await page.getByRole("button", { name: /^Analyze/ }).click();
  await expect(page.locator(".reader .token").first()).toBeVisible();
  expect(
    await page.locator(".reader .punctuation").count(),
    "the passage has marks to test",
  ).toBeGreaterThan(0);

  const breaches: string[] = [];
  for (const scale of ["1", "1.4", "1.8", "2.2", "2.5"]) {
    await page.evaluate((value) => {
      document.documentElement.style.setProperty("--reader-scale", value);
    }, scale);
    await page.waitForTimeout(100);
    breaches.push(
      ...(
        await page.evaluate(
          ({ closing, opening }) => {
            const close = new RegExp(closing);
            const open = new RegExp(opening);
            const pieces = [...document.querySelectorAll(".reader .line")].flatMap((line) => [
              ...line.querySelectorAll(".token, .punctuation"),
            ]);
            const row = (el: Element) => {
              const rects = el.getClientRects();
              return rects[rects.length - 1];
            };
            const out: string[] = [];
            pieces.forEach((piece, index) => {
              if (!piece.classList.contains("punctuation")) return;
              const text = piece.textContent ?? "";
              const here = row(piece);
              if (close.test(text) && index > 0) {
                const before = row(pieces[index - 1]);
                // Same row: the boxes overlap vertically. A full stop sits higher than the word.
                if (!(here.top < before.bottom && here.bottom > before.top))
                  out.push(`${text} began a line`);
              }
              if (open.test(text) && index + 1 < pieces.length) {
                const after = pieces[index + 1].getClientRects()[0];
                if (!(here.top < after.bottom && here.bottom > after.top))
                  out.push(`${text} ended a line`);
              }
            });
            return out;
          },
          { closing: CLOSING.source, opening: OPENING.source },
        )
      ).map((breach) => `scale ${scale}: ${breach}`),
    );
  }
  expect(breaches).toEqual([]);
});
