import type { Page } from "@playwright/test";
import { assertContained } from "./contained";
import { expect, test } from "./fixtures";
import { openSettings } from "./menu";

async function openSection(page: Page, query: string, section: string) {
  await page.goto(`/?qa=analysis${query}`);
  await openSettings(page);
  await page.locator("nav.sections").getByRole("button", { name: section, exact: true }).click();
}

/**
 * **A paused optional download keeps what it has, and says so.** It can be finished, or thrown
 * away — and throwing it away is its own button, not a side effect of starting again.
 */
test("a paused optional download offers to resume or discard what it saved", async ({ page }) => {
  await openSection(page, "&fixture=optional-partial", "General");
  const offer = page.locator(".optional-resource", { hasText: "Offline pronunciation" });
  await expect(offer).toContainText("40 MB of 90 MB saved");
  await expect(offer.getByRole("button", { name: "Resume Offline pronunciation" })).toBeVisible();
  const discard = offer.getByRole("button", {
    name: "Discard partial download of Offline pronunciation",
  });
  await expect(discard).toBeVisible();
  await assertContained(page);

  await discard.click();
  await expect(offer).not.toContainText("saved");
  await expect(offer.getByRole("button", { name: /^Get / })).toBeVisible();
});

/**
 * The required components list how much of each is on disk, and the button that throws a
 * partial away names the component it belongs to.
 */
test("a required component's partial says how much is saved and what discarding drops", async ({
  page,
}) => {
  // Text-only, so setup is unfinished and Settings is still reachable: the realistic way here.
  await openSection(page, "&setup=paused&text-only=1", "Recognition");
  await page.getByText("Downloaded resources and repair details").click();
  const row = page.locator(".installation-details li", { hasText: "manga-ocr" });
  await expect(row).toContainText("180 MB of 440 MB saved");
  await expect(
    row.getByRole("button", { name: "Discard partial download of manga-ocr" }),
  ).toBeVisible();
  await assertContained(page);
});

test("a paused Windows job refreshes the offers without inventing a byte count", async ({
  page,
}) => {
  await openSection(page, "&fixture=optional-bits", "General");
  const offer = page.locator(".optional-resource", { hasText: "Offline pronunciation" });
  await expect(offer.getByRole("button", { name: /^Get / })).toBeVisible();
  await page.evaluate(() => {
    const harness = (window as Window & { __SAKATSU_QA__?: { pauseOptionalDownload: () => void } })
      .__SAKATSU_QA__!;
    harness.pauseOptionalDownload();
  });
  await expect(offer.getByRole("button", { name: "Resume Offline pronunciation" })).toBeVisible();
  await expect(offer).toContainText("Download paused.");
  await expect(offer).not.toContainText("0 B");
  await assertContained(page);
});
