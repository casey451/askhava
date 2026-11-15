/// <reference types="node" />
// TD-BIZ-42's browser half: the served business portal end to end on T452's keyed web binary
// (T454, D-3674, D-2117; BIZ-013, BIZ-015, BIZ-025, BIZ-036, PICK-024, CARD-022).
//
// T085's tests/e2e/business_portal.spec.ts is not changed. This spec seeds through psql on
// DATABASE_URL, as tests/e2e/j1_reduced.spec.ts seeds its quote token: an active business, its
// owner with a linked fake texting phone (555-01xx) sealed with `seal` into privacy.staff_phone, a
// released free pick whose homeowner contact (fake name, phone and street) is sealed the same way
// into privacy.contact, and a portals.login_link row for a token chosen here. The owner signs in
// through the served sign-in pages with no email read: the new-device code is computed here from
// the Verify stand-in's rule (stand_ins/twilio.py `start_verification`: the sum of the E.164
// number's character codes modulo 1,000,000, six digits). Then the released job's page shows
// PICK-024's tip line once and no phone-shaped number, and the home, billing and notice-settings
// pages load. POSTs are re-sent with a same-origin https Origin, as the J1 specs do (D-1250). No
// test-control route; fake data only; no key or value is printed or logged.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { keyedUrl, seal } from "./keyed";

/** The owner's fake texting phone, as the vault stores it (`+1` and ten digits). */
const OWNER_PHONE = "+19285550143";
/** The homeowner's fake contact; her phone must never show on the business's pages. */
const HER = { name: "Dana Testperson", phone: "+19285550187", address: "300 Fake Lane" };
/** PICK-024's tip line (portals' `TIP_LINE`). */
const TIP_LINE = "Tip: on your first call, ask which number to text.";
/** A US phone number in any usual written shape. */
const PHONE_SHAPED = /(\+?1[-. ]?)?\(?\d{3}\)?[-. ]?\d{3}[-. ]?\d{4}/;

/** The code the Verify stand-in issues for `phone` (stand_ins/twilio.py `start_verification`). */
function verifyCode(phone: string): string {
  let sum = 0;
  for (let i = 0; i < phone.length; i += 1) sum += phone.charCodeAt(i);
  return String(sum % 1_000_000).padStart(6, "0");
}

/** `value` sealed for `column` of row `id` of `table`, as a SQL bytea literal. */
function sealed(table: string, column: string, id: string, value: string): string {
  return `decode('${seal(table, column, id, value).toString("hex")}', 'hex')`;
}

/** Runs `sql` through psql on DATABASE_URL; `what` names it in the error. */
function psql(sql: string, what: string): void {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding ${what} failed with status ${String(done.status)}`);
}

/** An active business, its owner and texting line (sealed) and a sign-in link for `token`. */
function seedOwner(token: string, ids: { business: string; owner: string }): void {
  const line = randomUUID();
  const hash = createHash("sha256").update(token).digest("hex");
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${ids.business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT '${ids.owner}', id, gen_random_uuid(), true, true, true, now()
      FROM b RETURNING id, business_id),
    p AS (INSERT INTO businesses.staff_phone (id, business_id, linked_by, linked_at)
      SELECT '${line}', business_id, id, now() FROM m RETURNING id),
    c AS (INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version,
      consented_at, line_id) SELECT m.business_id, m.id, 'text', 'con019-v1', now(), p.id FROM m, p),
    v AS (INSERT INTO privacy.staff_phone (line_id, number)
      VALUES ('${line}', ${sealed("privacy.staff_phone", "number", line, OWNER_PHONE)}))
    INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
    SELECT decode('${hash}', 'hex'), id, business_id, now(), now() + interval '15 minutes' FROM m`, "the business");
}

/** A connected job, the business's free page pick on it and its release bound to her sealed contact. */
function seedReleasedPick(business: string): string {
  const [pick, contact] = [randomUUID(), randomUUID()];
  const field = (column: string, value: string) => sealed("privacy.contact", column, contact, value);
  psql(`WITH c AS (INSERT INTO privacy.contact (id, name, phone, address)
      VALUES ('${contact}', ${field("name", HER.name)}, ${field("phone", HER.phone)}, ${field("address", HER.address)})),
    j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('connected', false, now(), now(), now()) RETURNING id),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT '${pick}', id, '${business}', 1, 'page', 'free', now() FROM j RETURNING id, job_id)
    INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen)
    SELECT id, job_id, '${business}', 'no_fee', now(), '${contact}', false FROM p`, "the released pick");
  return pick;
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

/** Signs the owner in through the served sign-in pages with the stand-in Verify's code. */
async function signIn(page: Page, token: string): Promise<void> {
  await httpsOrigin(page);
  await page.goto(`${keyedUrl()}/b/sign-in/${token}`);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Enter your sign-in code");
  await page.getByLabel("Sign-in code").fill(verifyCode(OWNER_PHONE));
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("You're signed in");
}

test("TD-BIZ-42: the owner signs in on the keyed binary with no email read, sees the released job's tip line once and no phone, then home, billing and notice settings", async ({ page }) => {
  const token = randomBytes(32).toString("hex");
  const ids = { business: randomUUID(), owner: randomUUID() };
  seedOwner(token, ids);
  const pick = seedReleasedPick(ids.business);
  await signIn(page, token);

  await page.goto(`${keyedUrl()}/b/picks/${pick}`);
  const job = page.locator("main");
  await expect(job).toContainText(TIP_LINE);
  const text = await job.innerText();
  expect(text.split(TIP_LINE).length - 1, "PICK-024's tip line once").toBe(1);
  expect(text.split("Dana").length - 1, "her first name once").toBe(1);
  expect(text, "no phone-shaped number").not.toMatch(PHONE_SHAPED);
  expect(text).not.toContain("Testperson");

  await page.goto(`${keyedUrl()}/b`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Your business account");
  await expect(page.getByRole("heading", { name: "Your numbers this month" })).toBeVisible();
  await page.goto(`${keyedUrl()}/b/billing`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Billing");
  await expect(page.getByRole("link", { name: /Download this statement/ })).toHaveAttribute("href", "/b/billing/statement.csv");
  await page.goto(`${keyedUrl()}/b/notices`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Where we send your job notices");
});
