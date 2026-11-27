/// <reference types="node" />
// TD-A11Y-05's browser half (T657; A11Y-030; D-4326, D-5670): the failed POST answers of the
// served portal forms, which tests/a11y/page_states.ts cannot open (it opens states by GET only).
// Owner sign-in with a wrong password, the owner step-up with a wrong code, business sign-in with
// an empty address, and the quote form with package 3 named but unpriced and with a bad high price
// each answer a page titled "Error: …" whose focused error summary is main's first element,
// before the h1, with aria-invalid="true" on exactly the faulty fields (both owner sign-in factors
// together, never one alone) and no axe violation at 320 and 1280 px. Each form is posted once and
// its answer scanned at both widths. The two owner tests send the edge header the default
// `ClientIpPolicy` reads (`x-real-ip`) with a documentation address, so their password and step-up
// tries count in that address's own buckets and never in 127.0.0.1's few an hour
// (`OWNER_TRIES_PER_IP`, `OWNER_STEP_UPS_PER_IP`) that the later owner specs and J3 sign in on
// (D-5872). The quote links are seeded through psql on DATABASE_URL as
// tests/e2e/action_links_served.spec.ts seeds its own, for a fake active business and a made-up
// job in a packages category (car detailing) and a range category (heating and cooling
// replacement). POSTs go with the browser's own headers (D-4292). Fake data only; no token is printed.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { axeScan } from "../a11y/fixtures";
import { test, expect } from "./fixtures";
import { keyedUrl } from "./keyed";
import { signInOwner } from "./owner_session";

/** A documentation address (RFC 5737) the owner tests post from, never the run's 127.0.0.1 (D-5872). */
const OWNER_TEST_IP = "203.0.113.57";

/** Runs `sql` through psql on DATABASE_URL. */
function psql(sql: string): void {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed with status ${String(done.status)}`);
}

/** An open job of `jobType` in `category` carded to a new fake active business, its card sent, and a live Quote link's path. */
function seedQuoteLink(category: string, jobType: string): string {
  const [business, job] = [randomUUID(), randomUUID()];
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(`action:quote:${job}:${token}`).digest("hex");
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id),
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES ('${job}', 'open', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    f AS (INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at)
      SELECT id, 1, '${category}', ARRAY['${jobType}'], now() FROM j),
    c AS (INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
      SELECT j.id, b.id, false, now() FROM j, b),
    s AS (INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place,
      photo_ids, category, candidates, emergency, recorded_at, sent_at)
      SELECT gen_random_uuid(), j.id, left(encode(sha256(convert_to(t.body, 'UTF8')), 'hex'), 32),
        t.body, 'Lake Havasu City', 'house', '{}', '${category}', ARRAY['${jobType}'], false,
        now(), now()
      FROM j, (VALUES ('A made-up job in Lake Havasu City for the form test.')) AS t(body))
    INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
      SELECT decode('${hash}', 'hex'), m.id, m.business_id, now() - interval '1 day', now() + interval '2 hours' FROM m`);
  return `/b/a/quote/${job}/${token}`;
}

/** Answers every quote fact so only the price can fail. */
async function answerFacts(page: Page): Promise<void> {
  for (const id of ["trip_fee-none", "parts-included", "permit-not_needed", "may_change-no", "visit_part-morning", "spanish-no"]) {
    await page.locator(`#${id}`).check();
  }
  const soon = new Date(Date.now() + 7 * 86_400_000).toISOString().slice(0, 10);
  await page.locator("#visit_date").fill(soon);
}

/** A11Y-030: the failed page `what` as TD-A11Y-05 asks, marking exactly `invalid`, axe-clean at 320 and 1280 px. */
async function failedPage(page: Page, invalid: string[], what: string): Promise<void> {
  await expect(page).toHaveTitle(/^Error: /);
  const first = page.locator("main > :first-child");
  await expect(first, what).toHaveAttribute("id", "error-summary");
  await expect(first).toHaveAttribute("role", "alert");
  const order = await page.evaluate(() => {
    const summary = document.getElementById("error-summary");
    const h1 = document.querySelector("main h1");
    return summary !== null && h1 !== null && (summary.compareDocumentPosition(h1) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
  });
  expect(order, `${what}: the summary comes before the h1`).toBe(true);
  await expect.poll(() => page.evaluate(() => document.activeElement?.id), what).toBe("error-summary");
  const marked = await page.locator('[aria-invalid="true"]').evaluateAll((els) => els.map((e) => e.id));
  expect(marked, `${what}: aria-invalid on exactly the faulty fields`).toEqual(invalid);
  for (const width of [320, 1280]) {
    await page.setViewportSize({ width, height: 900 });
    const { violations } = await axeScan(page);
    expect(violations, `${what} at ${String(width)} px`).toEqual([]);
  }
}

test.describe("TD-A11Y-05 A11Y-030: failed portal forms", () => {
  test.setTimeout(150_000);

  test("owner sign-in with a wrong password marks both factors", async ({ page }) => {
    await page.context().setExtraHTTPHeaders({ "x-real-ip": OWNER_TEST_IP });
    await page.goto("/owner/sign-in");
    await page.getByLabel("Password").fill("not-the-password-0657");
    await page.getByLabel("Code", { exact: true }).fill("123456");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await failedPage(page, ["password", "code"], "owner sign-in");
    await expect(page.locator("#error-summary h2")).toHaveText("That didn't work");
    for (const id of ["password", "code"]) {
      await expect(page.locator(`#${id}`)).toHaveAttribute("aria-describedby", /(^| )signin-error$/);
    }
  });

  test("the owner step-up with a wrong code marks the code", async ({ page }) => {
    await page.context().setExtraHTTPHeaders({ "x-real-ip": OWNER_TEST_IP });
    await signInOwner(page);
    await page.getByLabel("Code", { exact: true }).fill("000000");
    await page.getByRole("button", { name: "Check code" }).click();
    await failedPage(page, ["code"], "owner step-up");
  });

  test("business sign-in with an empty address marks the email field", async ({ page }) => {
    await page.goto("/b/sign-in");
    await page.getByRole("button", { name: "Email me a sign-in link" }).click();
    await failedPage(page, ["email"], "business sign-in");
  });

  test("the quote form with package 3 named but unpriced marks package 3's price", async ({ page }) => {
    await page.goto(`${keyedUrl()}${seedQuoteLink("car_detailing", "full detail")}`);
    await page.getByLabel("Package 1 name").fill("Basic wash");
    await page.getByLabel("Package 1 price ($)").fill("120");
    await page.getByLabel("Package 3 name").fill("Full detail");
    await answerFacts(page);
    await page.getByRole("button", { name: /^Send quote for job \d+$/ }).click();
    await failedPage(page, ["tier3_price"], "quote form, package 3 unpriced");
    await expect(page.locator("#error-summary a")).toHaveAttribute("href", "#tier3_price");
    await expect(page.locator("#tier3_price")).toHaveAttribute("aria-describedby", "tier3_price-error");
  });

  test("the quote form with a bad high price marks the high price", async ({ page }) => {
    await page.goto(`${keyedUrl()}${seedQuoteLink("hvac_replacement", "full system replacement")}`);
    await page.getByLabel("Low price ($)").fill("3000");
    await page.getByLabel("High price ($)").fill("lots");
    await answerFacts(page);
    await page.getByRole("button", { name: /^Send quote for job \d+$/ }).click();
    await failedPage(page, ["high"], "quote form, bad high price");
    await expect(page.locator("#error-summary a")).toHaveAttribute("href", "#high");
  });
});
