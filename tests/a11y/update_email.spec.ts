// The quote page's update choice in a browser (T729; TD-TXT-38's Ally cases: TXT-018, A11Y-030,
// D-4727, D-5442).
//
// The email question, its A11Y-030 error and the unconfirmed state are POST answers, so
// tests/a11y/page_states.ts (GET only) cannot reach them: this spec presses each step on the
// keyed web binary (tests/e2e/keyed.ts; the suite's default binary has no VAULT_KEY and stores no
// address) and runs axe on each answer at 320, 768 and 1280 px, with no violation allowed. Its
// job is seeded through psql: an open job with its quote-page token (stored only as its SHA256),
// a homeowner on an empty vault contact and her request's conversation, as page_states.ts's
// homeowner seeds do. Fake data only.
import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import type { Page } from "@playwright/test";
import { keyedUrl } from "../e2e/keyed";
import { WIDTHS, axeScan, expect, open, test } from "./fixtures";

const QUESTION = "What email should we use? We never share it with businesses.";

/** Seeds an open job with a homeowner and returns its quote page on the keyed binary. */
function seedJob(): string {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  const sql = `WITH c AS (INSERT INTO privacy.contact (id) VALUES (gen_random_uuid()) RETURNING id),
    j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('open', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    t AS (INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('${hash}', 'hex'), now() FROM j),
    h AS (INSERT INTO intake.homeowner (contact_id, created_at) SELECT id, now() FROM c RETURNING id)
    INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id, consent_template,
      consent_sent_at, opened_at) SELECT h.id, j.id, 'en', gen_random_uuid(), 'consent', now(), now() FROM h, j`;
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding the update choice failed: ${done.stderr}`);
  return `${keyedUrl()}/q/${token}`;
}

/** axe at every width on the page as it stands: no violation. */
async function clean(page: Page, step: string): Promise<void> {
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 900 });
    const { violations } = await axeScan(page);
    expect(violations, `${step} at ${String(width)} px`).toEqual([]);
  }
}

test("TD-TXT-38 A11Y-030: the email question, a malformed address and the unconfirmed state pass axe", async ({ page }) => {
  await open(page, seedJob(), 1280);
  await expect(page.getByLabel(QUESTION)).toHaveCount(0);
  await page.getByRole("button", { name: "Get updates by email instead" }).click();
  const field = page.getByLabel(QUESTION);
  await expect(field).toBeVisible();
  await clean(page, "the email question");
  await field.fill("pat@");
  await page.getByRole("button", { name: "Use this email" }).click();
  await expect(page).toHaveTitle(/^Error: /);
  await expect(page.locator("#update-summary")).toBeFocused();
  await expect(page.locator("#update-summary a")).toHaveAttribute("href", "#update-email");
  await expect(field).toHaveValue("pat@");
  await expect(field).toHaveAttribute("aria-invalid", "true");
  await clean(page, "a malformed address");
  await field.fill("pat@example.test");
  await page.getByRole("button", { name: "Use this email" }).click();
  await expect(page.getByText("We sent a confirm link to that email.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Confirm this email" })).toHaveCount(0);
  await clean(page, "the unconfirmed address");
});
