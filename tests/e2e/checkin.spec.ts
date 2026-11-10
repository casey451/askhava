/// <reference types="node" />
// The check-in answer page the emailed check-in's buttons open, served (T231; TD-TXT-31; TXT-034,
// SEC-026; D-493, D-2128, D-2414 to D-2417).
//
// A Connected job with a live link token is seeded through psql on DATABASE_URL, as
// feedback.spec.ts does; each button opens /c/<token>/<day>-<answer>. A mail scanner's GET (twice)
// must change nothing; only the page's POST with its CSRF token records the answer, once. Under
// the site's `Referrer-Policy: no-referrer` the browser sends `Origin: null` on a same-origin form
// POST, which `security::is_cross_site` refuses (D-1250), so `page.route` re-sends the page's POST
// with the https Origin a same-origin request has. The email itself (one button per answer, zero
// texts) is checked in app/messaging/src/checkin_email.rs. Fake data only.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { test, expect } from "./fixtures";

function psql(sql: string): string {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-t", "-A", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed: ${done.stderr}`);
  return done.stdout.trim();
}

/** Seeds a job in `status` with a live link token; returns the token's hash and the token. */
function seedJob(status: string): { hash: string; token: string } {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  psql(`WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('${status}', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j)
    INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('${hash}', 'hex'), now() FROM j`);
  return { hash, token };
}

/** The status and version of the job behind `hash`, as "status|version". */
function jobState(hash: string): string {
  return psql(`SELECT j.status || '|' || j.version FROM jobs.job j
    JOIN jobs.quote_token t ON t.job_id = j.id WHERE t.token_hash = decode('${hash}', 'hex')`);
}

/** Re-sends every POST with a same-origin https Origin (see the header and D-1250). */
async function httpsOrigin(page: Page): Promise<void> {
  await page.route("**/c/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const origin = new URL(request.url()).host;
    const headers = { ...request.headers(), origin: `https://${origin}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

test("TD-TXT-31 TXT-034, SEC-026: a button opened by GET twice changes nothing and still works; only the POST records, once", async ({ page }) => {
  const { hash, token } = seedJob("connected");
  const before = jobState(hash);
  const path = `/c/${token}/3-done`;
  for (let i = 0; i < 2; i += 1) {
    const reply = await page.goto(path);
    expect(reply?.status()).toBe(200);
    expect(reply?.headers()["cache-control"]).toContain("no-store");
  }
  expect(jobState(hash), "GET changes nothing").toBe(before);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Send your answer for request \d+$/);
  await expect(page.locator("main")).toContainText("You picked: The work is done");
  await httpsOrigin(page);
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Thanks\. We got your answer for request \d+\.$/);
  expect(jobState(hash).split("|")[0]).toBe("done");
  const after = jobState(hash);
  await page.goto(path);
  await expect(page.getByRole("button", { name: "Send" })).toHaveCount(0);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^There's nothing to send for request \d+$/);
  expect(jobState(hash), "recorded once").toBe(after);
});

test("TD-TXT-31 SEC-026: a cross-site POST or a wrong CSRF token is refused and changes nothing", async ({ page, request }) => {
  const { hash, token } = seedJob("connected");
  const before = jobState(hash);
  const path = `/c/${token}/3-done`;
  await page.goto(path);
  const csrf = await page.locator("input[name=csrf]").getAttribute("value");
  const cookies = (await page.context().cookies()).map((c) => `${c.name}=${c.value}`).join("; ");
  const host = new URL(page.url()).host;
  const tries = [
    { site: "cross-site", origin: "https://elsewhere.example", csrf: csrf ?? "" },
    { site: "same-origin", origin: `https://${host}`, csrf: "0".repeat(64) },
  ];
  for (const t of tries) {
    const reply = await request.post(path, {
      headers: { cookie: cookies, origin: t.origin, "sec-fetch-site": t.site, "content-type": "application/x-www-form-urlencoded" },
      data: `csrf=${t.csrf}`,
      maxRedirects: 0,
    });
    expect(reply.status(), t.site).toBe(403);
    expect(reply.headers()["cache-control"]).toContain("no-store");
  }
  expect(jobState(hash)).toBe(before);
});

test("TD-TXT-31 SEC-037: a bad link or an unknown answer code is a 404", async ({ page }) => {
  const { token } = seedJob("connected");
  for (const path of [`/c/${token}/3-problem`, `/c/${token}/7-done`, `/c/${"0".repeat(64)}/3-done`, "/c/short/3-done"]) {
    expect((await page.goto(path))?.status(), path).toBe(404);
  }
});

test("TD-TXT-31 (axe part): the answer page and its thanks have no axe violation at 320, 768 and 1280 px", async ({ page }) => {
  const { token } = seedJob("connected");
  await httpsOrigin(page);
  await page.goto(`/c/${token}/14-later`);
  const scan = async (state: string): Promise<void> => {
    for (const width of WIDTHS) {
      await page.setViewportSize({ width, height: 900 });
      const { violations } = await axeScan(page);
      expect(violations, `${state} at ${String(width)} px`).toEqual([]);
    }
  };
  await scan("answer");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Thanks\./);
  await scan("thanks");
});
