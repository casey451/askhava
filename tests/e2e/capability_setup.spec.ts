/// <reference types="node" />
// The sign-up capability step in a browser (T155; TD-BIZ-29; BIZ-049, BIZ-004, A11Y-001; D-393,
// D-1709).
//
// The sign-up routes are not served yet (D-1709), so no browser can reach the step through a
// session. What a request checks now is only the edge: the step is not served, so a signed-out
// GET or POST of it is refused (4xx) and gets no checklist. That nothing is stored is not checked
// here (no route exists to store through); `save_capability`'s rules are checked in the
// integration test. The pages themselves are the templates as rendered: the
// integration test `td_biz_29_write_pages_for_axe` (tests/integration/tests/capability_setup.rs)
// writes the one-page form and every guided screen for an R-11 business into a temporary folder,
// and each is loaded here and scanned with axe at 320, 768 and 1280 px; each guided screen asks
// one question. The answers themselves (pre-ticks, untick, extras, largest job, recurring, the
// same result either way) are checked in that integration test. Fake data only.
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { test, expect } from "./fixtures";

const SCREENS = ["all", "jobs", "towns", "largest", "recurring", "after-hours", "check"];
const QUESTIONS = ["job-types", "towns", "largest", "recurring", "after_hours"];
const REPO_ROOT = join(import.meta.dirname, "..", "..");

/** Renders every capability screen through the integration test and returns the folder. */
function renderScreens(): string {
  const dir = mkdtempSync(join(tmpdir(), "capability-"));
  const run = spawnSync(
    "cargo",
    ["test", "-p", "askhava-integration", "--test", "capability_setup", "td_biz_29_write_pages_for_axe"],
    { cwd: REPO_ROOT, encoding: "utf8", env: { ...process.env, CAPABILITY_HTML_DIR: dir, SQLX_OFFLINE: "true" } },
  );
  if (run.status !== 0) throw new Error(`rendering the screens failed with exit ${String(run.status)}`);
  return dir;
}

test("TD-BIZ-29 (edge): a signed-out GET or POST of the unserved capability step is refused with no checklist", async ({ request }) => {
  const get = await request.get("/b/signup/capability?step=all", { maxRedirects: 0 });
  expect(get.status()).toBeGreaterThanOrEqual(400);
  expect(await get.text()).not.toContain("Which jobs do you want?");
  const post = await request.post("/b/signup/capability?step=check", {
    form: { job_type: "electrical:panel upgrade", largest: "none" },
  });
  expect(post.status()).toBeGreaterThanOrEqual(400);
  expect(await post.text()).not.toContain("Which jobs do you want?");
});

test("TD-BIZ-29 (axe part): the one-page form and every guided screen pass axe at 320, 768 and 1280 px", async ({ page }) => {
  test.setTimeout(600_000);
  const dir = renderScreens();
  try {
    for (const screen of SCREENS) {
      const html = readFileSync(join(dir, `${screen}.html`), "utf8");
      await page.setContent(html);
      await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
      const asked = await page.locator(QUESTIONS.map((q) => `fieldset#${q}`).join(", ")).count();
      const want = screen === "all" ? 5 : screen === "check" ? 0 : 1;
      expect(asked, screen).toBe(want);
      for (const width of WIDTHS) {
        await page.setViewportSize({ width, height: 900 });
        const { violations } = await axeScan(page);
        expect(violations, `${screen} at ${String(width)} px`).toEqual([]);
      }
    }
    await page.setContent(readFileSync(join(dir, "jobs.html"), "utf8"));
    await expect(page.getByRole("checkbox", { name: "panel upgrade" })).toBeChecked();
    await expect(page.getByRole("checkbox", { name: "weekly pool service" })).not.toBeChecked();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
