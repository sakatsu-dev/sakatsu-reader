import type { Page } from "@playwright/test";
import { writeFile } from "node:fs/promises";

import { expect, test } from "./fixtures";
import { assertContained } from "./contained";

/**
 * The hint at the trailing end of the actions row.
 *
 * It had two states behind one boolean — "Reading…" while a capture was in flight, the binding
 * otherwise — and that was enough while every engine loaded in under a second. PaddleOCR-VL
 * ended it: the first read starts the app-managed llama.cpp server and loads its weights, and
 * up to 20, and every one of those seconds said "Reading…". Nothing said which engine was
 * reading, or that the pause was a model waking up rather than a capture going wrong.
 *
 * The four phases are unit-tested in `capture-hint.test.mjs`. What is asserted here is the part
 * only a browser can answer: that the two idle faces are really stacked and really swap, and
 * that the loading phase reaches the screen.
 */

const hint = (page: Page) => page.locator(".shortcut");

async function open(page: Page, query = "") {
  await page.goto(`/?qa=analysis${query}`);
  await expect(hint(page)).toBeVisible();
}

test("idle names the engine, in the selector's words rather than the id", async ({ page }) => {
  // `?engine=vlm` is what Rust reports. "PaddleOCR-VL" is what a person chose.
  await open(page, "&engine=vlm");
  await expect(hint(page)).toContainText("PaddleOCR-VL");
  await expect(hint(page)).not.toContainText("vlm");
  await assertContained(page);
});

/**
 * Without motion the hint states both halves at once.
 *
 * The QA settings have `animations: false`, so this is also the shape every other layout test
 * and every screenshot baseline sees — which is deliberate: a crossfade under a screenshot is a
 * flake waiting to be filed.
 */
test("with animation off the two halves become one line and nothing moves", async ({ page }) => {
  await open(page, "&engine=pp-ocrv6");
  await expect(hint(page)).toHaveText(/Middle click reads · PP-OCRv6/);
  await expect(hint(page).locator(".face")).toHaveCount(0);
});

/**
 * With motion on, the faces are stacked in one grid cell and take turns.
 *
 * The stack is what keeps the row from stepping as the text changes: the element is as wide as
 * its wider face whichever one is showing. Asserted by geometry rather than by reading the CSS,
 * because where the faces actually land is the property, not the declaration that causes it.
 *
 * They share a **right** edge. The cell is as wide as the longer face, so left-aligning them
 * would make the shorter one stop short and open a gap that closes again on every turn.
 */
test("with animation on the faces share a right edge and take turns", async ({ page }) => {
  await open(page, "&engine=vlm&motion=on");

  const faces = hint(page).locator(".face");
  await expect(faces).toHaveCount(2);
  await expect(faces.nth(0)).toHaveText("Middle click reads");
  await expect(faces.nth(1)).toHaveText("PaddleOCR-VL");

  const boxes = await faces.evaluateAll((nodes) =>
    nodes.map((node) => {
      const { right, y, width } = node.getBoundingClientRect();
      return { right: Math.round(right), y: Math.round(y), width: Math.round(width) };
    }),
  );
  expect(boxes[0].right).toBe(boxes[1].right);
  expect(boxes[0].y).toBe(boxes[1].y);
  // And they are genuinely different lengths, so the shared right edge is doing work rather
  // than being true by accident.
  expect(boxes[0].width).not.toBe(boxes[1].width);

  // Exactly one is showing at a time — the other is faded out, not removed, so it has
  // somewhere to fade back from.
  await expect(faces.locator(".off")).toHaveCount(0);
  await expect(hint(page).locator(".face.off")).toHaveCount(1);

  // And they turn over. The interval is four seconds; this waits on the swap rather than on a
  // clock, so a change to the pace does not break the test.
  const shownFirst = await hint(page).locator(".face:not(.off)").textContent();
  await expect(hint(page).locator(".face:not(.off)")).not.toHaveText(shownFirst ?? "", {
    timeout: 15_000,
  });
});

/**
 * The fade follows the motion token, at a multiple of it.
 *
 * **This is the check `tokens.md` asks for by name.** A hardcoded duration looks identical on
 * the machine it was written on and silently opts the element out of both the animation setting
 * and the reader's operating system — the ledger records four components that already did
 * exactly that.
 *
 * **It has to move the token to prove anything.** Written first as a ratio at the default
 * duration, it passed with a hardcoded `207ms` in place: at `--anim: 180ms` a literal 207 *is*
 * 1.15x, so the two are indistinguishable at the one setting the fixture happens to use. What
 * separates them is what happens when the setting changes, so that is what this does.
 *
 * **OUTSTANDING: intermittently flaky under the full parallel suite.** It failed once in a
 * `check:layout` run of 453 on 2026-09-08, and passed both in isolation and on an immediate second
 * full run with no code between them. A deliberate 2026-09-10 stress run executed all five tests
 * in this file 20 times across all three viewports with eight workers: 300/300 passed. That is
 * evidence that the failure did not reproduce under that load; it is not a diagnosis. The cause
 * remains **not diagnosed**. The plausible shape is that
 * `getComputedStyle` is read before the style recalculation for the just-set `--anim` has landed,
 * which would make it a load-sensitive race rather than a behaviour change — but that is a
 * hypothesis, not a finding.
 *
 * Recorded here rather than retried until green. A test that fails one run in several is telling
 * you something, and a suite whose flakes go unwritten is one whose next real failure gets
 * dismissed as another flake. Do not quarantine this without diagnosing it first.
 *
 * **2026-09-12: instrumented, and stressed again without reproducing.** The measurement now reports
 * the resolved token beside each duration and attaches everything at the point of measurement, so
 * the next failure arrives with the evidence to diagnose it instead of one number. Until then the
 * cause remains undiagnosed — the instrumentation is not a fix and the hypothesis is still a
 * hypothesis.
 */
test("the fade tracks the motion token rather than fixing its own duration", async ({
  page,
}, testInfo) => {
  await open(page, "&engine=vlm&motion=on");

  // **Everything the failure would need, gathered where the failure happens.**
  //
  // This used to return two strings and restore `--anim` before returning, so the retained trace
  // and screenshot showed the *healthy* state and `before` — read and then discarded — never
  // reached anyone. A failing run said "Expected 345, Received x" and nothing else.
  //
  // The decisive extra is `resolved`: the value `--anim` actually held on the root at the instant
  // each duration was read. If a failure ever shows `resolved` already at the value just set while
  // the duration still reflects the previous one, that is the stale-recalculation hypothesis
  // confirmed. If `resolved` disagrees with what was set, something else is writing the token — the
  // `$effect` in `App.svelte` is the obvious candidate, and the 4-second face-swap timer is
  // mutating classes on this very subtree throughout.
  const measured = await page.locator(".shortcut .face").first().evaluate((node) => {
    const root = document.documentElement;
    const before = root.style.getPropertyValue("--anim");
    const beforePriority = root.style.getPropertyPriority("--anim");
    const sample = () => ({
      duration: getComputedStyle(node).transitionDuration,
      resolved: getComputedStyle(root).getPropertyValue("--anim").trim(),
      inline: root.style.getPropertyValue("--anim"),
      priority: root.style.getPropertyPriority("--anim"),
      classes: node.className,
    });

    try {
      root.style.setProperty("--anim", "300ms");
      const slow = sample();
      root.style.setProperty("--anim", "0ms");
      const off = sample();
      return {
        slow,
        off,
        before,
        beforePriority,
        reducedMotion: window.matchMedia("(prefers-reduced-motion: reduce)").matches,
        transition: getComputedStyle(node).transition,
        faces: document.querySelectorAll(".shortcut .face").length,
      };
    } finally {
      // In `finally` so an exception between the writes cannot leave the page on a forced value
      // for every later test in this worker.
      root.style.setProperty("--anim", before, beforePriority);
    }
  });

  const ms = (value: string) =>
    value.endsWith("ms") ? Number.parseFloat(value) : Number.parseFloat(value) * 1000;

  // Attached unconditionally: a passing run's numbers are the baseline a failing one is read
  // against, and there is no way to collect them after the fact.
  //
  // **Written to a file rather than attached as a body.** The local reporter is `line`, which
  // prints only attachments that have a path — a body attachment exists in the run and reaches
  // neither the console nor the disk, which is no use to a campaign run locally. Writing it to
  // the test's own output directory puts the path in the failure output beside the screenshot.
  const record = testInfo.outputPath("anim-measurement.json");
  await writeFile(record, JSON.stringify({ ...measured, viewport: page.viewportSize() }, null, 2));
  await testInfo.attach("anim-measurement.json", { path: record, contentType: "application/json" });

  // 15% slower than whatever the reader asked for, not 15% slower than one remembered number.
  expect(ms(measured.slow.duration)).toBeCloseTo(345, 0);
  // And the half that matters most: off has to mean off. A multiple of zero is still zero;
  // a literal goes on fading.
  expect(ms(measured.off.duration)).toBe(0);
  // If these ever fail, the token itself was not what the test thought it was, and the duration
  // assertions above were measuring something other than what they claim.
  expect(measured.slow.resolved).toBe("300ms");
  expect(measured.off.resolved).toBe("0ms");
});

/**
 * The state this whole change exists for.
 *
 * The fixture holds `read_frame` open and reports the engine as not yet loaded, which is a cold
 * PaddleOCR-VL read. Before this, those seconds were indistinguishable from a slow read.
 */
test("a capture waiting on a cold engine says what it is waiting for", async ({ page }) => {
  await open(page, "&fixture=ocr-loading");
  await page.evaluate(() => {
    (window as unknown as { __SAKATSU_QA__: { startColdCapture: () => void } })
      .__SAKATSU_QA__.startColdCapture();
  });
  await expect(hint(page)).toHaveText(/Loading PaddleOCR-VL…/);
  await expect(hint(page)).not.toHaveText(/Reading/);
  await expect(hint(page).locator(".spinner")).toBeVisible();
  await assertContained(page);
});
