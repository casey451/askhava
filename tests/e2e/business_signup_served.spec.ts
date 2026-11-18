/// <reference types="node" />
// The served business sign-up, end to end in a browser (T464, TD-SEC-69; BIZ-002, BIZ-016, BIZ-040,
// BIZ-049; D-3676).
//
// On the keyed web binary (tests/e2e/keyed.ts, T452), a fake R-37 plumber signs up on the fixture
// ROC record ROC100001 (tests/fixtures/license_source, T385's `file://` directory; its record phone
// is the fake 928-555-0142), through every step and activation. The identity code is the one the
// Verify stand-in makes for the record's phone (`stand_ins/twilio.py` `start_verification`: the sum
// of the character codes of the E.164 number, modulo 1,000,000, six digits). The emailed code is
// never stored, so it is computed here as the app computes it (`code_of`,
// app/businesses/src/email_verification.rs, over `IdempotencyKey::derive`, app/adapters/src/base.rs)
// with the keyed binary's ID secret, from the open `businesses.mailbox_check` row's ID and its
// `sent_at` in Unix seconds read through psql on DATABASE_URL, and typed into the real email step,
// so activation runs through the app's `confirm` and `settle`. Then the owner signs in through a
// link seeded as tests/e2e/keyed_server.spec.ts seeds one (with a fake texting line for the
// new-device code), the done screen links to the capability step, the step is saved, and the
// inbox check opens. When no ROC list is stored yet, the fixture's one row is stored first. POSTs
// go with the browser's own headers (D-4292). Fake data only; no key
// or value is printed or logged.
import { createHash, createHmac, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { keyedIdSecret, keyedUrl, seal } from "./keyed";

/** The fixture ROC record's fake phone, as Verify gets it. */
const RECORD_PHONE = "+19285550142";

/** The owner's fake texting line for the sign-in's new-device code. */
const OWNER_PHONE = "+19285550187";

/** Runs `sql` through psql on DATABASE_URL and returns its unaligned, tuples-only output. */
function psql(sql: string): string {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-A", "-t", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed with status ${String(done.status)}`);
  return done.stdout.trim();
}

/** Stores the fixture's ROC row as today's list when no ROC list is stored yet (LIC-007). */
function storeRocList(): void {
  psql(`WITH i AS (INSERT INTO licensing.list_import (source, list_date, rows_recorded, rows_skipped)
      SELECT 'roc', current_date, 1, 0 WHERE NOT EXISTS (SELECT 1 FROM licensing.list_import WHERE source = 'roc')
      RETURNING id)
    INSERT INTO licensing.agency_record (import_id, source, list_date, license_number, class_code, status, issued_on)
    SELECT id, 'roc', current_date, 'ROC100001', 'R-37', 'active', '2015-06-01' FROM i`);
}

/** The Verify stand-in's code for `to` (`stand_ins/twilio.py` `start_verification`). */
function standInCode(to: string): string {
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
function emailedCode(business: string): string {
  const row = psql(`SELECT id || '|' || floor(extract(epoch FROM sent_at))::bigint FROM businesses.mailbox_check
    WHERE business_id = '${business}' AND open`);
  const [id = "", sent = ""] = row.split("|");
  const key = derive(keyedIdSecret(), ["businesses.mailbox_code", id, sent]);
  return String(BigInt(`0x${key.slice(0, 12)}`) % 1_000_000n).padStart(6, "0");
}

/** The owner of `business` gets a fake texting line (sealed) and a sign-in link for `token`. */
function seedSignInLink(business: string, token: string): void {
  const line = randomUUID();
  const sealed = seal("privacy.staff_phone", "number", line, OWNER_PHONE).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  psql(`WITH m AS (SELECT id, business_id FROM businesses.team_member WHERE business_id = '${business}' AND is_owner),
    p AS (INSERT INTO businesses.staff_phone (id, business_id, linked_by, linked_at)
      SELECT '${line}', business_id, id, now() FROM m RETURNING id),
    c AS (INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version,
      consented_at, line_id) SELECT m.business_id, m.id, 'text', 'con019-v1', now(), p.id FROM m, p),
    v AS (INSERT INTO privacy.staff_phone (line_id, number) VALUES ('${line}', decode('${sealed}', 'hex')))
    INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
    SELECT decode('${hash}', 'hex'), id, business_id, now(), now() + interval '15 minutes' FROM m`);
}

/** The code the Verify stand-in sent to `phone`, from its test hook standing in for the phone. */
async function verifyCode(phone: string): Promise<string> {
  const reply = await fetch(`http://127.0.0.1:4010/_stand_in/twilio/code?to=${encodeURIComponent(phone)}`);
  expect(reply.status, "a Verify code went to the owner's texting line").toBe(200);
  return ((await reply.json()) as { code: string }).code;
}

/** The business the browser's sign-up binding cookie names (`<sign-up ID>.<salt>.<MAC>`). */
async function boundBusiness(page: Page): Promise<string> {
  const cookies = await page.context().cookies(keyedUrl());
  const binding = cookies.find((cookie) => cookie.name === "__Host-askhava-signup");
  return binding?.value.split(".")[0] ?? "";
}

/** The form, the identity code and the service area, up to the email code step (BIZ-002, BIZ-016). */
async function signUpToEmailStep(page: Page): Promise<void> {
  await page.goto(`${keyedUrl()}/b/signup`);
  await page.getByLabel("Business name").fill("Example Plumbing Co");
  await page.getByLabel("ROC license number, if you have one").fill("ROC100001");
  await page.getByLabel("Email for job requests and picks").fill(`owner-${randomBytes(4).toString("hex")}@example.test`);
  await page.locator("#cat-plumbing_repair").check();
  await page.locator("#consent").check();
  await page.getByRole("button", { name: "Next: get your code" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Where your code goes");
  await page.getByRole("button", { name: "Send the code" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Show this business is yours");
  await page.getByLabel("6-digit code").fill(standInCode(RECORD_PHONE));
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("How far do you travel for jobs?");
  await page.getByLabel("Base ZIP code").fill("86403");
  await page.locator("#radius").fill("25");
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Check your email for a code");
}

test("TD-SEC-69: a fake R-37 sign-up on the keyed binary activates through the app, and the first sign-in leads to the capability step and the inbox check", async ({ page }) => {
  storeRocList();
  await signUpToEmailStep(page);
  const business = await boundBusiness(page);
  expect(business, "the binding cookie names the sign-up").toMatch(/^[0-9a-f-]{36}$/);
  await page.getByLabel("6-digit code").fill(emailedCode(business));
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("You're signed up");
  expect(psql(`SELECT status FROM businesses.business WHERE id = '${business}'`)).toBe("active");
  const token = randomBytes(32).toString("hex");
  seedSignInLink(business, token);
  await page.goto(`${keyedUrl()}/b/sign-in/${token}`);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Enter your sign-in code");
  await page.getByLabel("Sign-in code").fill(await verifyCode(OWNER_PHONE));
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("You're signed in");
  await page.getByRole("link", { name: "pick the jobs you want us to send you" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Pick the jobs we send you");
  await expect(page.getByRole("checkbox", { name: "leak repair" })).toBeChecked();
  await page.getByRole("checkbox", { name: "Lake Havasu City" }).check();
  await page.getByRole("radio", { name: "No limit" }).check();
  await page.locator("#recurring-no").check();
  await page.locator("#after_hours-no").check();
  await page.getByRole("button", { name: "Check your answers" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Check your answers");
  await page.getByRole("button", { name: "Save and continue" }).click();
  await expect(page).toHaveURL(/\/b\/signup\/inbox$/);
  await expect(page.getByRole("button", { name: "Send me a sample job card" })).toBeVisible();
  expect(psql(`SELECT count(*) FROM businesses.capability WHERE business_id = '${business}'`)).toBe("1");
});
