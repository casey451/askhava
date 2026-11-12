/// <reference types="node" />
// The built browser files served at /static/<crate>/ in a browser (T362, TD-SEC-62; PICK-009,
// A11Y-033, SEC-012; D-2633, D-1235).
//
// `Ci` runs `npm run build` before the journeys, so `app/portals/static/quote_page.js` exists and
// the CI server (no `ASKHAVA_STATIC_DIR`) serves it from the repository. A job in Quotes ready with
// one quote is seeded through psql on DATABASE_URL, as pick_flow.spec.ts does; the page's script
// must load as JavaScript under the security headers with no console error, and a quote stored
// after load must give one status line, "1 new quote added below.", on the next 30-second poll
// (Playwright's clock runs it at once). Fake data only.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import { test, expect } from "./fixtures";

function psql(sql: string): void {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed: ${done.stderr}`);
}

/** A fake quote on the job whose quote-page token hashes to `hash`, received `minutes` from now. */
function addQuote(hash: string, minutes: number): void {
  psql(`INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes,
      original, ai_read, confirmed, received_at)
    SELECT job_id, gen_random_uuid(), 'firm', 19000, false, '', 'Sample wording T362 ${String(minutes)}',
      false, true, now() + interval '${String(minutes)} minutes'
    FROM jobs.quote_token WHERE token_hash = decode('${hash}', 'hex')`);
}

/** A job in Quotes ready with one quote; returns the page path and the token's hash. */
function seedQuotePage(): { path: string; hash: string } {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  psql(`WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j)
    INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('${hash}', 'hex'), now() FROM j`);
  addQuote(hash, 1);
  return { path: `/q/${token}`, hash };
}

test("TD-SEC-62 PICK-009 A11Y-033 SEC-012: the quote page's script loads with no console error and a later quote gives one status line", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.clock.install();
  const { path, hash } = seedQuotePage();
  const script = page.waitForResponse((r) => new URL(r.url()).pathname === "/static/portals/quote_page.js");
  await page.goto(path);
  const served = await script;
  expect(served.status(), "the script's answer").toBe(200);
  const headers = served.headers();
  expect(headers["content-type"]).toContain("javascript");
  expect(headers["x-content-type-options"]).toBe("nosniff");
  expect(headers["content-security-policy"]).toContain("object-src 'none'");
  const status = page.locator("#quote-status");
  await expect(status).toHaveText("");
  await expect(page.locator("#quotes li[data-number]")).toHaveCount(1);
  addQuote(hash, 2);
  await page.clock.runFor(30_000);
  await expect(page.locator("#quotes li[data-number]")).toHaveCount(2);
  await expect(status).toHaveText("1 new quote added below.");
  expect(errors, "console errors").toEqual([]);
});
