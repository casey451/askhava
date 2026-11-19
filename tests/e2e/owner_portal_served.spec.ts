/// <reference types="node" />
// The served owner home in a browser (T310; the browser halves of TD-OPS-01 and TD-BIZ-07 named
// by TD-SEC-50; OPS-003, FEE-007, BIZ-027, SEC-043; D-2128, D-3744, D-3890).
//
// The owner signs in only through the shared tests/e2e/owner_session.ts helper (never running
// `bootstrap-owner` here) and steps up with a fresh code, then pauses one trade and resumes it,
// switches the fee mode, and reads a needs-review sign-up's 4-day decision line, seeded through
// psql on DATABASE_URL (a fake business with no contact). The end-to-end database is shared with
// one worker, so the trade is resumed and the fee mode switched back in a `finally`. The wrong-
// session, CSRF and fresh-factor refusals are app/server/tests/owner_portal_route.rs; the axe
// checks at 320, 768 and 1280 are the Ally suite's (tests/a11y/page_states.ts). Fake data only.
import { randomUUID } from "node:crypto";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { psql } from "./keyed";
import { ownerCode, signInOwner } from "./owner_session";

// Up to two TOTP steps of waiting, plus the pages (D-3878).
test.setTimeout(150_000);

/** Seeds one needs-review sign-up waiting since a day ago. */
function seedNeedsReview(): void {
  const business = randomUUID();
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'needs_review', now(), now()) RETURNING id)
    INSERT INTO businesses.signup (business_id, mailbox_id, review_since, submitted_at)
      SELECT id, gen_random_uuid(), now() - interval '1 day', now() - interval '1 day' FROM b`);
}

/** Saves the fee mode whose radio is labelled `label`, back on the home page. */
async function saveFeeMode(page: Page, label: string): Promise<void> {
  await page.getByLabel(label).check();
  await page.getByRole("button", { name: "Save fee mode" }).click();
  await expect(page).toHaveURL(/\/owner\/home$/);
}

test("TD-OPS-01, TD-BIZ-07: the owner pauses a trade, switches the fee mode and sees the review target", async ({ page }) => {
  seedNeedsReview();
  await signInOwner(page);
  await page.getByLabel("Code", { exact: true }).fill(await ownerCode());
  await page.getByRole("button", { name: "Check code" }).click();
  await expect(page.locator("#step-up-done")).toHaveText(/That worked/);
  const home = await page.goto("/owner/home");
  expect(home?.status()).toBe(200);
  expect(home?.headers()["cache-control"]).toContain("no-store");
  await expect(page.locator("#needs-review")).toContainText("4 calendar days after this came in");
  const row = page.locator("#pause-switches li").nth(3);
  const name = ((await row.getByRole("button").textContent()) ?? "").replace(/^Pause /, "");
  try {
    await row.getByRole("button", { name: `Pause ${name}` }).click();
    await expect(row).toContainText("is paused");
    await row.getByRole("button", { name: `Resume ${name}` }).click();
    await expect(row).toContainText("is running");
    await saveFeeMode(page, "One flat fee for each job type");
    await expect(page.getByLabel("One flat fee for each job type")).toBeChecked();
  } finally {
    await page.goto("/owner/home");
    const resume = row.getByRole("button", { name: `Resume ${name}` });
    if ((await resume.count()) > 0) await resume.click();
    if (!(await page.getByLabel("By the size of each quote").isChecked())) await saveFeeMode(page, "By the size of each quote");
  }
  await expect(page.getByLabel("By the size of each quote")).toBeChecked();
});
