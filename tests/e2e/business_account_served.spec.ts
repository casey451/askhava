/// <reference types="node" />
// TD-SEC-49's browser half (T306, D-2128, D-4551; BIZ-029, SEC-004, SEC-042): on T452's keyed web
// binary, a signed-in owner replies to a seeded complaint. The seed goes through psql on
// DATABASE_URL (`keyed.ts`): a fake active business, its owner's session and one fake complaint an
// hour old; the browser posts the reply with its own headers (D-4292) and the page then shows the
// reply in place of the box, with no homeowner detail. The served pages' axe checks run through
// their `PAGE_STATES` entries in tests/a11y/page_states.ts. T562 adds the charge page's case here.
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
