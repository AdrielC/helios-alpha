import { defineConfig, devices } from "@playwright/test";

const externalBaseUrl = process.env.HELIOS_OPERATOR_BASE_URL;

export default defineConfig({
  testDir: "./tests/e2e",
  outputDir: "./test-results",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  workers: 2,
  reporter: process.env.CI ? [["github"], ["html", { open: "never" }]] : "list",
  snapshotPathTemplate: "{testDir}/__screenshots__/{arg}{ext}",
  expect: {
    timeout: 10_000,
    toHaveScreenshot: {
      animations: "disabled",
      caret: "hide",
      maxDiffPixelRatio: 0.025,
    },
  },
  use: {
    baseURL: externalBaseUrl ?? "http://127.0.0.1:4175",
    colorScheme: "dark",
    locale: "en-US",
    launchOptions: process.env.CI ? undefined : {
      executablePath: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    },
    timezoneId: "UTC",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "desktop-chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 1000 } },
    },
  ],
  webServer: externalBaseUrl ? undefined : {
    command: "npm run dev -- --host 127.0.0.1 --port 4175",
    url: "http://127.0.0.1:4175/#overview",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
