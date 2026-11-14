/// <reference types="node" />
// The keyed web binary works for a browser (T452, D-3674; SEC-004, PRIV-002, A11Y-002).
//
// `webserver.ts` starts a second web binary with fake Verify on the stand-in and its own vault keys;
// `keyed.ts` seals in the vault's format. The first test seeds, through psql on DATABASE_URL as
// tests/e2e/j1_reduced.spec.ts does, a fake active business whose owner has a texting staff phone
// (555-01xx, sealed with `seal` into `privacy.staff_phone`) and a sign-in link for a token chosen
// here; pressing Sign in on the keyed binary sends a Verify code to that phone (the stand-in's code
// hook answers for it) and typing it signs the browser in. The second proves a cookie an Ally seed
// returns reaches the keyed binary on the next request. POSTs are re-sent with a same-origin https
// Origin, as the J1 specs do (D-1250). Fake data only; no key or value is printed or logged.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { open, seededUrl } from "../a11y/fixtures";
import { test, expect } from "./fixtures";
import { keyedUrl, seal } from "./keyed";

/** The owner's fake texting phone, as the vault stores it (`+1` and ten digits). */
const PHONE = "+19285550142";

/** An active business, its owner, the owner's texting line (sealed) and a sign-in link for `token`. */
function seedOwnerWithPhone(token: string): void {
  const line = randomUUID();
  const sealed = seal("privacy.staff_phone", "number", line, PHONE).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  const sql = `WITH b AS (INSERT INTO businesses.business (status, created_at, status_changed_at)
      VALUES ('active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id),
    p AS (INSERT INTO businesses.staff_phone (id, business_id, linked_by, linked_at)
      SELECT '${line}', business_id, id, now() FROM m RETURNING id),
    c AS (INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version,
      consented_at, line_id) SELECT m.business_id, m.id, 'text', 'con019-v1', now(), p.id FROM m, p),
    v AS (INSERT INTO privacy.staff_phone (line_id, number) VALUES ('${line}', decode('${sealed}', 'hex')))
    INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
    SELECT decode('${hash}', 'hex'), id, business_id, now(), now() + interval '15 minutes' FROM m`;
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding the business failed with status ${String(done.status)}`);
}

/** Re-sends every sign-in POST with a same-origin https Origin (D-1250). */
async function httpsOrigin(page: Page): Promise<void> {
  await page.route("**/b/sign-in/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const headers = { ...request.headers(), origin: `https://${new URL(request.url()).host}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

/** The code the Verify stand-in sent to `phone`, from its test hook standing in for the phone. */
async function verifyCode(phone: string): Promise<string> {
  const reply = await fetch(`http://127.0.0.1:4010/_stand_in/twilio/code?to=${encodeURIComponent(phone)}`);
  expect(reply.status, "a Verify code went to the owner's texting phone").toBe(200);
  return ((await reply.json()) as { code: string }).code;
}

test("SEC-004 D-3674: on the keyed binary a sign-in link sends a Verify code to the owner's sealed texting phone, and the code signs the browser in", async ({ page }) => {
  const token = randomBytes(32).toString("hex");
  seedOwnerWithPhone(token);
  await httpsOrigin(page);
  await page.goto(`${keyedUrl()}/b/sign-in/${token}`);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Enter your sign-in code");
  await expect(page.locator("#code-hint")).toHaveText("We texted a 6-digit code to your phone.");
  await page.getByLabel("Sign-in code").fill(await verifyCode(PHONE));
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("You're signed in");
  const names = (await page.context().cookies(keyedUrl())).map((cookie) => cookie.name);
  expect(names).toContain("__Host-askhava-session");
});

test("A11Y-002 D-3674: a cookie an Ally seed returns reaches the keyed binary on the next request", async ({ page }) => {
  const value = randomBytes(16).toString("hex");
  const url = seededUrl({ url: `${keyedUrl()}/b/sign-in`, cookies: [{ name: "__Host-askhava-probe", value }] });
  const sent = page.waitForRequest((request) => request.url() === url);
  await open(page, url, 1280);
  const cookie = (await (await sent).allHeaders()).cookie ?? "";
  expect(cookie.split("; ")).toContain(`__Host-askhava-probe=${value}`);
});
