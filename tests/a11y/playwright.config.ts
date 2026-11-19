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
//
// T536 (D-3889, D-3892): the test clock is forward-only and shared by every binary, so the journeys
// that advance it (`LATE`) form a second project, `late`, that depends on `main` and runs after
// every other spec, whose PAGE_STATES sessions and 15-minute and 2-hour links would otherwise have
// expired. Playwright runs a dependency project unfiltered, so a single-file run of a late spec
// runs all of `main` first: add `--no-deps` while iterating on one late spec.
import { defineConfig } from "@playwright/test";
import { webPort } from "../e2e/webserver";

/** The clock-advancing journeys, run last in the `late` project (D-3892). */
const LATE = ["**/j_full_keyboard*.spec.ts", "**/j3_*.spec.ts", "**/j4_*.spec.ts"];

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
  projects: [
    { name: "main", testIgnore: LATE },
    { name: "late", testMatch: LATE, dependencies: ["main"] },
  ],
});
