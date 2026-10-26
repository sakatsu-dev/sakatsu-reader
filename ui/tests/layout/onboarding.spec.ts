import { expect, test } from "./fixtures";
import { assertContained } from "./contained";

async function deferAnalyzer(page: import("@playwright/test").Page) {
  await expect(page.getByRole("heading", { name: "Read with Ichiran, assisted by Lindera." })).toBeVisible();
  await page.getByRole("button", { name: "Use Lindera for now" }).click();
}

test("fresh setup explains the local install before consent and remains keyboard reachable", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=fresh");
  await expect(page.getByRole("heading", { name: "Read Japanese without leaving your game or book." })).toBeVisible();
  await expect(page.getByText("Dictionaries + two OCR engines", { exact: true })).toBeVisible();
  await expect(page.getByText(/Nothing downloads until you continue/)).toBeVisible();
  const begin = page.getByRole("button", { name: "Set up Sakatsu Reader" });
  await begin.focus();
  await expect(begin).toBeFocused();
  await assertContained(page);
});

test("system checks keep repair choices and technical paths behind disclosure", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=fresh");
  await page.getByRole("button", { name: "Set up Sakatsu Reader" }).click();
  await expect(page.getByRole("heading", { name: /finish cleanly/ })).toBeVisible();
  await expect(page.getByText("Windows 10/11 x64", { exact: true })).toBeVisible();
  await expect(page.getByText("Downloaded resources location", { exact: true })).toBeHidden();
  await page.getByText("Technical details", { exact: true }).click();
  await expect(page.getByText("Downloaded resources location", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Browse…" }).first()).toBeVisible();
  await assertContained(page);
});

test("active setup is one friendly resumable task and excludes optional features", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=downloading");
  await expect(page.getByRole("heading", { name: "Setting up screen capture…" })).toBeVisible();
  await expect(page.getByRole("strong").filter({ hasText: "manga-ocr" })).toBeVisible();
  await expect(page.getByText(/MB\/s/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Pause" })).toBeVisible();
  for (const optional of ["Translation", "Offline pronunciation", "Frequency", "Ichiran"]) {
    await expect(page.getByText(optional, { exact: true })).toHaveCount(0);
  }
  await assertContained(page);
});

test("paused or failed setup offers a calm retry and text-only route", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=error");
  await expect(page.getByRole("heading", { name: "Your progress is safe." })).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry now" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Use pasted text while setup finishes" })).toBeVisible();
  await expect(page.getByText(/eight attempts/)).toBeVisible();
  await assertContained(page);
});

test("successful verification ends with two concrete first actions", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=complete-after-start");
  await page.getByRole("button", { name: "Set up Sakatsu Reader" }).click();
  await page.getByRole("button", { name: "Download and install" }).click();
  await deferAnalyzer(page);
  await expect(page.getByRole("heading", { name: "Sakatsu Reader is ready." })).toBeVisible();
  await expect(page.getByText("Paste Japanese", { exact: true })).toBeVisible();
  await expect(page.getByText("Capture from the screen", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Start reading" }).click();
  await expect(page.getByRole("button", { name: "Analyze" })).toBeVisible();
  await assertContained(page);
});

test("the Ready secondary action opens optional downloads outside onboarding", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=complete-after-start");
  await page.getByRole("button", { name: "Set up Sakatsu Reader" }).click();
  await page.getByRole("button", { name: "Download and install" }).click();
  await deferAnalyzer(page);
  await expect(page.getByRole("heading", { name: "Sakatsu Reader is ready." })).toBeVisible();
  await page.getByRole("button", { name: "Explore optional features" }).click();
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Optional downloads" })).toBeVisible();
  await expect(page.getByText("Translation", { exact: true })).toBeVisible();
  await expect(page.getByText("Offline pronunciation", { exact: true })).toBeVisible();
  await assertContained(page);
});

test("reviewed fresh onboarding layouts remain visually stable", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name === "edge-620-150", "desktop and minimum-window baselines cover both layout modes");
  await page.goto("/?qa=onboarding&setup=fresh");
  await expect(page).toHaveScreenshot("onboarding-welcome.png", {
    animations: "disabled",
    maxDiffPixelRatio: 0.01,
  });
});

/**
 * The last onboarding step still offers both ways out.
 *
 * `onboarding-welcome` is the only stage with a pixel baseline, and Ready is the stage where a
 * missing button strands somebody who has just finished installing. Structure is the right
 * assertion for that, and it costs no baseline to maintain.
 */
test("the ready stage offers both a first action and the optional route", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=complete-after-start");
  await page.getByRole("button", { name: "Set up Sakatsu Reader" }).click();
  await page.getByRole("button", { name: "Download and install" }).click();
  await deferAnalyzer(page);
  await expect(page.locator(".onboarding, .stage").first()).toMatchAriaSnapshot(`
    - heading "Sakatsu Reader is ready." [level=1]
    - button "Start reading"
    - button "Explore optional features"
  `);
});

test("the analyzer step resumes after restart and requires a real answering analyzer", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=analyzer");
  await expect(page.getByRole("heading", { name: "Read with Ichiran, assisted by Lindera." })).toBeVisible();
  await expect(page.getByText(/about 4.7 GB for Ichiran/)).toBeVisible();
  await page.getByRole("button", { name: "Use installed Ichiran" }).click();
  await expect(page.getByRole("alert")).toContainText("Ichiran is not answering yet");
  await expect(page.getByRole("button", { name: "Start reading", exact: true })).toHaveCount(0);
  await assertContained(page);
});

test("guided setup is explicitly started, then adopted without restarting Sakatsu", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=analyzer");
  await page.getByRole("button", { name: /Set up Ichiran/ }).click();
  await page.getByRole("button", { name: "Use Ichiran now" }).click();
  await expect(page.getByRole("heading", { name: "Sakatsu Reader is ready." })).toBeVisible();
  await page.getByRole("button", { name: "Start reading", exact: true }).click();
  await expect(page.getByRole("button", { name: /^Analyze/ })).toBeVisible();
});

test("a working installation can be adopted without running an installer", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=analyzer&ichiran=ready");
  await page.getByRole("button", { name: "Use installed Ichiran" }).click();
  await expect(page.getByRole("heading", { name: "Sakatsu Reader is ready." })).toBeVisible();
});

/**
 * **"Use Lindera for now" selects Lindera**, including where Ichiran already answers. It once only
 * dismissed the step, which left Auto in charge: a reader who chose Lindera on a machine with a
 * working Ichiran went on reading with Ichiran.
 */
test("using Lindera for now selects the built-in analyzer, even where Ichiran answers", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=analyzer&ichiran=ready");
  await deferAnalyzer(page);
  await expect(page.getByRole("heading", { name: "Sakatsu Reader is ready." })).toBeVisible();
  await page.getByRole("button", { name: "Start reading", exact: true }).click();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByRole("combobox", { name: "Analyzer" })).toHaveValue("builtin");
});

/**
 * A setup whose completion cannot be heard says so and can try again, rather than leaving a
 * permanently grey button with an ordinary status line beside it.
 */
test("a setup that cannot listen for completion says why and recovers on retry", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=analyzer&ichiran=listen-fails");
  await expect(page.getByRole("alert")).toContainText("could not listen for setup to finish");
  await expect(page.getByRole("button", { name: /Set up Ichiran/ })).toBeDisabled();
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByRole("button", { name: /Set up Ichiran/ })).toBeEnabled();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("prerequisite failure keeps setup reachable and allows explicit deferral", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=analyzer&ichiran=install-fails");
  await page.getByRole("button", { name: /Set up Ichiran/ }).click();
  await expect(page.getByRole("status").filter({ hasText: "Restart Windows" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Set up Ichiran/ })).toBeEnabled();
  await deferAnalyzer(page);
  await expect(page.getByRole("heading", { name: "Sakatsu Reader is ready." })).toBeVisible();
});

test("a completed installer cannot bypass a failing probe", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=analyzer&ichiran=probe-fails");
  await page.getByRole("button", { name: /Set up Ichiran/ }).click();
  await page.getByRole("button", { name: "Use Ichiran now" }).click();
  await expect(page.getByRole("alert")).toContainText("Ichiran is not answering yet");
});

test("setup cannot be dismissed while the installer is running or a choice cannot be saved", async ({ page }) => {
  await page.goto("/?qa=onboarding&setup=analyzer&ichiran=running");
  await page.getByRole("button", { name: /Set up Ichiran/ }).click();
  await expect(page.getByRole("button", { name: "Use Lindera for now" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Use installed Ichiran" })).toBeDisabled();
  await page.goto("/?qa=onboarding&setup=analyzer&ichiran=save-fails");
  await page.getByRole("button", { name: "Use Lindera for now" }).click();
  await expect(page.getByRole("alert")).toContainText("could not be saved");
  await expect(page.getByRole("heading", { name: "Read with Ichiran, assisted by Lindera." })).toBeVisible();
});

test("completed existing installations are not forced back through setup", async ({ page }) => {
  await page.goto("/?qa=analysis&setup=complete");
  await expect(page.locator(".onboarding")).toHaveCount(0);
});

test("reviewed analyzer onboarding layouts remain visually stable", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name === "edge-620-150", "wide and minimum-window baselines cover both layout modes");
  await page.goto("/?qa=onboarding&setup=analyzer");
  await expect(page.getByRole("button", { name: /Set up Ichiran/ })).toBeEnabled();
  await assertContained(page);
  await expect(page).toHaveScreenshot("onboarding-analyzer.png", { animations: "disabled", maxDiffPixelRatio: 0.01 });
});
