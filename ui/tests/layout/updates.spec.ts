import { expect, test } from "./fixtures";
import { assertContained } from "./contained";
import { openSettings } from "./menu";

async function openUpdates(page: import("@playwright/test").Page, state: string) {
  await page.goto(`/?qa=analysis&update=${state}`);
  await openSettings(page);
  await page.locator("nav.sections").getByRole("button", { name: "Updates", exact: true }).click();
}

test("unconfigured builds explain why updates cannot be checked", async ({ page }) => {
  await openUpdates(page, "not_configured");
  await expect(page.getByText("Updates are not configured in this build.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Check for updates", exact: true })).toBeDisabled();
  await assertContained(page);
});

test("checking, offline and current are distinct states", async ({ page }) => {
  for (const [state, text] of [
    ["checking", "Checking for updates…"],
    ["offline", "The release service could not be reached."],
    ["current", "You have the latest version."],
  ]) {
    await openUpdates(page, state);
    await expect(page.getByText(text, { exact: true })).toBeVisible();
  }
});

test("download and installation require separate choices", async ({ page }) => {
  await openUpdates(page, "available");
  await expect(page.getByRole("button", { name: "Restart and install" })).toHaveCount(0);
  await page.getByRole("button", { name: "Download update", exact: true }).click();
  await expect(page.getByRole("button", { name: "Restart and install" })).toBeVisible();
  await page.getByRole("button", { name: "Restart and install" }).click();
  await expect(page.getByRole("alert")).toContainText("Installation refused");
  await expect(page.getByRole("heading", { name: "Settings", exact: true })).toBeVisible();
});

test("unknown download size shows bytes without an invented total", async ({ page }) => {
  await openUpdates(page, "downloading");
  await expect(page.getByText(/^Downloaded /)).not.toContainText(" of ");
  await assertContained(page);
});

test("a late event cannot replace a newer ready update", async ({ page }) => {
  await openUpdates(page, "available");
  await page.getByRole("button", { name: "Download update", exact: true }).click();
  await expect(page.getByRole("button", { name: "Restart and install" })).toBeVisible();
  await page.evaluate(() => {
    const qa = (window as unknown as { __SAKATSU_QA__: { emitUpdate: (value: unknown) => void } })
      .__SAKATSU_QA__;
    qa.emitUpdate({
      generation: 1,
      phase: "current",
      version: null,
      notes: "",
      downloaded: 0,
      total: null,
      error: null,
    });
  });
  await expect(page.getByRole("button", { name: "Restart and install" })).toBeVisible();
  await expect(page.getByText("You have the latest version.")).toHaveCount(0);
});

for (const theme of ["dark", "light"]) {
  test(`updates remain readable at size extremes in ${theme}`, async ({ page }, info) => {
    await openUpdates(page, `ready&theme=${theme}`);
    for (const size of [12, 24]) {
      await page.evaluate((px) => {
        document.documentElement.style.fontSize = `${px}px`;
      }, size);
      await expect(page.getByRole("button", { name: "Restart and install" })).toBeVisible();
      await assertContained(page);
      await page.getByRole("button", { name: "Restart and install" }).scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath(`updates-${theme}-${size}.png`) });
    }
  });
}
