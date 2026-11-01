/// <reference types="node" />
// The reduced J1, the stage-2 exit evidence (T173; docs/test-design.md section 5, J1; D-491, D-1280).
//
// J1 is the homeowner's journey: (1) text in and consent, (2) ZIP and trade, (3) the urgency and
// question set, (4) the card shown and sent, (5) quotes on the quote page, (6) the pick by code and
// the contact choice. The reduced form leaves out the photo, the review links, the share box, the
// payment and the call, and seeds the two diagnostic-visit quotes instead of a business quoting.
// What runs today is step 5 and part of step 6: the review step, then the code-and-contact form
// reached directly by rewriting the final form's action to /code, not through the real final Pick
// button, which neither sends nor checks a code yet (D-1248, D-1264). Steps 1 to 4 (no route takes
// an inbound text, T241) and the verified pick are not in this file; D-1280 records them. The
// test-control router has no seeding route, so the job and its quotes are seeded through psql on
// DATABASE_URL, as tests/e2e/pick_flow.spec.ts does (D-1280). POSTs are re-sent with a same-origin
// https Origin until D-1250's fix lands. Fake data only: no name, phone or address is stored.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";

/** A job in Quotes ready with two diagnostic-visit quotes ($89 visit each); returns its quote page. */
function seedTwoDiagnosticQuotes(): string {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  const sql = `WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    t AS (INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('${hash}', 'hex'), now() FROM j)
    INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes,
      original, ai_read, confirmed, received_at)
    SELECT j.id, gen_random_uuid(), 'diagnostic_visit', 8900, false, '',
      'Diagnostic visit, sample wording ' || q.n, false, true, now() + q.n * interval '1 minute'
    FROM j, generate_series(1, 2) AS q(n)`;
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding the two quotes failed: ${done.stderr}`);
  return `/q/${token}`;
}

/** Re-sends every POST with a same-origin https Origin (D-1250). */
async function httpsOrigin(page: Page): Promise<void> {
  await page.route("**/q/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const headers = { ...request.headers(), origin: `https://${new URL(request.url()).host}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

test("J1 step 5: both seeded diagnostic-visit quotes are on the quote page, in arrival order, each with Pick", async ({ page }) => {
  const path = seedTwoDiagnosticQuotes();
  await page.goto(path);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Your quotes for request \d+$/);
  await expect(page.locator("#quotes > li")).toHaveCount(2);
  for (const n of [1, 2]) {
    const card = page.locator(`#quote-${String(n)}`);
    await expect(card.getByRole("heading", { level: 2 })).toHaveText(`Quote #${String(n)}`);
    await expect(card).toContainText(`Diagnostic visit, sample wording ${String(n)}`);
    await expect(card.getByRole("button", { name: `Pick #${String(n)}` })).toBeEnabled();
    await expect(card).toContainText(`or reply ${String(n)} YES`);
  }
  await expect(page.locator("#no-quotes")).toHaveCount(0);
});

test("J1 step 6 (partial): Pick #2 and the review step, then the code-and-contact form reached directly (form action rewritten to /code), with the call or email choice", async ({ page }) => {
  const path = seedTwoDiagnosticQuotes();
  await httpsOrigin(page);
  await page.goto(path);
  await page.getByRole("button", { name: "Pick #2" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Check your pick: quote #2");
  await page.locator("#final-pick").evaluate((button) => {
    const form = button.closest("form");
    if (form === null) throw new Error("no final form");
    form.action = form.action.replace(/\/pick$/, "/code");
  });
  await page.locator("#final-pick").click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Finish your pick for request \d+$/);
  await expect(page.locator("main")).toContainText("You picked quote #2.");
  await expect(page.getByRole("group", { name: "How should the business reach you?" })).toBeVisible();
  await page.getByLabel(/^A phone call/).check();
  await expect(page.getByLabel(/^Email\./)).not.toBeChecked();
  await expect(page.getByLabel("6-digit code")).toHaveAttribute("autocomplete", "one-time-code");
  await expect(page.getByRole("button", { name: "Finish my pick" })).toBeEnabled();
});
