// Playwright configuration for the lab Core Web Vitals spec (T418, D-3292).
//
// The same globalSetup as the journeys (tests/e2e/webserver.ts: the test-control web and worker
// binaries and the stand-ins against DATABASE_URL) and the same `controlSecret` option, but its own
// testDir, so `--config tests/e2e/playwright.config.ts` never runs this spec and this config runs
// nothing else. Chromium only (the throttling goes through the DevTools protocol). Lab figures are
// never a pass or fail: the spec asserts only that each figure was measured, and perf/compare.py
// warns on a missed goal. Run: npx playwright test --config tests/perf/playwright.config.ts
import { defineConfig } from "@playwright/test";
import type { ControlOptions } from "../e2e/fixtures";
import { controlSecret, webPort } from "../e2e/webserver";

export default defineConfig<ControlOptions>({
  testDir: ".",
  testMatch: "**/*.spec.ts",
  globalSetup: "../e2e/webserver.ts",
  fullyParallel: false,
  workers: 1,
  forbidOnly: process.env.CI !== undefined,
  retries: 0,
  timeout: 240_000,
  reporter: process.env.CI === undefined ? [["list"]] : [["list"], ["github"]],
  outputDir: "../../test-results/perf",
  use: {
    browserName: "chromium",
    viewport: { width: 390, height: 844 },
    baseURL: `http://127.0.0.1:${String(webPort())}`,
    controlSecret: controlSecret(),
  },
});
