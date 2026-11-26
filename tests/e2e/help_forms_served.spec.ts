/// <reference types="node" />
// The browser halves of TD-BIZ-23 and TD-PRIV-16 (T309, D-2128, D-4293; BIZ-047, OPS-036,
// PRIV-031, SEC-042), on T452's keyed web binary, which has the email provider the Get help answer
// is queued with and the vault the help form's opt-out uses. A seeded business owner (psql on
// DATABASE_URL, `keyed.ts`) sends Get help and reads the topic's answer with the "Got it" line;
// one help request is open for that business. A visitor with no session sends "please delete my
// data" through the site help form with the token its own GET /help set: the answer links the
// homeowner sign-in, one help request opens and no data request, deletion or copy is recorded.
// Each test closes the requests it opened, so the owner help page's a11y state sees none open.
// The served pages' axe checks run through their `PAGE_STATES` entries in tests/a11y/page_states.ts.
// Fake data only; no key or value is printed or logged.
import { randomUUID } from "node:crypto";
import { test, expect } from "./fixtures";
import { keyedUrl, psql, sessionSql, signIn, token } from "./keyed";

/** A fake active business and its owner's session (`hash`); returns the business ID. */
function seedOwner(hash: string): string {
  const business = randomUUID();
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id),
    ${sessionSql(hash, "m")}
    SELECT 1`);
  return business;
}

/** Help requests open for `asker` (`business` with its ID, or `email` for the site form). */
function openRequests(asker: string, id: string | null): number {
  const whose = id === null ? "asker_id IS NULL" : `asker_id = '${id}'`;
  return Number(psql(`SELECT count(*) FROM ops.help_request WHERE asker = '${asker}' AND ${whose} AND closed_at IS NULL`));
}

/** Closes the help requests `asker` opened since `since`, so later pages see none open (as owner_reports_served.spec.ts does). */
function closeRequests(asker: string, since: string): void {
  psql(`UPDATE ops.help_request SET closed_at = now() WHERE asker = '${asker}' AND opened_at >= '${since}' AND closed_at IS NULL`);
}

/** Audit rows of a data request, a deletion or a contact read: what a help-form ask must not leave. */
function dataRows(): number {
  return Number(psql(`SELECT count(*) FROM ops.audit_log WHERE subject_kind IN ('portals.data_request', 'privacy.contact')`));
}

test("TD-BIZ-23: a signed-in business sends Get help and reads the topic's answer", async ({ page }) => {
  const owner = token();
  const business = seedOwner(owner.hash);
  const since = psql("SELECT now()");
  await signIn(page, owner.value);

  await page.goto(`${keyedUrl()}/b/help`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Get help");
  await page.getByLabel("Emails not arriving").check();
  await page.getByLabel("Job number (optional)").fill("1042");
  await page.getByLabel("Your message").fill("Fake: no card emails since Monday.");
  await page.getByRole("button", { name: "Send" }).click();

  const answer = page.locator("#help-answer");
  await expect(answer).toContainText("spam");
  await expect(answer.locator("[data-fixed='BIZ-047']")).toContainText("usually replies within 2 days");
  expect(openRequests("business", business), "one help request for that business").toBe(1);
  closeRequests("business", since);
});

test("TD-PRIV-16: a help-form delete request is answered with the sign-in link and changes nothing", async ({ page }) => {
  await signIn(page, null);
  const [before, rows, since] = [openRequests("email", null), dataRows(), psql("SELECT now()")];

  await page.goto(`${keyedUrl()}/help`);
  await page.getByLabel("Your message").fill("Please delete my data.");
  await page.getByRole("button", { name: "Send message" }).click();

  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Message sent");
  await expect(page.getByRole("link", { name: "sign in with your phone number" })).toHaveAttribute("href", "/h/sign-in");
  expect(openRequests("email", null), "one help request opened").toBe(before + 1);
  expect(dataRows(), "nothing requested, deleted or read").toBe(rows);
  closeRequests("email", since);
});
