import { defineConfig, devices } from "@playwright/test";

/**
 * These tests drive the REAL mobile web app in a browser against the REAL Veydrop engine and
 * HTTP server (started headless by `crates/veydrop-core/examples/e2e_harness.rs`) over the PC's
 * real LAN address. Device profiles only emulate viewport and user agent: no physical iPhone or
 * Android phone is involved, and WebKit is not iOS Safari.
 */
export default defineConfig({
  testDir: "./tests",
  timeout: 60_000,
  expect: { timeout: 10_000 },
  workers: 1,
  fullyParallel: false,
  retries: 0,
  reporter: [["list"], ["html", { open: "never", outputFolder: "report" }]],
  outputDir: "results",
  use: {
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    acceptDownloads: true,
  },
  projects: [
    { name: "chromium-android-profile", use: { ...devices["Pixel 7"] } },
    { name: "webkit-iphone-profile", use: { ...devices["iPhone 14"] } },
  ],
});
