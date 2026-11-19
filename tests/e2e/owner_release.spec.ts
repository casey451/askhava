/// <reference types="node" />
// The owner's release button in a browser (T540; TD-PICK-41's browser half; PICK-022, PICK-023,
// SEC-006; D-074, D-3890, D-3892).
//
// The owner signs in only through the shared tests/e2e/owner_session.ts helper. A released pick
// on a fake job is seeded through psql on DATABASE_URL (a fake business whose owner has a mailbox
// ID only, no contact), and so is a paid pick with no release on another job. Pressing "Re-send
// the release notice" for the released job queues one release notice under that press's key and adds no
// release row; pressing again with the same press ID queues nothing more; the
// unreleased job is refused with the error summary and queues nothing. No page shows a phone
// number. The database refusals in every state are tests/integration/tests/owner_release.rs.
// Fake data only.
import { randomUUID } from "node:crypto";
import { test, expect } from "./fixtures";
import { psql } from "./keyed";
import { signInOwner } from "./owner_session";

test.setTimeout(120_000);

/** Seeds a paid pick on a new connected job, with its release row when `released`; its job number. */
function seedPick(pick: string, released: boolean): string {
  const release = released
    ? `,
    r AS (INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen)
      SELECT p.id, p.job_id, p.business_id, 'paid', now(), gen_random_uuid(), false FROM p)`
    : "";
  return psql(`WITH b AS (INSERT INTO businesses.business (status, created_at, status_changed_at)
      VALUES ('active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING business_id),
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES (gen_random_uuid(), 'connected', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j RETURNING number),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT '${pick}', j.id, m.business_id, 1, 'page', 'paid', now() FROM j, m RETURNING id, job_id, business_id)${release}
    SELECT number FROM num`);
}

/** How many release re-send notices are queued for `pick`. */
function resends(pick: string): string {
  return psql(`SELECT count(*) FROM core.task WHERE kind = 'messaging.business_notice'
    AND dedupe_key LIKE 'messaging.business_notice.release_resend.${pick}.%'`);
}

test("TD-PICK-41: the owner re-sends a standing release once per press and never releases a job", async ({ page }) => {
  const [released, unreleased] = [randomUUID(), randomUUID()];
  const releasedNumber = seedPick(released, true);
  const unreleasedNumber = seedPick(unreleased, false);
  await signInOwner(page);
  const opened = await page.goto("/owner/release");
  expect(opened?.status()).toBe(200);
  expect(opened?.headers()["cache-control"]).toContain("no-store");
  await page.getByLabel("Job number").fill(releasedNumber);
  const press = await page.locator('input[name="press"]').inputValue();
  await page.getByRole("button", { name: "Re-send the release notice" }).click();
  await expect(page.locator("#release-status")).toContainText(`Sent again. We sent the release notice for job ${releasedNumber} to the business.`);
  expect(resends(released)).toBe("1");
  expect(psql(`SELECT count(*) FROM picks.release WHERE pick_id = '${released}'`)).toBe("1");
  await page.getByLabel("Job number").fill(releasedNumber);
  await page.locator('input[name="press"]').evaluate((input, old) => {
    (input as HTMLInputElement).value = old;
  }, press);
  await page.getByRole("button", { name: "Re-send the release notice" }).click();
  await expect(page.locator("#release-status")).toContainText("You already pressed this, so we didn't send it twice.");
  expect(resends(released)).toBe("1");
  await page.getByLabel("Job number").fill(unreleasedNumber);
  await page.getByRole("button", { name: "Re-send the release notice" }).click();
  await expect(page).toHaveTitle(/^Error: /);
  await expect(page.locator("#error-summary")).toContainText(`Job ${unreleasedNumber} hasn't been released`);
  await expect(page.getByLabel("Job number")).toHaveValue(unreleasedNumber);
  expect(resends(unreleased)).toBe("0");
  expect(psql(`SELECT count(*) FROM picks.release WHERE pick_id = '${unreleased}'`)).toBe("0");
  expect(await page.content()).not.toMatch(/\d{3}-\d{3}-\d{4}/);
});
