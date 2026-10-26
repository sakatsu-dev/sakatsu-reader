import type { Page } from "@playwright/test";
import { assertContained } from "./contained";
import { expect, test } from "./fixtures";
import { openSettings } from "./menu";

type Harness = {
  commands: string[];
  copied: string[];
  failNext: (command: string, times?: number) => void;
};

const qa = <T>(page: Page, read: (harness: Harness) => T) =>
  page.evaluate((source) => {
    const harness = (window as Window & { __SAKATSU_QA__?: Harness }).__SAKATSU_QA__!;
    return new Function("harness", `return (${source})(harness)`)(harness);
  }, read.toString()) as Promise<T>;

async function openDiagnostics(page: Page, query = "") {
  await page.goto(`/?qa=analysis${query}`);
  await openSettings(page);
  await page
    .locator("nav.sections")
    .getByRole("button", { name: "Diagnostics", exact: true })
    .click();
}

/**
 * **What is running, as reported.** One line per job saying what the settings ask for, what is
 * loaded, where it runs and on what — and the last result of each kind, or that there has been
 * none. Nothing is drawn for a device nobody reported.
 */
test("Diagnostics names each engine and the last result of each kind", async ({ page }) => {
  await openDiagnostics(page);
  const list = page.locator("dl.engines");
  await expect(list).toBeVisible();
  await expect(list.locator("dt")).toHaveText([
    "Recognition",
    "Analysis",
    "Translation",
    "Last recognition",
    "Last analysis",
    "Last translation",
  ]);
  await expect(list.locator("dd").nth(0)).toContainText("Auto ready · in the app · CPU");
  await expect(list.locator("dd").nth(1)).toHaveText(
    "Automatic ready, using Lindera · in the app · CPU",
  );
  await expect(list.locator("dd").nth(2)).toHaveText("FuguMT not loaded yet · in the app");
  await expect(list.locator("dd").nth(3)).toHaveText("none this session");
  await expect(list.locator("dd").nth(4)).toHaveText("Lindera · CPU");
  await assertContained(page);
});

test("a status that cannot be read says so, rather than showing nothing", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openSettings(page);
  // Armed once the app is up, and before the section that reads the status opens.
  await qa(page, (harness) => harness.failNext("runtime_status"));
  await page
    .locator("nav.sections")
    .getByRole("button", { name: "Diagnostics", exact: true })
    .click();
  await expect(page.getByRole("alert").filter({ hasText: "could not be read" })).toBeVisible();
  await expect(page.locator("dl.engines")).toHaveCount(0);
});

/**
 * **Previewed, then copied — and copying is only copying.** The text that reaches the clipboard is
 * exactly the previewed text, it goes through `copy_text` (so the clipboard watcher ignores it),
 * and nothing is analysed as a result.
 */
test("the bug report is previewed, then copied exactly, and copying starts no analysis", async ({
  page,
}) => {
  await openDiagnostics(page);
  await page.getByRole("button", { name: "Preview", exact: true }).click();
  const report = page.getByRole("textbox", { name: "Bug report details" });
  await expect(report).toBeVisible();
  const previewed = await report.inputValue();
  expect(previewed).toContain("Sakatsu Reader diagnostic report");
  await expect(report).toHaveAttribute("readonly", "");

  const before = await qa(page, (harness) => harness.commands.length);
  await page.getByRole("button", { name: "Copy", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "Copied." })).toBeVisible();

  expect(await qa(page, (harness) => harness.copied)).toEqual([previewed]);
  const after = await qa(page, (harness) => harness.commands.slice(0));
  const since = after.slice(before);
  expect(since).toContain("copy_text");
  for (const analysis of ["study", "analyze", "read_frame", "recognize_clipboard"]) {
    expect(since, `copying started ${analysis}`).not.toContain(analysis);
  }
});

/**
 * With Windows keyboard navigation on — the reader-first default ignores Tab on purpose — the
 * preview is reached from its button, and focus is never thrown back to the page while it builds.
 */
test("the report is reachable from the keyboard", async ({ page }) => {
  await openDiagnostics(page, "&keyboard=tab");
  const preview = page.getByRole("button", { name: "Preview", exact: true });
  await preview.focus();
  await page.keyboard.press("Enter");
  const report = page.getByRole("textbox", { name: "Bug report details" });
  await expect(report).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(report).toBeFocused();
});

test("a report that cannot be built says so", async ({ page }) => {
  await openDiagnostics(page);
  await qa(page, (harness) => harness.failNext("diagnostic_report"));
  await page.getByRole("button", { name: "Preview", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "could not be built" })).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Bug report details" })).toHaveCount(0);
});

/**
 * The longest a row can be: a GPU named without a single break opportunity, and the report open.
 * Nothing escapes its card, at every window the suite runs.
 */
test("a long device name and an open report stay inside the card", async ({ page }) => {
  await openDiagnostics(page, "&fixture=long-paths");
  await expect(page.locator("dl.engines dd").first()).toContainText(
    "GPU acceleration active (29/29 layers)",
  );
  await page.getByRole("button", { name: "Preview", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Bug report details" })).toBeVisible();
  await assertContained(page);
  const sideways = await page
    .locator(".pane")
    .evaluate((pane) => pane.scrollWidth - pane.clientWidth);
  expect(sideways, "the pane does not scroll sideways").toBeLessThanOrEqual(1);
});

/**
 * **Unloading is reported as it happened.** The row stops saying "ready", the button goes with
 * it, and the next use loads the model again. A refusal — an engine still busy — is shown as an
 * alert, not swallowed.
 */
test("a loaded engine can be unloaded from memory, and a refusal is shown", async ({ page }) => {
  await openDiagnostics(page, "&translator=loaded");
  const list = page.locator("dl.engines");
  const recognition = list.getByRole("button", { name: "Unload from memory for recognition" });
  await recognition.click();
  await expect(page.getByRole("status").filter({ hasText: "Recognition unloaded" })).toBeVisible();
  await expect(list.locator("dd").nth(0)).toContainText("not loaded yet");
  await expect(recognition).toHaveCount(0);

  await qa(page, (harness) => harness.failNext("unload_translator"));
  await list.getByRole("button", { name: "Unload from memory for translation" }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(list.locator("dd").nth(2)).toContainText("ready", { useInnerText: true });
  await assertContained(page);
});
