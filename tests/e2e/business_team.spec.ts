/// <reference types="node" />
// The business team page's walk (T544, D-3890, D-3892, D-5318, D-5567; BIZ-023, SEC-038, SEC-019):
// on T452's keyed web binary, the owner opens /b/team, ticks a role for a member and presses Save
// roles; the change first goes to the step-up page and changes nothing. The owner passes the
// step-up the way business_step_up_served.spec.ts does (send a code, type the stand-in Verify code
// for the owner's linked fake phone), re-opens /b/team (the step-up lands on /b, D-5318), presses
// Save roles again and the role changes once, then removes a member. A "Confirm and billing"
// member sees the list and the monthly-limit form only; a "Cards and quotes" member gets the one
// not-found answer. Every page passes axe. The seed goes through psql on DATABASE_URL
// (`keyed.ts`): fake 555-01xx phone and example.com addresses only. No test-control route.
import { randomUUID } from "node:crypto";
import type { Locator, Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { axeScan } from "../a11y/fixtures";
import { keyedUrl, psql, seal, sessionSql, signIn, token } from "./keyed";

const OWNER_PHONE = "+19285550175";

/** The stand-in Verify's code for `phone` (`stand_ins/twilio.py`: the code is a digest of `To`). */
function verifyCode(phone: string): string {
  let sum = 0;
  for (let i = 0; i < phone.length; i += 1) sum += phone.charCodeAt(i);
  return String(sum % 1_000_000).padStart(6, "0");
}

/** A sealed sign-in address row for `mailbox`. */
function mailboxSql(name: string, mailbox: string, address: string): string {
  const sealed = seal("privacy.business_mailbox", "address", mailbox, address).toString("hex");
  return `${name} AS (INSERT INTO privacy.business_mailbox (id, address) VALUES ('${mailbox}', decode('${sealed}', 'hex')))`;
}

/** A member row of business CTE `b`, added `after` seconds after the owner. */
function memberSql(name: string, [id, mailbox]: [string, string], roles: string, after: number): string {
  return `${name} AS (INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT '${id}', id, '${mailbox}', false, ${roles},
      now() + interval '${String(after)} seconds' FROM b)`;
}

interface Team { pat: string; sam: string }

/** The fake business: the owner (session `hash`, a linked fake phone), Pat ("Cards and quotes") and Sam ("Confirm and billing"). */
function seedTeam(hash: string): Team {
  const [business, owner, line] = [randomUUID(), randomUUID(), randomUUID()];
  const [ownerBox, pat, patBox, sam, samBox] = [randomUUID(), randomUUID(), randomUUID(), randomUUID(), randomUUID()];
  const phone = seal("privacy.staff_phone", "number", line, OWNER_PHONE).toString("hex");
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT '${owner}', id, '${ownerBox}', true, true, true, now()
      FROM b RETURNING id, business_id),
    ${memberSql("k", [pat, patBox], "true, false", 1)},
    ${memberSql("j", [sam, samBox], "false, true", 2)},
    p AS (INSERT INTO businesses.staff_phone (id, business_id, linked_by, linked_at)
      SELECT '${line}', business_id, id, now() FROM m RETURNING id),
    c AS (INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version,
      consented_at, line_id) SELECT m.business_id, m.id, 'text', 'con019-v1', now(), p.id FROM m, p),
    v AS (INSERT INTO privacy.staff_phone (line_id, number) VALUES ('${line}', decode('${phone}', 'hex'))),
    ${mailboxSql("o1", ownerBox, "owner@example.com")},
    ${mailboxSql("o2", patBox, "pat@example.com")},
    ${mailboxSql("o3", samBox, "sam@example.com")},
    ${sessionSql(hash, "m")}
    SELECT 1`);
  return { pat, sam };
}

/** A fresh session token for team member `id`. */
function sessionFor(id: string): string {
  const t = token();
  psql(`WITH k AS (SELECT id, business_id FROM businesses.team_member WHERE id = '${id}'),
    ${sessionSql(t.hash, "k")} SELECT 1`);
  return t.value;
}

/** The member's roles as `cards_and_quotes|confirm_and_billing` and whether they were removed. */
function stored(member: string): string {
  return psql(`SELECT cards_and_quotes::text || '|' || confirm_and_billing::text || '|' || (removed_at IS NOT NULL)::text
    FROM businesses.team_member WHERE id = '${member}'`);
}

/** No axe-core violation on the page as it stands. */
async function noViolation(page: Page): Promise<void> {
  const { violations } = await axeScan(page);
  expect(violations).toEqual([]);
}

/** Opens the team page and ticks "Confirm and billing" for Pat; Pat's list entry. */
async function tickPat(page: Page): Promise<Locator> {
  await page.goto(`${keyedUrl()}/b/team`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Your team and monthly limit");
  const roles = page.getByRole("group", { name: "Roles for pat@example.com" });
  await roles.getByLabel("Confirm and billing").check();
  return page.getByRole("listitem").filter({ has: page.getByRole("heading", { name: /pat@example\.com/ }) });
}

test("BIZ-023: a role change on the team page goes through once, after the step-up", async ({ page }) => {
  const owner = token();
  const { pat, sam } = seedTeam(owner.hash);

  await signIn(page, sessionFor(sam));
  await page.goto(`${keyedUrl()}/b/team`);
  await expect(page.getByText("pat@example.com")).toBeVisible();
  await expect(page.getByRole("button", { name: "Save monthly limit" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Save roles" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Remove from your team" })).toHaveCount(0);
  await noViolation(page);
  await signIn(page, sessionFor(pat));
  expect((await page.goto(`${keyedUrl()}/b/team`))?.status(), "Cards and quotes: not found").toBe(404);

  await signIn(page, owner.value);
  await (await tickPat(page)).getByRole("button", { name: "Save roles" }).click();
  await expect(page).toHaveURL(/\/b\/step-up\?return=%2Fb$/);
  expect(stored(pat), "no step-up: nothing changed").toBe("true|false|false");
  await page.getByRole("button", { name: "Send me a code" }).click();
  await expect(page.locator("#step-up-sent")).toBeVisible();
  await page.getByLabel("Code", { exact: true }).fill(verifyCode(OWNER_PHONE));
  await page.getByRole("button", { name: "Check code" }).click();
  await expect(page.locator("#step-up-done")).toHaveText(/That worked/);

  const entry = await tickPat(page);
  await noViolation(page);
  await entry.getByRole("button", { name: "Save roles" }).click();
  await expect(page).toHaveURL(`${keyedUrl()}/b`);
  expect(stored(pat), "the role changed once").toBe("true|true|false");

  await page.goto(`${keyedUrl()}/b/team`);
  const samEntry = page.getByRole("listitem").filter({ has: page.getByRole("heading", { name: /sam@example\.com/ }) });
  await samEntry.getByRole("button", { name: "Remove from your team" }).click();
  await expect(page).toHaveURL(`${keyedUrl()}/b`);
  expect(stored(sam), "removed").toBe("false|true|true");
  await page.goto(`${keyedUrl()}/b/team`);
  await expect(page.getByText("sam@example.com")).toHaveCount(0);
});
