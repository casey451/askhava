/// <reference types="node" />
// TD-SEC-44's browser half for a used link (T814, D-5316, D-5320; SEC-026, BIZ-038, A11Y-001): on
// T452's keyed web binary, a used action link pressed again acts no more and shows its result,
// and that answer has no axe-core violation. Links are seeded through psql on DATABASE_URL as
// tests/e2e/action_links_served.spec.ts seeds them: a portals.login_link row holding the SHA-256
// of `action:<kind>:<object>:<token>` (D-1955's hash) for a token chosen here, for the owner of a
// fake active business. The Quote link's job is an open AC repair job carded to that business,
// its card sent (D-3189); its form opens in the browser, the first press (a same-origin fetch of
// the filled form) stores the quote, and the form's own button then presses the used link again:
// the job page comes back with the one stored quote. The Hold link's first press needs a pick in
// a running Confirm window, which `action_links_route.rs` covers; here its page opens while the
// link is unused, the row is then marked used as that press leaves it, and the used link is
// pressed twice (a fetch, then the page's own button): both answers are "This link was already
// used" with the portal link. The done page answers only a POST, so it has no PAGE_STATES entry
// (D-5320). No test-control route; fake data only; no token is printed.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { axeScan } from "../a11y/fixtures";
import { keyedUrl, psql } from "./keyed";

/** A link for `kind` on `object`: its path and the SQL hex of its stored hash (D-1955). */
function link(kind: string, object: string): { path: string; hash: string } {
  const value = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(`action:${kind}:${object}:${value}`).digest("hex");
  return { path: `/b/a/${kind}/${object}/${value}`, hash };
}

/** A new fake active business and its owner as CTEs `b` and `m`. */
const OWNER_SQL = (business: string): string => `b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id)`;

/** The owner's live link row for `hash`, from CTE `m`. */
const LINK_SQL = (hash: string): string => `INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
      SELECT decode('${hash}', 'hex'), m.id, m.business_id, now() - interval '1 day', now() + interval '2 hours' FROM m`;

/** An open AC repair job carded to a new fake active business, its card sent, and the owner's live Quote link. */
function seedQuoteLink(): { path: string; job: string } {
  const [business, job] = [randomUUID(), randomUUID()];
  const quote = link("quote", job);
  psql(`WITH ${OWNER_SQL(business)},
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES ('${job}', 'open', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    f AS (INSERT INTO jobs.job_facts (job_id, seq, category, job_types, set_at)
      SELECT id, 1, 'hvac_repair', ARRAY['AC not cooling'], now() FROM j),
    c AS (INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
      SELECT j.id, b.id, false, now() FROM j, b),
    s AS (INSERT INTO quotes.shown_card (request_id, job_id, digest, card_text, town, place,
      photo_ids, category, candidates, emergency, recorded_at, sent_at)
      SELECT gen_random_uuid(), j.id, left(encode(sha256(convert_to(t.body, 'UTF8')), 'hex'), 32),
        t.body, 'Lake Havasu City', 'house', '{}', 'hvac_repair', ARRAY['AC not cooling'], false,
        now(), now()
      FROM j, (VALUES ('AC repair in Lake Havasu City. The AC runs but the air stays warm.')) AS t(body))
    ${LINK_SQL(quote.hash)}`);
  return { path: quote.path, job };
}

/** The owner of a new fake active business and their live Hold link on a random pick: its path and hash. */
function seedHoldLink(): { path: string; hash: string } {
  const hold = link("hold", randomUUID());
  psql(`WITH ${OWNER_SQL(randomUUID())} ${LINK_SQL(hold.hash)}`);
  return hold;
}

/** The firm $325 quote with every quote fact (PICK-040), visiting ten days from now. */
function quoteFields(): Record<string, string> {
  const visit = new Date(Date.now() + 10 * 24 * 3600 * 1000).toISOString().slice(0, 10);
  return {
    amount: "325", notes: "Parts and labor included.", trip_fee: "none", parts: "included",
    permit: "not_needed", may_change: "no", visit_date: visit, visit_part: "morning", spanish: "no",
  };
}

/** Puts `fields` into the page's form (text values, ticked radios) and returns nothing. */
async function fill(page: Page, fields: Record<string, string>): Promise<void> {
  await page.evaluate((values) => {
    const form = document.querySelector("form[method='post']") as HTMLFormElement;
    for (const [name, value] of Object.entries(values)) {
      for (const field of Array.from(form.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>(`[name='${name}']`))) {
        if (field instanceof HTMLInputElement && field.type === "radio") field.checked = field.value === value;
        else field.value = value;
      }
    }
  }, fields);
}

/** The first press: the page's form posted by a same-origin fetch; its status and body. */
async function press(page: Page): Promise<{ status: number; text: string }> {
  return page.evaluate(async () => {
    const form = document.querySelector("form[method='post']") as HTMLFormElement;
    const body = new URLSearchParams(new FormData(form) as unknown as Record<string, string>);
    const reply = await fetch(location.pathname, { method: "POST", body });
    return { status: reply.status, text: await reply.text() };
  });
}

/** No axe-core violation on the page as it stands. */
async function noViolation(page: Page): Promise<void> {
  const { violations } = await axeScan(page);
  expect(violations).toEqual([]);
}

test("TD-SEC-44: a used Quote link pressed again shows the job page with the one stored quote", async ({ page }) => {
  const quote = seedQuoteLink();
  await page.goto(`${keyedUrl()}${quote.path}`);
  await fill(page, quoteFields());
  const first = await press(page);
  expect(first.status, "the first press stores the quote").toBe(200);
  expect(first.text).toContain("Quotes so far: 1.");
  await fill(page, { ...quoteFields(), amount: "990" });
  await page.getByRole("button", { name: /^Send quote for job \d+$/ }).click();
  await expect(page.getByText("Quotes so far: 1.")).toBeVisible();
  expect(psql(`SELECT count(*) FROM quotes.quote WHERE job_id = '${quote.job}'`), "one quote stored (SEC-026)").toBe("1");
  await noViolation(page);
});

test("TD-SEC-44: a used Hold link pressed twice shows the already-used page", async ({ page }) => {
  const hold = seedHoldLink();
  await page.goto(`${keyedUrl()}${hold.path}`);
  await expect(page.getByRole("button", { name: /^Hold job / })).toBeVisible();
  psql(`UPDATE portals.login_link SET used_at = now() WHERE token_sha256 = decode('${hold.hash}', 'hex')`);
  const first = await press(page);
  expect(first.status).toBe(200);
  expect(first.text).toContain("This link was already used");
  await page.getByRole("button", { name: /^Hold job / }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("This link was already used");
  await expect(page.getByRole("link", { name: "Open your portal" })).toHaveAttribute("href", "/b");
  await noViolation(page);
});
