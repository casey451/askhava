// Playwright configuration for the Ally suite (T053; A11Y-001, A11Y-002; docs/test-design.md section 6).
//
// The app is started by the e2e suite's globalSetup, tests/e2e/webserver.ts (web and worker binaries
// with the test-control feature, the stand-ins, the migrations), against the Postgres in
// DATABASE_URL; fixtures.ts seeds each page state into that same database. `bypassCSP` lets the
// checks add the text-spacing stylesheet to a page whose CSP allows no inline style (SEC-013); axe
// itself runs through Playwright's evaluate, which CSP never blocks. Any failed check fails the run,
// which turns `Ally` red once T173 runs it there.
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
  reporter: [["list"]],
  outputDir: "../../test-results/a11y",
  use: {
    baseURL: `http://127.0.0.1:${String(webPort())}`,
    bypassCSP: true,
  },
});
