/// <reference types="node" />
// TD-BIZ-62's stand-in half (T734, D-4727, D-4748, D-4749, D-4875, D-5670; BIZ-051, BIZ-054,
// SEC-038, A11Y-030): on T452's keyed web binary, whose Stripe base URL is the Python stand-in, a
// signed-in owner walks the step-up page (the stand-in Verify code for the owner's linked fake
// phone), opens the served leave screen and presses its button. Her saved card was attached to a
// customer at the stand-in first (a customer and a setup intent, as Stripe saves a card). With
// the `stripe_detach` fault off the card is detached at the stand-in (a second detach is refused
// as not attached); with it on (every detach answers Stripe's 500) the leave still completes, the
// card stays marked removed and it is still attached at the stand-in, the one detach the leave
// counted as failed (the count and its one log line are `app/server/tests/business_leave_route.rs`'
// and `portals::business_left`'s). Each screen passes axe. The seed goes through psql on
// DATABASE_URL (`keyed.ts`): a fake active business, its owner with a linked fake 555-01xx phone
// and a session, and the saved card's rows. `Faults` has no `stripe_detach` key and `fixtures.ts`
// is an earlier task's test, so the fault is set through a local type extending it (D-5670 (d)). Fake data only.
import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { APIRequestContext, Page } from "@playwright/test";
import { test, expect, type Faults } from "./fixtures";
import { axeScan } from "../a11y/fixtures";
import { keyedUrl, psql, seal, sessionSql, signIn, token } from "./keyed";

/** The brand file the app embeds (app/core/data/brand.json, D-1932): expected brand words come from it. */
const BRAND = JSON.parse(readFileSync(join(import.meta.dirname, "..", "..", "app", "core", "data", "brand.json"), "utf8")) as {
  display_name: string;
};
const STRIPE = "http://127.0.0.1:4010/stripe/v1";

/** The stand-in's faults with T751's `stripe_detach` switch, which `Faults` does not name (D-5670 (d)). */
interface DetachFaults extends Faults {
  stripe_detach?: "fail" | null;
}
const STRIPE_AUTH = { authorization: "Bearer sk_test_fake_t734" };

test.describe.configure({ mode: "serial" });

/** The stand-in Verify's code for `phone` (`stand_ins/twilio.py`: the code is a digest of `To`). */
function verifyCode(phone: string): string {
  let sum = 0;
  for (let i = 0; i < phone.length; i += 1) sum += phone.charCodeAt(i);
  return String(sum % 1_000_000).padStart(6, "0");
}

/** Attaches `card` to a new stand-in customer, as a saved card is; the customer's ID. */
async function attachAtStandIn(request: APIRequestContext, card: string): Promise<string> {
  const customer = await request.post(`${STRIPE}/customers`, { headers: STRIPE_AUTH, form: { email: "owner.t734@example.com" } });
  expect(customer.status()).toBe(200);
  const id = ((await customer.json()) as { id: string }).id;
  const form = { customer: id, payment_method: card, usage: "off_session" };
  const setup = await request.post(`${STRIPE}/setup_intents`, { headers: STRIPE_AUTH, form });
  expect(setup.status()).toBe(200);
  return id;
}

/** A detach sent straight to the stand-in: its status (200 while attached, 400 once detached). */
async function detachStatus(request: APIRequestContext, card: string): Promise<number> {
  const reply = await request.post(`${STRIPE}/payment_methods/${card}/detach`, { headers: STRIPE_AUTH, form: {} });
  return reply.status();
}

/** The fake business: its owner's session (`hash`), texting line `phone` and saved `card`; its ID. */
function seedOwner(hash: string, phone: string, card: string, customer: string): string {
  const [business, owner, line] = [randomUUID(), randomUUID(), randomUUID()];
  const sealed = seal("privacy.staff_phone", "number", line, phone).toString("hex");
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT '${owner}', id, gen_random_uuid(), true, true, true, now()
      FROM b RETURNING id, business_id),
    p AS (INSERT INTO businesses.staff_phone (id, business_id, linked_by, linked_at)
      SELECT '${line}', business_id, id, now() FROM m RETURNING id),
    c AS (INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version,
      consented_at, line_id) SELECT m.business_id, m.id, 'text', 'con019-v1', now(), p.id FROM m, p),
    v AS (INSERT INTO privacy.staff_phone (line_id, number) VALUES ('${line}', decode('${sealed}', 'hex'))),
    ${sessionSql(hash, "m")}
    SELECT 1`);
  psql(`WITH k AS (INSERT INTO payments.customer (business_id, stripe_customer_id)
      VALUES ('${business}', '${customer}') RETURNING business_id),
    a AS (INSERT INTO payments.card_authorization (business_id, version, text, accepted_at)
      SELECT business_id, 1, 'Sample authorization wording', now() FROM k RETURNING id, business_id)
    INSERT INTO payments.payment_method (stripe_payment_method_id, business_id, authorization_id,
      method_type, usage, brand, last4, saved_at)
    SELECT '${card}', business_id, id, 'card', 'off_session', 'visa', '4242', now() FROM a`);
  return business;
}

/** `business`'s status and whether `card` is marked removed (`status|t` or `status|f`). */
function leftState(business: string, card: string): string {
  return psql(`SELECT b.status || '|' || (m.removed_at IS NOT NULL)::text
    FROM businesses.business b, payments.payment_method m
    WHERE b.id = '${business}' AND m.stripe_payment_method_id = '${card}'`);
}

/** No axe-core violation on the page as it stands. */
async function noViolation(page: Page): Promise<void> {
  const { violations } = await axeScan(page);
  expect(violations).toEqual([]);
}

/** Signs the owner in, passes the step-up page and presses "Leave" on the served leave screen. */
async function leaveThroughPages(page: Page, session: string, phone: string): Promise<void> {
  await signIn(page, session);
  await page.goto(`${keyedUrl()}/b/step-up?return=%2Fb%2Fleave`);
  await page.getByRole("button", { name: "Send me a code" }).click();
  await page.getByLabel("Code", { exact: true }).fill(verifyCode(phone));
  await page.getByRole("button", { name: "Check code" }).click();
  await expect(page.locator("#step-up-done")).toHaveText(/That worked/);
  await page.goto(`${keyedUrl()}/b/leave`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Leave ${BRAND.display_name}`);
  await noViolation(page);
  await page.getByLabel("Too few jobs").check();
  await page.getByRole("button", { name: `Leave ${BRAND.display_name}` }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(`You've left ${BRAND.display_name}`);
  await noViolation(page);
}

test("TD-BIZ-62: leaving through the served pages detaches the saved card at the stand-in", async ({ page, request }) => {
  const [card, phone, owner] = ["pm_card_chargeCustomerFail", "+19285550175", token()];
  const customer = await attachAtStandIn(request, card);
  const business = seedOwner(owner.hash, phone, card, customer);
  await leaveThroughPages(page, owner.value, phone);
  expect(leftState(business, card), "left, card marked removed").toBe("removed|true");
  expect(await detachStatus(request, card), "already detached at the stand-in").toBe(400);
});

test("TD-BIZ-62: with every detach failing the leave still completes and the card stays removed", async ({ page, request, control }) => {
  const [card, phone, owner] = ["pm_card_chargeDeclined", "+19285550176", token()];
  const customer = await attachAtStandIn(request, card);
  const business = seedOwner(owner.hash, phone, card, customer);
  const fail: DetachFaults = { stripe_detach: "fail" };
  await control.setFaults(fail);
  await leaveThroughPages(page, owner.value, phone);
  expect(leftState(business, card), "left, card marked removed").toBe("removed|true");
  await control.clearFaults();
  expect(await detachStatus(request, card), "the failed detach left it attached").toBe(200);
});
