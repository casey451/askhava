// Playwright configuration for the e2e and accessibility journeys (T227, D-453, D-491).
//
// `globalSetup` is tests/e2e/webserver.ts, which builds and starts the web and worker binaries
// (test-control feature) and the stand-ins, waits for the web port, and stops them after the run.
// `controlSecret` is the value that file gave the web binary, so the `control` fixture in
// fixtures.ts can reach the test-control router. Specs live beside this file as `*.spec.ts`; they
// import `test` and `expect` from ./fixtures. In CI (`CI` set; the Ci job's "E2e" part, T173) a
// stray `test.only` fails the run and each failure is also an annotation on the commit; no test is
// retried, so one failing assertion turns `Ci` red. Run: npx playwright test --config tests/e2e/playwright.config.ts
//
// T536 (D-3889, D-3892): the test clock is forward-only and shared by every binary, so the journeys
// that advance it by hours to days (`LATE`) form a second project, `late`, that depends on `main`
// and runs after every other spec, whose seeded sessions and links would otherwise have expired.
// Playwright runs a dependency project unfiltered, so a single-file run of a late spec runs all of
// `main` first: add `--no-deps` while iterating on one late spec.
import { defineConfig } from "@playwright/test";
import type { ControlOptions } from "./fixtures";
import { controlSecret, webPort } from "./webserver";

/** The clock-advancing journeys, run last in the `late` project (D-3892). */
const LATE = ["**/j1_full*.spec.ts", "**/j2_full*.spec.ts", "**/j1_j2_web_email*.spec.ts", "**/j3_*.spec.ts", "**/j4_*.spec.ts"];

export default defineConfig<ControlOptions>({
  testDir: ".",
  testMatch: "**/*.spec.ts",
  globalSetup: "./webserver.ts",
  fullyParallel: false,
  workers: 1,
  forbidOnly: process.env.CI !== undefined,
  retries: 0,
  reporter: process.env.CI === undefined ? [["list"]] : [["list"], ["github"]],
  outputDir: "../../test-results",
  use: {
    baseURL: `http://127.0.0.1:${String(webPort())}`,
    controlSecret: controlSecret(),
  },
  projects: [
    { name: "main", testIgnore: LATE },
    { name: "late", testMatch: LATE, dependencies: ["main"] },
  ],
});
