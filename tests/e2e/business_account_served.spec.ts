/// <reference types="node" />
// TD-SEC-49's browser half (T306, D-2128, D-4551; BIZ-029, SEC-004, SEC-042): on T452's keyed web
// binary, a signed-in owner replies to a seeded complaint. The seed goes through psql on
// DATABASE_URL (`keyed.ts`): a fake active business, its owner's session and one fake complaint an
// hour old; the browser posts the reply with its own headers (D-4292) and the page then shows the
// reply in place of the box, with no homeowner detail. The served pages' axe checks run through
// their `PAGE_STATES` entries in tests/a11y/page_states.ts. T562 (FEE-031, D-3890) adds the charge
// page's case: the owner opens "Why was I charged?" for a seeded $25 charge on a fake $480 quote.
// No test-control route; fake data only; no key or value is printed or logged.
import { randomUUID } from "node:crypto";
import { test, expect } from "./fixtures";
import { keyedUrl, psql, sessionSql, signIn, token } from "./keyed";

/** A fake active business, its owner's session (`session.hash`) and a `no_show` complaint an hour old. */
function seedComplaint(hash: string): void {
  const business = randomUUID();
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id),
    ${sessionSql(hash, "m")}
    INSERT INTO businesses.complaint (business_id, job_id, kind, safety, received_at)
      VALUES ('${business}', gen_random_uuid(), 'no_show', false, now() - interval '1 hour')`);
}

test("TD-SEC-49: a signed-in owner replies to a seeded complaint within its 5 days", async ({ page }) => {
  const owner = token();
  seedComplaint(owner.hash);
  await signIn(page, owner.value);

  await page.goto(`${keyedUrl()}/b/complaints`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Complaints");
  await expect(page.locator("main")).toContainText("No show");
  const box = page.locator("main textarea");
  await expect(box).toHaveCount(1);
  await box.fill("Sorry we missed you. We will call to set a new time.");
  await page.locator("main form").getByRole("button").click();

  await expect(page).toHaveURL(`${keyedUrl()}/b/complaints`);
  await expect(page.locator("main")).toContainText("Sorry we missed you. We will call to set a new time.");
  await expect(page.locator("main textarea"), "one reply, then the box closes").toHaveCount(0);
});

/** A fake active business, its owner's session and a pick charged $25 on its fake $480 firm quote: the pick. */
function seedCharge(hash: string): string {
  const pick = randomUUID();
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES (gen_random_uuid(), 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id),
    ${sessionSql(hash, "m")},
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES (gen_random_uuid(), 'awaiting_confirm', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    q AS (INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes, original,
      ai_read, confirmed, received_at) SELECT j.id, m.business_id, 'firm', 48000, false, '', 'Fake quote wording',
      false, true, now() FROM j, m),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT '${pick}', j.id, m.business_id, 1, 'page', 'awaiting_confirm', now() FROM j, m RETURNING id, job_id, business_id),
    f AS (INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents, terms_version, fixed_at)
      SELECT p.id, p.business_id, p.job_id, t.mode, 'charged', 2500, t.authorization_version, now() FROM p, payments.fee_terms t),
    i AS (INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at)
      SELECT id, business_id, job_id, 2500, 'pi_fake' || replace(id::text, '-', ''), false, now() FROM p RETURNING pick_id),
    a AS (INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at)
      SELECT pick_id, 1, gen_random_uuid(), 'succeeded', now(), now() FROM i RETURNING pick_id, press_id)
    INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label,
      stripe_customer_id, stripe_intent_id, amount_cents, pressed_at, charged_at)
      SELECT p.id, p.business_id, p.job_id, a.press_id, 1, 'pick-' || p.id::text || '-try1',
        'cus_fake' || replace(p.business_id::text, '-', ''), 'pi_fake' || replace(p.id::text, '-', ''), 2500, now(), now()
      FROM p, a`);
  return pick;
}

test("TD-SEC-49: a signed-in owner opens a charge's page", async ({ page }) => {
  const owner = token();
  const pick = seedCharge(owner.hash);
  await signIn(page, owner.value);

  await page.goto(`${keyedUrl()}/b/charges/${pick}`);
  await expect(page.getByRole("heading", { level: 1 })).toContainText("Why was I charged for job");
  await expect(page.locator("#quote-line")).toContainText("$480 (Firm price)");
  await expect(page.locator("#tier-line")).toContainText("Introduction fee by job size: $25.00 (small job).");
  await expect(page.locator("#basis-line")).toContainText("The fee basis from your quote is $480.00.");
});
