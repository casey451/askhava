// Playwright configuration for the Ally suite (T053; A11Y-001, A11Y-002; docs/test-design.md section 6).
//
// The app is started by the e2e suite's globalSetup, tests/e2e/webserver.ts (web and worker binaries
// with the test-control feature, the stand-ins, the migrations), against the Postgres in
// DATABASE_URL; fixtures.ts seeds each page state into that same database. Pages keep their CSP
// (SEC-013); only the text-spacing tests in reflow_spacing.spec.ts turn on `bypassCSP`, to add their
// stylesheet. axe runs through Playwright's evaluate, which CSP never blocks. Besides the list on
// the console, a JSON report with every attachment (axe's incomplete results, A11Y-025) is written
// to test-results/a11y-report/results.json. Any failed check fails the run, which turns `Ally` red
// once T173 runs it there.
// Run: npx playwright test --config tests/a11y/playwright.config.ts
import { defineConfig } from "@playwright/test";
import { webPort } from "../e2e/webserver";

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.spec.ts",
  globalSetup: "../e2e/webserver.ts",
  fullyParallel: false,
  workers: 1,
  forbidOnly: true,
  reporter: [["list"], ["json", { outputFile: "../../test-results/a11y-report/results.json" }]],
  outputDir: "../../test-results/a11y",
  use: {
    baseURL: `http://127.0.0.1:${String(webPort())}`,
  },
});
