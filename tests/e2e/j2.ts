/// <reference types="node" />
// Journey J2's shared helpers (T099, D-3863; BIZ-002, BIZ-016, BIZ-040, PICK-022, FEE-010). No tests.
//
// The served business sign-up on the keyed binary (tests/e2e/keyed.ts, T452), as
// business_signup_served.spec.ts drives it: a fake R-37 plumber on the fixture ROC record ROC100001
// (record phone the fake 928-555-0142). The identity code is the Verify stand-in's (`stand_ins/twilio.py`
// `start_verification`: the sum of the E.164 number's character codes modulo 1,000,000, six digits);
// the emailed code is computed as `code_of` makes it (app/businesses/src/email_verification.rs, over
// `IdempotencyKey::derive`) from the open `businesses.mailbox_check` row. Then the readers of J2's
// closing checks: the release row (`picks.release`), the business notices (`core.task` of kind
// `messaging.business_notice`, done rows kept, migrations/0020), the job's open masked line
// (`calls.number_pool`), the ledger, the Stripe stand-in's charges and events, and the Twilio
// stand-in's call log (`GET .../Calls.json`, its account SID as the Basic user). Every SQL statement
// goes through psql on DATABASE_URL (role askhava_ci). Fake data only; no key or value is printed.
import { createHmac, randomBytes } from "node:crypto";
import { expect, type APIRequestContext, type Page } from "@playwright/test";
import { keyedIdSecret, keyedUrl, psql, stripeDelivery } from "./keyed";

/** `payments::api::AUTHORIZATION_VERSION`, the least version a Confirm accepts (keyed.ts). */
const AUTHORIZATION_VERSION = 2;
/** The fixture ROC record's fake phone, the one business phone J2 knows (TD-BIZ-22, TD-BIZ-25). */
export const RECORD_PHONE = "+19285550142";
/** The stand-in server's origin (tests/e2e/webserver.ts `STAND_IN_PORT`). */
export const STAND_IN = "http://127.0.0.1:4010";
/** Any `sk_test_` key: the Stripe stand-in checks only the prefix. */
const STRIPE_AUTH = { authorization: `Bearer sk_test_${"e2e".repeat(4)}` };
/** The Twilio stand-in's account SID (stand_ins/twilio.py), as the Basic user it asks for. */
const TWILIO_SID = `AC${"0".repeat(32)}`;

/** Stores the fixture's ROC row as today's list when no ROC list is stored yet (LIC-007). */
export function storeRocList(): void {
  psql(`WITH i AS (INSERT INTO licensing.list_import (source, list_date, rows_recorded, rows_skipped)
      SELECT 'roc', current_date, 1, 0 WHERE NOT EXISTS (SELECT 1 FROM licensing.list_import WHERE source = 'roc')
      RETURNING id)
    INSERT INTO licensing.agency_record (import_id, source, list_date, license_number, class_code, status, issued_on)
    SELECT id, 'roc', current_date, 'ROC100001', 'R-37', 'active', '2015-06-01' FROM i`);
}

/** The Verify stand-in's code for `to` (`stand_ins/twilio.py` `start_verification`). */
export function standInCode(to: string): string {
  let sum = 0;
  for (let i = 0; i < to.length; i += 1) sum += to.charCodeAt(i);
  return String(sum % 1_000_000).padStart(6, "0");
}

/** `IdempotencyKey::derive`: HMAC-SHA256 under `secret` over each part's 8-byte length and bytes. */
function derive(secret: string, parts: readonly string[]): string {
  const mac = createHmac("sha256", secret);
  for (const part of parts) {
    const bytes = Buffer.from(part, "utf8");
    const length = Buffer.alloc(8);
    length.writeBigUInt64BE(BigInt(bytes.length));
    mac.update(length).update(bytes);
  }
  return mac.digest("hex");
}

/** The emailed code of `business`'s open mailbox check, as `code_of` makes it (BIZ-040). */
export function emailedCode(business: string): string {
  const row = psql(`SELECT id || '|' || floor(extract(epoch FROM sent_at))::bigint FROM businesses.mailbox_check
    WHERE business_id = '${business}' AND open`);
  const [id = "", sent = ""] = row.split("|");
  const key = derive(keyedIdSecret(), ["businesses.mailbox_code", id, sent]);
  return String(BigInt(`0x${key.slice(0, 12)}`) % 1_000_000n).padStart(6, "0");
}

/** The business the browser's sign-up binding cookie names (`<sign-up ID>.<salt>.<MAC>`). */
export async function boundBusiness(page: Page): Promise<string> {
  const cookies = await page.context().cookies(keyedUrl());
  const binding = cookies.find((cookie) => cookie.name === "__Host-askhava-signup");
  return binding?.value.split(".")[0] ?? "";
}

/** A fresh fake sign-up email address. */
export const signUpEmail = (): string => `owner-${randomBytes(4).toString("hex")}@example.test`;

/**
 * J2 step 1 and 2: the sign-up form (its E-SIGN box unticked at first, CON-019), the identity code
 * to the record's phone, the service area and the emailed code, until "You're signed up" (BIZ-002,
 * BIZ-016, BIZ-040); returns the now active business.
 */
export async function signUp(page: Page): Promise<string> {
  storeRocList();
  await page.goto(`${keyedUrl()}/b/signup`);
  await expect(page.locator("#consent"), "CON-019: the E-SIGN box starts unticked").not.toBeChecked();
  await page.getByLabel("Business name").fill("Example Plumbing Co");
  await page.getByLabel("ROC license number, if you have one").fill("ROC100001");
  await page.getByLabel("Email for job requests and picks").fill(signUpEmail());
  await page.locator("#cat-plumbing_repair").check();
  await page.locator("#consent").check();
  await page.getByRole("button", { name: "Next: get your code" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Where your code goes");
  await page.getByRole("button", { name: "Send the code" }).click();
  await page.getByLabel("6-digit code").fill(standInCode(RECORD_PHONE));
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("How far do you travel for jobs?");
  await page.getByLabel("Base ZIP code").fill("86403");
  await page.locator("#radius").fill("25");
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Check your email for a code");
  const business = await boundBusiness(page);
  await page.getByLabel("6-digit code").fill(emailedCode(business));
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("You're signed up");
  expect(psql(`SELECT status FROM businesses.business WHERE id = '${business}'`), "BIZ-016: active").toBe("active");
  return business;
}

/** FEE-010 (D-3863): moves `business`'s join date (`businesses.signup.submitted_at`) to `at`. */
export function joinedAt(business: string, at: string): void {
  psql(`UPDATE businesses.signup SET submitted_at = '${at}' WHERE business_id = '${business}'`);
}

/**
 * FEE-010 (D-3857): reads the plumbing category as switched on before the launch date, as
 * migrations/0190 backfills a category already on; on a fresh database the catalog load switches
 * every category on at that moment, which would give every sign-up a free first win for 90 days.
 */
export function plumbingOnBeforeLaunch(): void {
  psql(`UPDATE catalog.category SET switched_on_at = '1970-01-01T00:00:00Z'
    WHERE code = 'plumbing_repair' AND switched_on_at > '2026-01-15T07:00:00Z'`);
}

/**
 * FEE-015: the owner accepts the card authorization the Confirm page shows (its words after a $0
 * page's "Next time it's one tap."), as keyed.ts `openAsOwner` stores it for a paid one. A $0
 * Confirm makes no Stripe call, so it never makes the business's Stripe customer, which the
 * authorization row names: a fake `cus_` ID stands in for it.
 */
export async function acceptAuthorization(page: Page, business: string): Promise<void> {
  const shown = (await page.locator("#save-card-text").textContent()) ?? "";
  const text = shown.replace(/^Next time it's one tap\. /, "").replaceAll("'", "''");
  psql(`WITH c AS (INSERT INTO payments.customer (business_id, stripe_customer_id)
      VALUES ('${business}', 'cus_j2${randomBytes(8).toString("hex")}') ON CONFLICT (business_id) DO NOTHING)
    INSERT INTO payments.card_authorization (business_id, version, text, accepted_at)
    SELECT '${business}', GREATEST(authorization_version, ${String(AUTHORIZATION_VERSION)}), '${text}', now() FROM payments.fee_terms`);
}

/** The count `sql` returns. */
const count = (sql: string): number => Number(psql(sql));

/** What J2's closing checks read about `pick` on `job` of `business`. */
export interface Closing { releases: number; basis: string; notices: number; lines: number; charges: number; ledger: string }

/** J2's closing-check reads (PICK-022, PICK-024, CALL-001, FEE-016): rows the release and the fee left. */
export function closing(business: string, pick: string, job: string): Closing {
  return {
    releases: count(`SELECT count(*) FROM picks.release WHERE pick_id = '${pick}'`),
    basis: psql(`SELECT string_agg(basis, ',') FROM picks.release WHERE pick_id = '${pick}'`),
    notices: count(`SELECT count(*) FROM core.task WHERE kind = 'messaging.business_notice' AND payload->>'pick' = '${pick}'`),
    lines: count(`SELECT count(*) FROM calls.number_pool WHERE job_id = '${job}' AND business_id = '${business}' AND closed_at IS NULL`),
    charges: count(`SELECT count(*) FROM payments.ledger_entry WHERE job_id = '${job}' AND kind = 'charge'`),
    ledger: psql(`SELECT string_agg(kind || ':' || amount_cents, ',' ORDER BY seq) FROM payments.ledger_entry
      WHERE job_id = '${job}' AND business_id = '${business}'`),
  };
}

/** The amounts in cents of the Stripe stand-in's succeeded charges naming `pick` (FEE-016). */
export async function standInCharges(request: APIRequestContext, pick: string): Promise<number[]> {
  const reply = await request.get(`${STAND_IN}/stripe/v1/charges?created[gte]=0&limit=100`, { headers: STRIPE_AUTH });
  const listed = (await reply.json()) as { data: { metadata?: { pick_id?: string }; status: string; amount: number }[] };
  return listed.data.filter((c) => c.metadata?.pick_id === pick && c.status === "succeeded").map((c) => c.amount);
}

interface StandInEvent { data: { object: { metadata?: { pick_id?: string } } } }

/**
 * D-3862: the stand-in's `payment_intent.succeeded` for `pick` from `GET /stripe/v1/events`,
 * delivered to the keyed binary's `/webhooks/stripe` signed with the run's webhook secret
 * (`stripeDelivery`); returns the webhook's status.
 */
export async function deliverSuccess(request: APIRequestContext, pick: string): Promise<number> {
  const listed = await request.get(`${STAND_IN}/stripe/v1/events?type=payment_intent.succeeded`, { headers: STRIPE_AUTH });
  const event = ((await listed.json()) as { data: StandInEvent[] }).data.find((e) => e.data.object.metadata?.pick_id === pick);
  if (event === undefined) throw new Error("the stand-in emitted no payment_intent.succeeded for the pick");
  const payload = JSON.stringify(event);
  const headers = stripeDelivery(payload, Math.floor(Date.now() / 1000));
  return (await request.post(`${keyedUrl()}/webhooks/stripe`, { headers, data: payload, maxRedirects: 0 })).status();
}

/** TD-BIZ-22, TD-BIZ-25: the Twilio stand-in's recorded calls to any of `phones`, as "to status". */
export async function callsTo(request: APIRequestContext, phones: readonly string[]): Promise<string[]> {
  const basic = Buffer.from(`${TWILIO_SID}:fake`).toString("base64");
  const reply = await request.get(`${STAND_IN}/twilio/2010-04-01/Accounts/${TWILIO_SID}/Calls.json`, {
    headers: { authorization: `Basic ${basic}` },
  });
  expect(reply.status(), "the stand-in's call log").toBe(200);
  const calls = ((await reply.json()) as { calls: { to: string; status: string }[] }).calls;
  return calls.filter((c) => phones.includes(c.to)).map((c) => `${c.to} ${c.status}`);
}
