import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/layout",
  outputDir: "test-results/layout",
  fullyParallel: true,
  // Bound concurrent WebViews on Windows. Eight workers previously exhausted socket buffers;
  // more CPU cores do not make the browser/network resources unlimited. CLI overrides still work.
  workers: 2,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI
    ? [["line"], ["html", { outputFolder: "playwright-report", open: "never" }]]
    : "line",
  expect: { timeout: 5000 },
  use: {
    baseURL: "http://127.0.0.1:1420",
    browserName: "chromium",
    channel: "msedge",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "edge-900-100", use: { viewport: { width: 900, height: 640 }, deviceScaleFactor: 1 } },
    {
      name: "edge-620-150",
      use: { viewport: { width: 620, height: 720 }, deviceScaleFactor: 1.5 },
    },
    { name: "edge-480-200", use: { viewport: { width: 480, height: 520 }, deviceScaleFactor: 2 } },
  ],
});
