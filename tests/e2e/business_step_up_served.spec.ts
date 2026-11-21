/// <reference types="node" />
// TD-SEC-64's browser half (T565, D-2636, D-4045, D-3890; SEC-038, SEC-014): on T452's keyed web
// binary, a signed-in owner's role change first goes to T364's step-up page and changes nothing;
// the owner then walks the page (send a code, type the stand-in Verify code for the owner's linked
// fake phone) and posts the role change and a $300 monthly limit with `page.request` and the
// page's own CSRF field, each stored once (the limit is checked here, where psql may read it). No page has a role form yet (T544 builds the team page), so the
// change is posted as the form would post it, with a browser's own headers (D-4292). Each step-up
// screen passes axe. The seed goes through psql on DATABASE_URL (`keyed.ts`): a fake active
// business, its owner with a linked fake 555-01xx phone sealed into privacy.staff_phone and a
// session, and a "Cards and quotes" member. No test-control route; fake data only.
import { randomUUID } from "node:crypto";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { axeScan } from "../a11y/fixtures";
import { keyedUrl, psql, seal, sessionSql, signIn, token } from "./keyed";

const OWNER_PHONE = "+19285550174";

/** The stand-in Verify's code for `phone` (`stand_ins/twilio.py`: the code is a digest of `To`). */
function verifyCode(phone: string): string {
  let sum = 0;
  for (let i = 0; i < phone.length; i += 1) sum += phone.charCodeAt(i);
  return String(sum % 1_000_000).padStart(6, "0");
}

/** The fake business: its owner's session (`hash`), the owner's texting line and a member; the member's ID. */
function seedTeam(hash: string): string {
  const [business, owner, member, line] = [randomUUID(), randomUUID(), randomUUID(), randomUUID()];
  const sealed = seal("privacy.staff_phone", "number", line, OWNER_PHONE).toString("hex");
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT '${owner}', id, gen_random_uuid(), true, true, true, now()
      FROM b RETURNING id, business_id),
    k AS (INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT '${member}', id, gen_random_uuid(), false, true, false,
      now() + interval '1 second' FROM b),
    p AS (INSERT INTO businesses.staff_phone (id, business_id, linked_by, linked_at)
      SELECT '${line}', business_id, id, now() FROM m RETURNING id),
    c AS (INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version,
      consented_at, line_id) SELECT m.business_id, m.id, 'text', 'con019-v1', now(), p.id FROM m, p),
    v AS (INSERT INTO privacy.staff_phone (line_id, number) VALUES ('${line}', decode('${sealed}', 'hex'))),
    ${sessionSql(hash, "m")}
    SELECT 1`);
  return member;
}

/** The member's roles as `cards_and_quotes|confirm_and_billing` (`t` or `f`). */
function roles(member: string): string {
  return psql(`SELECT cards_and_quotes::text || '|' || confirm_and_billing::text
    FROM businesses.team_member WHERE id = '${member}'`);
}

/** The business's stored monthly limit in cents, empty when none. */
function limit(member: string): string {
  return psql(`SELECT l.limit_cents FROM payments.fee_limit l
    JOIN businesses.team_member m ON m.business_id = l.business_id WHERE m.id = '${member}'`);
}

/** Posts `form` to `path` as the browser's own form would; the 303's target. */
async function post(page: Page, path: string, form: Record<string, string>): Promise<string> {
  const url = keyedUrl();
  const headers = { origin: url, "sec-fetch-site": "same-origin" };
  const reply = await page.request.post(`${url}${path}`, { form, headers, maxRedirects: 0 });
  expect(reply.status()).toBe(303);
  return reply.headers().location ?? "";
}

/** No axe-core violation on the page as it stands. */
async function noViolation(page: Page): Promise<void> {
  const { violations } = await axeScan(page);
  expect(violations).toEqual([]);
}

test("TD-SEC-64: a role change and a monthly limit go through only after the step-up page's code", async ({ page }) => {
  const owner = token();
  const member = seedTeam(owner.hash);
  await signIn(page, owner.value);

  await page.goto(`${keyedUrl()}/b/step-up?return=%2Fb`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Confirm it's you");
  await noViolation(page);
  const csrf = await page.locator("input[name=csrf]").first().inputValue();
  const role = { csrf, member, cards_and_quotes: "yes", confirm_and_billing: "yes" };
  expect(await post(page, "/b/team/role", role)).toBe("/b/step-up?return=%2Fb");
  expect(roles(member), "no step-up: nothing changed").toBe("true|false");
  expect(await post(page, "/b/billing/limit", { csrf, limit: "300" })).toBe("/b/step-up?return=%2Fb%2Fbilling");
  expect(limit(member), "no step-up: no limit").toBe("");

  await page.getByRole("button", { name: "Send me a code" }).click();
  await expect(page.locator("#step-up-sent")).toBeVisible();
  await noViolation(page);
  await page.getByLabel("Code", { exact: true }).fill(verifyCode(OWNER_PHONE));
  await page.getByRole("button", { name: "Check code" }).click();
  await expect(page.locator("#step-up-done")).toHaveText(/That worked/);
  await noViolation(page);

  expect(await post(page, "/b/team/role", role)).toBe("/b");
  expect(roles(member), "the role changed once").toBe("true|true");
  expect(await post(page, "/b/billing/limit", { csrf, limit: "300" })).toBe("/b/billing");
  expect(limit(member), "the limit set once").toBe("30000");
});
