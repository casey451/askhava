/// <reference types="node" />
// The served owner reports and help reply in a browser (T305; TD-SEC-48's browser half and the
// browser half of TD-OPS-15; OPS-039, OPS-040, OPS-036, FEE-046, SEC-005, SEC-034; D-2128,
// D-3744, D-4897, D-5318).
//
// The owner signs in only through the shared tests/e2e/owner_session.ts helper (never running
// `bootstrap-owner` here), then saves this week's minutes on the pilot scorecard, adds a fake
// expense on the money view and sees it listed on its month's page, opens the intake report, and
// types a reply on the help page to a fake email-only help request seeded through psql, which the
// page refuses as email-only and sends nothing. The end-to-end database is shared with one
// worker, so the request is closed in a `finally`. The wrong-session, CSRF and cross-site
// refusals are app/server/tests/owner_reports_route.rs; the axe checks at 320, 768 and 1280 are
// the Ally suite's (tests/a11y/page_states.ts). Fake data only.
import { randomBytes } from "node:crypto";
import { test, expect } from "./fixtures";
import { psql } from "./keyed";
import { signInOwner } from "./owner_session";

// Up to one TOTP step of waiting, plus the pages (D-3878).
test.setTimeout(150_000);

test("TD-SEC-48, TD-OPS-15: the owner saves minutes, adds an expense and tries a help reply", async ({ page }) => {
  await signInOwner(page);

  const scorecard = await page.goto("/owner/scorecard");
  expect(scorecard?.status()).toBe(200);
  expect(scorecard?.headers()["cache-control"]).toContain("no-store");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Pilot scorecard");
  await expect(page.locator("#ladder")).toContainText("paid introductions");
  await page.getByLabel("Minutes spent recruiting businesses").fill("90");
  await page.getByLabel("Minutes spent on everything else").fill("45");
  await page.getByRole("button", { name: "Save minutes" }).click();
  await expect(page.getByRole("status").first()).toHaveText("Your minutes for this week are saved.");

  const vendor = `Sample Vendor ${randomBytes(4).toString("hex")}`;
  const money = await page.goto("/owner/money");
  expect(money?.headers()["cache-control"]).toContain("no-store");
  await page.getByLabel("Date").fill(new Date().toISOString().slice(0, 10));
  await page.getByLabel("Paid to").fill(vendor);
  await page.getByLabel("Amount in dollars").fill("12.50");
  await page.getByLabel("Category").fill("Software");
  await page.getByRole("button", { name: "Add expense" }).click();
  await expect(page).toHaveURL(/\/owner\/money\?month=\d{4}-\d{2}$/);
  await expect(page.locator("#expenses").getByRole("row").filter({ hasText: vendor })).toHaveCount(1);

  const report = await page.goto("/owner/intake-report");
  expect(report?.status()).toBe(200);
  await expect(page.getByRole("heading", { level: 1 })).toContainText("Intake report, ");

  const request = psql(`INSERT INTO ops.help_request (asker, opened_at, due_at)
    VALUES ('email', now(), now() + interval '1 hour') RETURNING id`).split("\n")[0] ?? "";
  try {
    const help = await page.goto(`/owner/help?request=${request}`);
    expect(help?.headers()["cache-control"]).toContain("no-store");
    await page.getByLabel("Your reply").fill("Sample reply: we looked into it.");
    await page.getByRole("button", { name: "Send reply" }).click();
    await expect(page.locator("#error-summary")).toContainText("This reply has to go by email for now.");
    await expect(page.getByLabel("Your reply")).toHaveValue("Sample reply: we looked into it.");
    expect(psql(`SELECT count(*) FROM ops.help_message WHERE request_id = '${request}' AND from_owner`)).toBe("0");
  } finally {
    psql(`UPDATE ops.help_request SET closed_at = now() WHERE id = '${request}'`);
  }
});
