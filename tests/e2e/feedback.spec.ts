/// <reference types="node" />
// The private feedback page in a browser (T230; TD-JOB-10; JOB-017; D-083, D-466, D-493).
//
// A Done job with a live link token is seeded through psql on DATABASE_URL, as pick_flow.spec.ts
// does; the page opens at /f/<token>. Under the site's `Referrer-Policy: no-referrer` the browser
// sends `Origin: null` on a same-origin form POST, which `security::is_cross_site` refuses (D-1250),
// so `page.route` re-sends each POST with the https Origin a same-origin request has. The stored
// row is read back through psql to check the words and the tick were kept as sent; which feedback
// may reach askhava.com (ticked only, a negative one like a positive one, the four removal reasons,
// never the owner's, a relative's or a business's) is checked on the database in
// app/portals/src/feedback.rs. Fake data only.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { test, expect } from "./fixtures";

const LABEL = "Ask Hava may show what I wrote on askhava.com with my first name and town";

function psql(sql: string): string {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-t", "-A", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed: ${done.stderr}`);
  return done.stdout.trim();
}

/** A new random link token and its SHA256 hash. */
function newToken(): { token: string; hash: string } {
  const token = randomBytes(32).toString("hex");
  return { token, hash: createHash("sha256").update(token).digest("hex") };
}

/** Seeds a job that entered `status` `since` ago (a Postgres interval) with a live link token;
 * returns the token's hash and the page path. */
function seedJob(status: string, since = "0 minutes"): { hash: string; path: string } {
  const { token, hash } = newToken();
  psql(`WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('${status}', false, now() - interval '${since}', now() - interval '${since}',
        now() - interval '${since}') RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j)
    INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('${hash}', 'hex'), now() FROM j`);
  return { hash, path: `/f/${token}` };
}

/** The stored feedback of the job behind `hash`, as "share|body", or "" when none. */
function stored(hash: string): string {
  return psql(`SELECT f.share || '|' || f.body FROM jobs.feedback f
    JOIN jobs.quote_token t ON t.job_id = f.job_id WHERE t.token_hash = decode('${hash}', 'hex')`);
}

/** Re-sends every POST with a same-origin https Origin (see the header and D-1250). */
async function httpsOrigin(page: Page): Promise<void> {
  await page.route("**/f/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const origin = new URL(request.url()).host;
    const headers = { ...request.headers(), origin: `https://${origin}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

async function noAllFeedbackClaim(page: Page): Promise<void> {
  const text = (await page.locator("body").innerText()).toLowerCase();
  for (const banned of ["all feedback", "all reviews", "every review", "reward", "discount", "gift card"]) {
    expect(text, banned).not.toContain(banned);
  }
}

test("TD-JOB-10 JOB-017: the share box is one unticked box with the JOB-017 label, and the page offers no reward", async ({ page }) => {
  const { path } = seedJob("done");
  await page.goto(path);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^How did request \d+ go\?$/);
  const share = page.getByRole("checkbox", { name: LABEL, exact: true });
  await expect(share).not.toBeChecked();
  await expect(page.getByRole("checkbox")).toHaveCount(1);
  await expect(page.locator("main")).toContainText("never shown on quote cards");
  await noAllFeedbackClaim(page);
});

test("TD-JOB-10 JOB-017: ticked negative feedback is stored with its tick as sent, then the thanks shows once", async ({ page }) => {
  const { hash, path } = seedJob("done");
  await httpsOrigin(page);
  await page.goto(path);
  await page.getByLabel("Your feedback").fill("Sample: the visit was late and the work was poor.");
  await page.getByRole("checkbox", { name: LABEL, exact: true }).check();
  await page.getByRole("button", { name: "Send feedback" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Thanks\. We got your feedback/);
  expect(stored(hash)).toBe("true|Sample: the visit was late and the work was poor.");
  await noAllFeedbackClaim(page);
  await page.goto(path);
  await expect(page.getByRole("button", { name: "Send feedback" })).toHaveCount(0);
});

test("TD-JOB-10 JOB-017: unticked feedback is stored private; an empty send keeps the tick and says why", async ({ page }) => {
  const { hash, path } = seedJob("done");
  await httpsOrigin(page);
  await page.goto(path);
  await page.getByRole("checkbox", { name: LABEL, exact: true }).check();
  await page.getByRole("button", { name: "Send feedback" }).click();
  await expect(page).toHaveTitle(/^Error: /);
  await expect(page.getByRole("alert")).toContainText("Please write something before you send.");
  await expect(page.getByRole("checkbox", { name: LABEL, exact: true })).toBeChecked();
  expect(stored(hash)).toBe("");
  await page.getByRole("checkbox", { name: LABEL, exact: true }).uncheck();
  await page.getByLabel("Your feedback").fill("Sample: fine overall.");
  await page.getByRole("button", { name: "Send feedback" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Thanks\./);
  expect(stored(hash)).toBe("false|Sample: fine overall.");
});

test("TD-JOB-10 JOB-017: the page opens only after Done; any other status or a bad link is a 404", async ({ page }) => {
  for (const status of ["connected", "closed"]) {
    const { path } = seedJob(status);
    expect((await page.goto(path))?.status(), status).toBe(404);
  }
  expect((await page.goto(`/f/${"0".repeat(64)}`))?.status()).toBe(404);
});

test("TD-JOB-10 JOB-017, PICK-011: the page closes 30 days after Done; a revoked or re-issued link is a 404", async ({ page }) => {
  expect((await page.goto(seedJob("done", "29 days 23 hours").path))?.status()).toBe(200);
  expect((await page.goto(seedJob("done", "30 days 1 minute").path))?.status()).toBe(404);
  const revoked = seedJob("done");
  psql(`UPDATE jobs.quote_token SET revoked_at = now() WHERE token_hash = decode('${revoked.hash}', 'hex')`);
  expect((await page.goto(revoked.path))?.status()).toBe(404);
  const old = seedJob("done");
  const fresh = newToken();
  psql(`WITH r AS (UPDATE jobs.quote_token SET revoked_at = now()
      WHERE token_hash = decode('${old.hash}', 'hex') RETURNING job_id)
    INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT job_id, decode(md5(random()::text), 'hex'), 1, decode('${fresh.hash}', 'hex'), now() FROM r`);
  expect((await page.goto(old.path))?.status(), "re-issued").toBe(404);
  expect((await page.goto(`/f/${fresh.token}`))?.status(), "new link").toBe(200);
});

test("TD-JOB-10 JOB-017: a removal is never changed or cleared, and an author mark is never cleared", async () => {
  const { hash } = seedJob("done");
  const id = psql(`INSERT INTO jobs.feedback (job_id, body, share, received_at)
    SELECT job_id, 'Sample words', true, now() FROM jobs.quote_token
    WHERE token_hash = decode('${hash}', 'hex') RETURNING id`).split("\n")[0];
  const set = (columns: string): string => psql(`UPDATE jobs.feedback SET ${columns} WHERE id = '${id}'`);
  set("removed_reason = 'fake', removed_at = now(), author_kind = 'owner'");
  expect(() => set("removed_reason = NULL, removed_at = NULL")).toThrow(/never changed or cleared/);
  expect(() => set("removed_reason = 'abusive'")).toThrow(/never changed or cleared/);
  set("author_kind = 'business'");
  expect(() => set("author_kind = NULL")).toThrow(/never cleared/);
  expect(psql(`SELECT removed_reason || '|' || author_kind FROM jobs.feedback WHERE id = '${id}'`)).toBe("fake|business");
});

test("TD-JOB-10 (axe part): the feedback form, its error and the thanks have no axe violation at 320, 768 and 1280 px", async ({ page }) => {
  const { path } = seedJob("done");
  await httpsOrigin(page);
  await page.goto(path);
  const scan = async (state: string): Promise<void> => {
    for (const width of WIDTHS) {
      await page.setViewportSize({ width, height: 900 });
      const { violations } = await axeScan(page);
      expect(violations, `${state} at ${String(width)} px`).toEqual([]);
    }
  };
  await scan("form");
  await page.getByRole("button", { name: "Send feedback" }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  await scan("error");
  await page.getByLabel("Your feedback").fill("Sample: good work.");
  await page.getByRole("button", { name: "Send feedback" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Thanks\./);
  await scan("thanks");
});
