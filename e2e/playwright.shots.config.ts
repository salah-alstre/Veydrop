import { defineConfig, devices } from "@playwright/test";

/** Captures the README screenshots from the real mobile UI served by the real engine. Run: npx playwright test -c playwright.shots.config.ts */
export default defineConfig({
  testDir: "./screenshots",
  timeout: 60_000,
  workers: 1,
  reporter: "list",
  outputDir: "results-shots",
  projects: [{ name: "shots", use: { ...devices["Pixel 7"], deviceScaleFactor: 2 } }],
});
