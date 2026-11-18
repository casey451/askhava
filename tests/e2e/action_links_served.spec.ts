/// <reference types="node" />
// TD-SEC-67: the email action links served, in a browser, on T452's keyed web binary (T455,
// D-3675, D-3839; BIZ-038, PICK-040, SEC-014, SEC-026).
//
// Each link is seeded through psql on DATABASE_URL: a portals.login_link row holding the SHA-256
// of `action:<kind>:<object>:<token>` (D-1955's hash) for a token chosen here, as
// tests/a11y/page_states.ts seeds its action-link states. The Quote link's job is an open AC
// repair job carded to a fake active business and its card sent (D-3189). The quote form and the
// Hold page open in a browser; opening them uses nothing. With the business's hourly quote count
// already at `admit_quote`'s per-business limit, the form's POST is refused, stores no quote and
// leaves the link unused (SEC-014, D-2206). An owner with a linked fake phone signs in through
// the served sign-in link (a portals.login_link row for the bare token's hash) and the Verify
// stand-in's code, computed as tests/e2e/business_portal_served.spec.ts does, and presses an Ask about
// a charge link on a seeded $25 charge (payments.charge_evidence, keyed by pick, as T062 and T189
// store it): the page answers with the job number, the small-job tier and the "Why was I charged?"
// link, and one help request is queued (BIZ-032, FEE-031, D-225, D-3839). POSTs go with
// the browser's own headers (D-4292). No test-control route; fake data only; no token is printed.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import { test, expect } from "./fixtures";
import { keyedUrl, seal } from "./keyed";

/** `portals`' QUOTES_PER_BUSINESS: quote submissions per business per hour (SEC-014). */
const QUOTES_PER_BUSINESS = 30;

/** Runs `sql` through psql on DATABASE_URL and returns its rows, tab-split. */
function rows(sql: string): string[][] {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-AtqF", "\t", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed with status ${String(done.status)}`);
  return done.stdout.split("\n").filter((line) => line !== "").map((line) => line.split("\t"));
}

/** A link for `kind` on `object`: its token and the SQL hex of its stored hash (D-1955). */
function link(kind: string, object: string): { path: string; hash: string } {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(`action:${kind}:${object}:${token}`).digest("hex");
  return { path: `/b/a/${kind}/${object}/${token}`, hash };
}

/** An open AC repair job carded to a new fake active business, its card sent, and a live Quote link for the owner. */
function seedQuoteLink(): { path: string; business: string; job: string } {
  const [business, job] = [randomUUID(), randomUUID()];
  const quote = link("quote", job);
  rows(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id),
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
    INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
      SELECT decode('${quote.hash}', 'hex'), m.id, m.business_id, now() - interval '1 day', now() + interval '2 hours' FROM m`);
  return { path: quote.path, business, job };
}

/** A live Hold link on a random pick for a fake member of a fake business. */
function seedHoldLink(): string {
  const hold = link("hold", randomUUID());
  rows(`INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
    VALUES (decode('${hold.hash}', 'hex'), gen_random_uuid(), gen_random_uuid(), now() - interval '1 day', now() + interval '2 hours')`);
  return hold.path;
}

/** The owner's fake texting phone, as the vault stores it (`+1` and ten digits). */
const OWNER_PHONE = "+19285550143";

/** The code the Verify stand-in issues for `phone` (stand_ins/twilio.py `start_verification`). */
function verifyCode(phone: string): string {
  let sum = 0;
  for (let i = 0; i < phone.length; i += 1) sum += phone.charCodeAt(i);
  return String(sum % 1_000_000).padStart(6, "0");
}

/** A new active business, its owner with a linked fake phone (sealed) and a sign-in link for `token`. */
function seedOwner(token: string): { business: string; owner: string } {
  const [business, owner, line] = [randomUUID(), randomUUID(), randomUUID()];
  const hash = createHash("sha256").update(token).digest("hex");
  rows(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (id, business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT '${owner}', id, gen_random_uuid(), true, true, true, now()
      FROM b RETURNING id, business_id),
    p AS (INSERT INTO businesses.staff_phone (id, business_id, linked_by, linked_at)
      SELECT '${line}', business_id, id, now() FROM m RETURNING id),
    c AS (INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version,
      consented_at, line_id) SELECT m.business_id, m.id, 'text', 'con019-v1', now(), p.id FROM m, p),
    v AS (INSERT INTO privacy.staff_phone (line_id, number)
      VALUES ('${line}', decode('${seal("privacy.staff_phone", "number", line, OWNER_PHONE).toString("hex")}', 'hex')))
    INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
    SELECT decode('${hash}', 'hex'), id, business_id, now(), now() + interval '15 minutes' FROM m`);
  return { business, owner };
}

/** A job with a succeeded $25 charge of a new pick for `business`, and a live Ask about a charge link on that pick for `owner`. */
function seedCharge(business: string, owner: string): { path: string; pick: string; number: string } {
  const [pick, press, job] = [randomUUID(), randomUUID(), randomUUID()];
  const ask = link("ask-charge", pick);
  rows(`WITH j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES ('${job}', 'connected', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    i AS (INSERT INTO payments.pick_intent VALUES ('${pick}', '${business}', '${job}', 2500, 'pi_${pick}', false, now(), NULL)),
    a AS (INSERT INTO payments.charge_attempt VALUES ('${pick}', 1, '${press}', 'succeeded', now(), now())),
    e AS (INSERT INTO payments.charge_evidence VALUES ('${pick}', '${business}', '${job}', '${press}', 1,
      'pick-${pick}-try1', 'cus_FakeT455', 'pi_${pick}', NULL, 2500, now(), now()))
    INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
    VALUES (decode('${ask.hash}', 'hex'), '${owner}', '${business}', now(), now() + interval '2 hours')`);
  const number = rows(`SELECT number FROM jobs.job_number WHERE job_id = '${job}'`)[0]?.[0] ?? "";
  return { path: ask.path, pick, number };
}

/** Whether the link at `path` is still unused. */
function unused(path: string): boolean {
  const [, , , kind = "", object = "", token = ""] = path.split("/");
  const hash = createHash("sha256").update(`action:${kind}:${object}:${token}`).digest("hex");
  return rows(`SELECT used_at IS NULL FROM portals.login_link WHERE token_sha256 = decode('${hash}', 'hex')`)[0]?.[0] === "t";
}

test("TD-SEC-67: the quote form and the Hold page open in a browser and use nothing", async ({ page }) => {
  const quote = seedQuoteLink();
  await page.goto(`${keyedUrl()}${quote.path}`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Quote job \d+$/);
  await expect(page.getByRole("button", { name: /^Send quote for job \d+$/ })).toBeVisible();
  expect(unused(quote.path), "opening the quote form uses nothing (SEC-026)").toBe(true);

  const hold = seedHoldLink();
  await page.goto(`${keyedUrl()}${hold}`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Hold job .*for 2 more business hours$/);
  await expect(page.getByRole("button", { name: /^Hold job / })).toBeVisible();
  expect(unused(hold), "opening the Hold page uses nothing (SEC-026)").toBe(true);
});

test("TD-SEC-67: a quote form POST past admit_quote's per-business limit stores nothing", async ({ page }) => {
  const quote = seedQuoteLink();
  rows(`INSERT INTO core.rate_limit (bucket, window_start, hits)
    SELECT 'portals.quotes_per_business:id:${quote.business}', w, ${String(QUOTES_PER_BUSINESS)}
    FROM (VALUES (date_bin('1 hour', now(), timestamptz '1970-01-01 00:00:00+00')),
      (date_bin('1 hour', now() + interval '1 hour', timestamptz '1970-01-01 00:00:00+00'))) AS t(w)`);
  await page.goto(`${keyedUrl()}${quote.path}`);
  await page.getByRole("button", { name: /^Send quote for job \d+$/ }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Your quote wasn't sent");
  expect(rows(`SELECT count(*) FROM quotes.quote WHERE job_id = '${quote.job}'`)).toEqual([["0"]]);
  expect(unused(quote.path), "a refused quote leaves the link unused").toBe(true);
});

test("TD-SEC-67: a signed-in owner's Ask about a charge link answers with the charge once and opens one help request", async ({ page }) => {
  const token = randomBytes(32).toString("hex");
  const ids = seedOwner(token);
  const charge = seedCharge(ids.business, ids.owner);
  await page.goto(`${keyedUrl()}/b/sign-in/${token}`);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByLabel("Sign-in code").fill(verifyCode(OWNER_PHONE));
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("You're signed in");
  await page.goto(`${keyedUrl()}${charge.path}`);
  expect(unused(charge.path), "opening the page uses nothing (SEC-026)").toBe(true);
  await page.getByRole("button", { name: /^Ask about the charge for job/ }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("We got your question");
  await expect(page.locator("#charge-answer")).toContainText(`job ${charge.number}, the introduction fee for a small job`);
  await expect(page.getByRole("link", { name: "Why was I charged?" })).toHaveAttribute("href", `/b/charges/${charge.pick}`);
  expect(unused(charge.path), "the answered question used the link").toBe(false);
  const asked = rows(`SELECT count(*) FROM core.task WHERE kind = 'help_request_opened' AND payload->>'charge' = '${charge.pick}'`);
  expect(asked, "one help request (OPS-036)").toEqual([["1"]]);
});
