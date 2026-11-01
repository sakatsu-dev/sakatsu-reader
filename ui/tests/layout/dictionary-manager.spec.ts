import { expect, test } from "./fixtures";
import { openSettings } from "./menu";
import type { Page } from "@playwright/test";

async function settings(page: Page, state = "ready") {
  await page.goto(`/?qa=analysis&manager=${state}`);
  await openSettings(page);
  await page.getByRole("button", { name: "Search", exact: true }).click();
}
const panel = (page: Page) => page.locator(".dictionary-manager");

test("import controls describe dictionary files without application-specific branding", async ({
  page,
}) => {
  await settings(page);
  await expect(panel(page).locator(".drop-area")).toContainText(
    "Drop a dictionary ZIP or KANJIDIC2 .xml.gz here.",
  );
  await expect(panel(page).locator(".drop-area")).not.toContainText("Yomitan");
  await expect(panel(page).getByRole("button", { name: "Choose dictionary file" })).toBeVisible();
});

test("publisher updates show progress and the new revision", async ({ page }) => {
  await settings(page);
  await expect(panel(page).getByText("Example dictionary [2026-10-03]")).toBeVisible();
  await panel(page).getByRole("button", { name: "Allow publisher checks" }).click();
  await panel(page)
    .getByRole("button", { name: "Check for dictionary updates", exact: true })
    .click();
  await panel(page).getByRole("button", { name: "Update to 2026.10.18.0" }).click();
  await expect(panel(page).getByText("Up to date.", { exact: true })).toBeVisible();
  await expect(panel(page).getByText("2026.10.18.0 · 42 entries")).toBeVisible();
});

test("failed updates retain the installed revision", async ({ page }) => {
  await settings(page, "update-fails");
  await panel(page)
    .getByRole("button", { name: "Check for dictionary updates", exact: true })
    .click();
  await panel(page).getByRole("button", { name: "Update to 2026.10.18.0" }).click();
  await expect(panel(page).getByText(/The publisher archive was invalid/)).toBeVisible();
  await expect(panel(page).getByText("2026.10.03.0 · 42 entries")).toBeVisible();
});

test("empty and failed reads are distinct", async ({ page }) => {
  await settings(page, "empty");
  await expect(panel(page).getByText(/No dictionary is installed yet/)).toBeVisible();
  await settings(page, "unreadable");
  await expect(panel(page).getByRole("alert")).toContainText("could not be read");
  await expect(panel(page).getByText(/No dictionary is installed yet/)).toHaveCount(0);
});

test("a keyboard-accessible file chooser previews add and replacement targets", async ({
  page,
}) => {
  await settings(page);
  await page.evaluate(() => {
    (
      window as unknown as {
        __SAKATSU_QA__: { answerNext: (command: string, value: unknown) => void };
      }
    ).__SAKATSU_QA__.answerNext("plugin:dialog|open", "fixture.zip");
  });
  const choose = panel(page).getByRole("button", { name: "Choose dictionary file" });
  await choose.focus();
  await page.keyboard.press("Enter");
  await expect(panel(page).getByText("Imported dictionary", { exact: true })).toBeVisible();
  await expect(panel(page).getByRole("combobox")).toHaveValue("");
  await panel(page).getByRole("combobox").selectOption("fixture");
  await panel(page).getByRole("button", { name: "Install dictionary", exact: true }).click();
  await expect(panel(page).getByText("Dictionary ready.", { exact: true })).toBeVisible();
});

test("update preferences persist across Settings visits", async ({ page }) => {
  await settings(page);
  const automatic = panel(page).getByRole("checkbox", {
    name: "Download and install updates automatically",
  });
  await automatic.check();
  await expect(automatic).toBeChecked();
  await page.getByRole("button", { name: "General", exact: true }).click();
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(
    panel(page).getByRole("checkbox", { name: "Download and install updates automatically" }),
  ).toBeChecked();
});

test("a native drop opens the same import preview", async ({ page }) => {
  await settings(page);
  const area = panel(page).locator(".drop-area");
  await area.scrollIntoViewIfNeeded();
  const rect = await area.boundingBox();
  expect(rect).not.toBeNull();
  await page.evaluate((rect) => {
    (
      window as unknown as {
        __SAKATSU_QA__: {
          emitDictionaryDrop: (paths: string[], position: { x: number; y: number }) => void;
        };
      }
    ).__SAKATSU_QA__.emitDictionaryDrop(["fixture.zip"], {
      x: (rect!.x + 10) * devicePixelRatio,
      y: (rect!.y + 10) * devicePixelRatio,
    });
  }, rect);
  await expect(panel(page).getByText("Imported dictionary", { exact: true })).toBeVisible();
  await panel(page).getByRole("button", { name: "Cancel import" }).click();
  await expect(panel(page).getByText("Imported dictionary", { exact: true })).toHaveCount(0);
});

test("a held older status cannot undo a newer preference", async ({ page }) => {
  await settings(page);
  await page.evaluate(() =>
    (
      window as unknown as { __SAKATSU_QA__: { holdNext: (command: string) => void } }
    ).__SAKATSU_QA__.holdNext("dictionary_manager"),
  );
  const automatic = panel(page).getByRole("checkbox", {
    name: "Download and install updates automatically",
  });
  await automatic.check();
  await automatic.uncheck();
  await expect(automatic).not.toBeChecked();
  const released = await page.evaluate(() =>
    (
      window as unknown as { __SAKATSU_QA__: { releaseCommand: (command: string) => boolean } }
    ).__SAKATSU_QA__.releaseCommand("dictionary_manager"),
  );
  expect(released).toBe(true);
  await page.waitForTimeout(100);
  await expect(automatic).not.toBeChecked();
});

for (const theme of ["dark", "light"]) {
  for (const px of [12, 24]) {
    test(`dictionary controls wrap at ${px}px in ${theme}`, async ({ page }, testInfo) => {
      await settings(page, `ready&theme=${theme}`);
      await page.evaluate((px) => (document.documentElement.style.fontSize = `${px}px`), px);
      await expect(
        panel(page).getByRole("button", { name: "Choose dictionary file" }),
      ).toBeVisible();
      const overflow = await panel(page).evaluate(
        (element) => element.scrollWidth > element.clientWidth + 1,
      );
      expect(overflow).toBe(false);
      await page.screenshot({ path: testInfo.outputPath("dictionary-manager-top.png") });
      await panel(page)
        .getByRole("button", { name: "Choose dictionary file" })
        .scrollIntoViewIfNeeded();
      await page.screenshot({ path: testInfo.outputPath("dictionary-manager-import.png") });
    });
  }
}
