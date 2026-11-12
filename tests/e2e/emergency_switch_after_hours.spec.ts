/// <reference types="node" />
// The emergency-switch page's after-hours box in a browser (T369; TD-BIZ-47; BIZ-007, A11Y-001;
// D-2702, D-3106).
//
// The page is not served until T306, so no browser can reach it through a session. The pages are
// the template as rendered: the integration test `td_biz_47_write_pages_for_axe`
// (tests/integration/tests/after_hours_cover.rs) writes the switch page with the after-hours box
// unticked, ticked, ticked but suspended (D-3239), and after a refused tick listing the steps
// still missing into a temporary folder, and each is loaded here and scanned with axe at 320, 768 and 1280 px. Who may tick and
// when is checked in that integration test. Fake data only.
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { test, expect } from "./fixtures";

const PAGES = ["unticked", "ticked", "suspended", "refused"];
const REPO_ROOT = join(import.meta.dirname, "..", "..");

/** Renders the switch page's after-hours states through the integration test and returns the folder. */
function renderPages(): string {
  const dir = mkdtempSync(join(tmpdir(), "emergency-switch-"));
  const run = spawnSync(
    "cargo",
    ["test", "-p", "askhava-integration", "--test", "after_hours_cover", "td_biz_47_write_pages_for_axe"],
    { cwd: REPO_ROOT, encoding: "utf8", env: { ...process.env, EMERGENCY_SWITCH_HTML_DIR: dir, SQLX_OFFLINE: "true" } },
  );
  if (run.status !== 0) throw new Error(`rendering the pages failed with exit ${String(run.status)}`);
  return dir;
}

test("TD-BIZ-47 (axe part): the after-hours box unticked, ticked, suspended and refused passes axe at 320, 768 and 1280 px", async ({ page }) => {
  test.setTimeout(600_000);
  const dir = renderPages();
  try {
    for (const name of PAGES) {
      await page.setContent(readFileSync(join(dir, `${name}.html`), "utf8"));
      await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
      await expect(page.getByRole("heading", { name: "Emergency jobs from 8 p.m. to 8 a.m." })).toHaveCount(1);
      await expect(page.locator("#after-hours-line")).toContainText("marked EMERGENCY");
      for (const width of WIDTHS) {
        await page.setViewportSize({ width, height: 900 });
        const { violations } = await axeScan(page);
        expect(violations, `${name} at ${String(width)} px`).toEqual([]);
      }
    }
    await page.setContent(readFileSync(join(dir, "refused.html"), "utf8"));
    await expect(page.getByRole("alert")).toContainText("Send a test emergency email.");
    await page.setContent(readFileSync(join(dir, "ticked.html"), "utf8"));
    await expect(page.getByRole("button", { name: "Stop taking emergency jobs at night" })).toHaveCount(1);
    await page.setContent(readFileSync(join(dir, "suspended.html"), "utf8"));
    await expect(page.locator("#after-hours-suspended")).toContainText("paused");
    await expect(page.locator("#after-hours-on")).toHaveCount(0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
