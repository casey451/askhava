// Playwright configuration for the e2e and accessibility journeys (T227, D-453, D-491).
//
// `globalSetup` is tests/e2e/webserver.ts, which builds and starts the web and worker binaries
// (test-control feature) and the stand-ins, waits for the web port, and stops them after the run.
// `controlSecret` is the value that file gave the web binary, so the `control` fixture in
// fixtures.ts can reach the test-control router. Specs live beside this file as `*.spec.ts`; they
// import `test` and `expect` from ./fixtures. Run: npx playwright test --config tests/e2e/playwright.config.ts
import { defineConfig } from "@playwright/test";
import type { ControlOptions } from "./fixtures";
import { controlSecret, webPort } from "./webserver";

export default defineConfig<ControlOptions>({
  testDir: ".",
  testMatch: "**/*.spec.ts",
  globalSetup: "./webserver.ts",
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  outputDir: "../../test-results",
  use: {
    baseURL: `http://127.0.0.1:${String(webPort())}`,
    controlSecret: controlSecret(),
  },
});
