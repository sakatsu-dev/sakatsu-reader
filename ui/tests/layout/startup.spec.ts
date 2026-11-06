import { test, expect } from "./fixtures";
import type { Page } from "@playwright/test";
import { assertContained } from "./contained";
import { openSettings } from "./menu";
const startup = (page: Page) => page.getByRole("region", { name: "Starting Sakatsu Reader" });
async function finish(page: Page, role?: string) {
  await page.evaluate(
    (name) =>
      (
        window as unknown as { __SAKATSU_QA__: { finishStartup: (role?: string) => void } }
      ).__SAKATSU_QA__.finishStartup(name),
    role,
  );
}

test("intro reports completed stages and leaves only when initialization finishes", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?qa=analysis&startup=loading");
  await expect(startup(page)).toBeVisible();
  await expect(page.getByRole("progressbar", { name: "Startup checks" })).toHaveAttribute(
    "aria-valuenow",
    "2",
  );
  await expect(startup(page).getByText("Starting the parser")).toBeVisible();
  await finish(page, "analysis");
  await expect(page.getByRole("progressbar", { name: "Startup checks" })).toHaveAttribute(
    "aria-valuenow",
    "3",
  );
  await expect(startup(page)).toBeVisible();
  await finish(page, "recognition");
  await expect(startup(page)).toHaveCount(0);
  await expect(page.getByPlaceholder("Paste Japanese text")).toBeVisible();
  expect(errors).toEqual([]);
});

test("continue is keyboard accessible even with reader keyboard navigation disabled", async ({
  page,
}) => {
  await page.goto("/?qa=analysis&startup=loading");
  const button = startup(page).getByRole("button", { name: "Continue to reader" });
  await expect(button).toBeVisible();
  await button.focus();
  await page.keyboard.press("Enter");
  await expect(startup(page)).toHaveCount(0);
  await finish(page);
  await expect(startup(page)).toHaveCount(0);
});

test("window navigation can leave the intro while engines are still loading", async ({ page }) => {
  await page.goto("/?qa=analysis&startup=loading");
  await expect(startup(page)).toBeVisible();
  await openSettings(page);
  await expect(startup(page)).toHaveCount(0);
});

test("status failure offers retry and remains usable", async ({ page }) => {
  await page.goto("/?qa=analysis&startup=error");
  await expect(startup(page).getByRole("status")).toContainText("unavailable");
  await finish(page);
  await startup(page).getByRole("button", { name: "Try again" }).click();
  await expect(startup(page)).toHaveCount(0);
});

test("an OCR initialization problem is shown instead of claiming readiness", async ({ page }) => {
  await page.goto("/?qa=analysis&startup=unavailable");
  await expect(startup(page).getByText("Not loaded; capture can start it")).toBeVisible();
  await expect(startup(page).getByRole("status")).toContainText("need attention");
  await startup(page).getByRole("button", { name: "Continue to reader" }).click();
  await expect(startup(page)).toHaveCount(0);
});

test("an OCR left unloaded by choice does not hold the intro for attention", async ({ page }) => {
  // The same unloaded engine as the test above, which stops for attention; here it was a choice.
  await page.goto("/?qa=analysis&startup=unavailable&ocrpreload=off");
  await expect(startup(page)).toHaveCount(0);
});

test("loading stroke follows the logo outline without rotating it", async ({ page }, info) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/?qa=analysis&startup=loading&motion=on");
  const trace = page.locator(".loading-trace");
  await expect.poll(() => trace.evaluate((el) => el.getAnimations().length)).toBe(1);
  const geometry = await trace.evaluate((el) => {
    const rect = el as SVGRectElement;
    const outline = rect.ownerSVGElement!;
    const logo = outline.parentElement!.querySelector("img")!;
    const bounds = outline.getBoundingClientRect();
    const mark = logo.getBoundingClientRect();
    const animation = rect.getAnimations()[0];
    animation.pause();
    animation.currentTime = 0;
    const before = getComputedStyle(rect).strokeDashoffset;
    animation.currentTime = Number(animation.effect!.getTiming().duration) / 4;
    return {
      width: bounds.width,
      height: bounds.height,
      centerX: bounds.x + bounds.width / 2 - (mark.x + mark.width / 2),
      centerY: bounds.y + bounds.height / 2 - (mark.y + mark.height / 2),
      gap: (bounds.width - mark.width) / 2,
      cornerRatio: rect.rx.baseVal.value / rect.width.baseVal.value,
      before: parseFloat(before),
      after: parseFloat(getComputedStyle(rect).strokeDashoffset),
      outlineTransform: getComputedStyle(outline).transform,
      logoTransform: getComputedStyle(logo).transform,
    };
  });
  expect(geometry.width).toBeCloseTo(geometry.height, 2);
  expect(geometry.centerX).toBeCloseTo(0, 2);
  expect(geometry.centerY).toBeCloseTo(0, 2);
  expect(geometry.gap).toBeGreaterThan(0);
  expect(geometry.cornerRatio).toBeCloseTo(0.18, 2);
  expect(geometry.before).toBeCloseTo(0, 2);
  expect(geometry.after).toBeCloseTo(-25, 2);
  expect(geometry.outlineTransform).toBe("none");
  expect(geometry.logoTransform).toBe("none");
  await page.screenshot({ path: info.outputPath("square-trace.png") });
});

test("motion respects reduced preference and cancels an active spinner", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/?qa=analysis&startup=loading&motion=on");
  await expect(startup(page)).toHaveAttribute("data-motion", "full");
  await expect
    .poll(() => page.locator(".loading-trace").evaluate((el) => el.getAnimations().length))
    .toBe(1);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(startup(page)).toHaveAttribute("data-motion", "reduced");
  await expect
    .poll(() => page.locator(".loading-trace").evaluate((el) => el.getAnimations().length))
    .toBe(0);
});

test("the app's animations-off setting also leaves a static outline", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/?qa=analysis&startup=loading");
  await expect(startup(page)).toHaveAttribute("data-motion", "reduced");
  await expect
    .poll(() => page.locator(".loading-trace").evaluate((el) => el.getAnimations().length))
    .toBe(0);
});

test("intro motion follows the interface speed preference", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/?qa=analysis&startup=loading&motion=on&intro-speed=300");
  await expect(startup(page)).toHaveAttribute("data-motion", "full");
  await expect
    .poll(() =>
      page
        .locator(".loading-trace")
        .evaluate((el) => Number(el.getAnimations()[0]?.effect?.getTiming().duration ?? 0)),
    )
    .toBeCloseTo(1800, 2);
  const durations = await startup(page)
    .getByRole("button", { name: "Continue to reader" })
    .evaluate((button) => {
      const root = button.closest(".startup")!;
      (button as HTMLElement).click();
      return root.getAnimations().map((animation) => animation.effect?.getTiming().duration);
    });
  expect(durations).toContain(300);
  await expect(startup(page)).toHaveCount(0);
});

for (const theme of ["dark", "light"])
  for (const size of [12, 24]) {
    test(`intro fits ${theme} at ${size}px`, async ({ page }, info) => {
      await page.goto(`/?qa=analysis&startup=loading&theme=${theme}&intro-size=${size}`);
      await expect(startup(page)).toBeVisible();
      await assertContained(page, ".startup");
      const horizontal = await page
        .locator(".startup")
        .evaluate((el) => el.scrollWidth - el.clientWidth);
      expect(horizontal).toBeLessThanOrEqual(1);
      await page.getByRole("button", { name: "Continue to reader" }).scrollIntoViewIfNeeded();
      await expect(page.getByRole("button", { name: "Continue to reader" })).toBeInViewport();
      await page.screenshot({ path: info.outputPath(`intro-${theme}-${size}.png`) });
    });
  }
