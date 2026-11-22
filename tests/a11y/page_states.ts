/// <reference types="node" />
// The Ally page-state registry (T267; A11Y-002, TD-A11Y-03; D-1239, D-1772, D-1788): data only.
//
// A shared path: a task that adds a GET route appends that route's `PAGE_ENTRIES` entry here, with
// fake data only, and never removes or weakens another task's entry. Each entry names its route,
// its states and the seed function that stores one state with fake data and returns the path that
// opens it. The checks (the route table, `NOT_PAGES`, the axe tags, which states run) live in
// `fixtures.ts`, owner-only, so nothing here can narrow what is checked. Fake data only.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import { keyedUrl, seal } from "../e2e/keyed";

/** One seeded state of a page. */
export interface PageState {
  route: string;
  name: string;
  /** IDs of the `role="status"` regions the state must carry from first render (A11Y-018). */
  statusRegions: string[];
  /** A line only this state shows, so a seed that opens the wrong state fails. */
  shows: string;
  /** A line this state must not show. */
  hides: string;
  /** SQL that seeds the state; `:hash` is the token's SHA256 in hex. */
  sql: string;
  /** The HTTP status the state answers with; `open` expects 200 when it is left out (T615). */
  status?: number;
}

/**
 * What a seed returns: the path that opens the state, or an absolute URL (on the keyed binary of
 * tests/e2e/keyed.ts) or a path with the cookies `open` sets for its origin first (T452, D-3674).
 */
export type Seeded = string | { url: string; cookies: readonly { name: string; value: string }[] };

/** One route's states and how to seed them. */
export interface PageEntry {
  route: string;
  /** Seeds `state` with fake data and returns what opens it. */
  seed: (state: PageState) => Seeded;
  states: readonly PageState[];
}

/**
 * Seeds a `{token}` route's state through psql on DATABASE_URL (the database the web binary of
 * tests/e2e/webserver.ts uses): a random 64-hex token, stored only as its SHA256 (the form
 * `jobs::api::quote_token::resolve` looks up), put in the route's `{token}`.
 */
export function tokenSeed(state: PageState): string {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  const database = process.env.DATABASE_URL ?? "";
  const done = spawnSync("psql", [database, "-v", "ON_ERROR_STOP=1", "-q", "-c", state.sql.replace(":hash", hash)], {
    encoding: "utf8",
  });
  if (done.status !== 0) throw new Error(`seeding "${state.name}" failed: ${done.stderr}`);
  return state.route.replace("{token}", token);
}

function quotePage(status: string, quotes: number, extra = ""): string {
  return `WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('${status}', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    t AS (INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode(':hash', 'hex'), now() FROM j)${extra}
    INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes,
      original, ai_read, confirmed, received_at, unavailable_at)
    SELECT j.id, gen_random_uuid(), 'firm', 19000, false, '', 'Sample wording from business ' || q.n,
      false, true, now() + q.n * interval '1 minute', CASE WHEN q.n = 2 THEN now() END
    FROM j, generate_series(1, ${String(quotes)}) AS q(n)`;
}

const PAUSED = `,
    h AS (INSERT INTO intake.homeowner (id, contact_id, created_at)
      VALUES (gen_random_uuid(), gen_random_uuid(), now()) RETURNING id),
    c AS (INSERT INTO intake.conversation (id, homeowner_id, job_id, language, first_inbox_id,
      consent_template, consent_sent_at, opened_at)
      SELECT gen_random_uuid(), h.id, j.id, 'en', gen_random_uuid(), 'consent', now(), now()
      FROM h, j RETURNING id, job_id),
    p AS (INSERT INTO intake.homeowner_pause (conversation_id, job_id, inbox_id, paused_at)
      SELECT c.id, c.job_id, gen_random_uuid(), now() FROM c)`;

const QUOTE_ROUTE = "/q/{token}";

/** The quote page's states (T053, T246; docs/test-design.md section 6: empty, filled, after pick, ...). */
const QUOTE_STATES: readonly PageState[] = [
  ["no quotes yet", "No quotes yet.", "Quote #1", quotePage("open", 0)],
  ["three quotes, #2 gone", "Pick #3", "Pick #2", quotePage("quotes_ready", 3)],
  ["paused by CANCEL", "is still open.", "No quotes yet.", quotePage("open", 1, PAUSED)],
  // T308 (JOB-019, D-2128): an open job's page with its close button.
  ["with the close button", "Close request", "is closed.", quotePage("open", 1)],
  // D-1516: while the pick waits for Confirm the other available quotes keep their Pick buttons
  // (PICK-032, TD-PICK-45); once the business confirmed (Payment pending) none shows.
  ["after a pick", "Pick #1", "Pick #2", quotePage("awaiting_confirm", 3)],
  ["after the business confirmed", "Quote #3", "Pick #", quotePage("payment_pending", 3)],
  ["closed", "Quote #2 (no longer available)", "Pick #", quotePage("closed", 3)],
  // T608 (D-4252): a long list, so the 900 ms end-time check sees the capped stagger.
  ["eight quotes", "Pick #8", "Quote #9", quotePage("quotes_ready", 8)],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: QUOTE_ROUTE, name, shows, hides, sql, statusRegions: ["quote-status"] }));

const FEEDBACK_ROUTE = "/f/{token}";

/** A Done job with a live link token and, when `feedback` is set, its feedback stored (T230, JOB-017). */
function feedbackPage(feedback: boolean): string {
  const stored = feedback
    ? `,
    f AS (INSERT INTO jobs.feedback (job_id, body, share, received_at)
      SELECT id, 'Sample feedback words', false, now() FROM j)`
    : "";
  return `WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('done', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j)${stored}
    INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode(':hash', 'hex'), now() FROM j`;
}

/** The private feedback page's states: the form, and the thanks once feedback is stored (T230). */
const FEEDBACK_STATES: readonly PageState[] = [
  ["feedback form", "with my first name and town", "Thanks.", feedbackPage(false)],
  ["feedback sent", "Thanks.", "Send feedback", feedbackPage(true)],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: FEEDBACK_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const CHECK_IN_ROUTE = "/c/{token}/{answer}";

/** A job in `status` with a live link token: Connected waits for a check-in answer (T231, TXT-034). */
function checkInPage(status: string): string {
  return `WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('${status}', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j)
    INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode(':hash', 'hex'), now() FROM j`;
}

/** Seeds a check-in state and opens it with the day-3 DONE button's answer code. */
function checkInSeed(state: PageState): string {
  return tokenSeed(state).replace("{answer}", "3-done");
}

/** The check-in answer page's states: the answer with its Send button, and the closed line (T231). */
const CHECK_IN_STATES: readonly PageState[] = [
  ["check-in answer", "You picked:", "nothing to send", checkInPage("connected")],
  ["check-in closed", "nothing to send for request", "You picked:", checkInPage("done")],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: CHECK_IN_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const SIGNIN_ROUTE = "/b/sign-in";
const SIGNIN_LINK_ROUTE = "/b/sign-in/{token}";

/** The business sign-in form (T280, SEC-004): no seed, the plain path. */
const SIGNIN_STATES: readonly PageState[] = [
  ["business sign-in form", "Email address", "Press the button", ""],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: SIGNIN_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** A sign-in link's page (T280, SEC-026): a live link shows its button, an unknown one has ended. */
const SIGNIN_LINK_STATES: readonly PageState[] = [
  [
    "business sign-in link open",
    "Press the button to finish signing in",
    "stopped working",
    `INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
      VALUES (decode(':hash', 'hex'), gen_random_uuid(), gen_random_uuid(), now(), now() + interval '15 minutes')`,
  ],
  ["business sign-in link ended", "This sign-in link has stopped working", "Check your email", "SELECT 1"],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: SIGNIN_LINK_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const SIGNUP_ROUTE = "/b/signup";

/** The business sign-up form (T302, CON-019): no seed, the plain path; `main` stores the catalog. */
const SIGNUP_STATES: readonly PageState[] = [
  ["business sign-up form", "What work do you do?", "Where your code goes", ""],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: SIGNUP_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const ACTION_ROUTE = "/b/a/{kind}/{object}/{token}";

/** The action kind each action-link state opens, by state name (T301). */
const ACTION_KINDS: Readonly<Record<string, string>> = {
  "action page: pause job cards": "pause",
  "action page: away": "away",
  "quote form from a card's Quote link": "quote",
};

/**
 * Seeds an action-link state (T301, D-1955): a random token and object; the `portals.login_link`
 * row holds the SHA-256 of `action:<kind>:<object>:<token>` (`:hash`), and `:object` is the
 * object's ID in the state's SQL.
 */
function actionSeed(state: PageState): string {
  const kind = ACTION_KINDS[state.name] ?? "";
  const object = randomUUID();
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(`action:${kind}:${object}:${token}`).digest("hex");
  const sql = state.sql.replaceAll(":object", object).replace(":hash", hash);
  const database = process.env.DATABASE_URL ?? "";
  const done = spawnSync("psql", [database, "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding "${state.name}" failed: ${done.stderr}`);
  return state.route.replace("{kind}", kind).replace("{object}", object).replace("{token}", token);
}

/** A live action link for a fake member of business `:object` (a session kind's object). */
const PAUSE_LINK = `INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
  VALUES (decode(':hash', 'hex'), gen_random_uuid(), ':object', now() - interval '1 day', now() + interval '2 hours')`;

/**
 * An open AC repair job `:object` with its facts row, carded to a fake active business whose owner
 * holds the live Quote link, its card kept and sent under its own digest (D-3189, D-3675).
 */
const QUOTE_LINK = `WITH b AS (INSERT INTO businesses.business (status, created_at, status_changed_at)
      VALUES ('active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id),
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES (':object', 'open', false, now(), now(), now()) RETURNING id),
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
      SELECT decode(':hash', 'hex'), m.id, m.business_id, now() - interval '1 day', now() + interval '2 hours' FROM m`;

/** The email action links' pages (T301, BIZ-038): action pages (Away's hours field: T543, D-3892) and the priced quote form. */
const ACTION_STATES: readonly PageState[] = [
  ["action page: pause job cards", "Pause job cards", "Send quote for job", PAUSE_LINK],
  ["action page: away", "Hours until you're back", "Fix this to continue", PAUSE_LINK],
  ["quote form from a card's Quote link", "Send quote for job", "Fix these", QUOTE_LINK],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: ACTION_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const PORTAL_ROUTE = "/b";
const RELEASE_ROUTE = "/b/picks/{pick}";
const BILLING_ROUTE = "/b/billing";
const NOTICES_ROUTE = "/b/notices";

/** The fake notice address the notice page's seed seals into the vault. */
const NOTICE_ADDRESS = "owner@example.test";

/**
 * A fake active business, its owner (sign-in mailbox `:mailbox`) and a business session for the
 * token whose SHA256 is `:hash` (T300, D-3674), then `extra`, which may use the owner row `m`.
 */
function businessSql(extra: string): string {
  return `WITH b AS (INSERT INTO businesses.business (status, created_at, status_changed_at)
      VALUES ('active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, ':mailbox', true, true, true, now() FROM b
      RETURNING id, business_id),
    s AS (INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at)
      SELECT decode(':hash', 'hex'), 'business', id, now(), now() + interval '30 days' FROM m RETURNING id),
    bs AS (INSERT INTO portals.business_session (session_id, business_id) SELECT s.id, m.business_id FROM s, m)${extra}
    SELECT 1`;
}

/**
 * Seeds a business portal state through psql: `state.sql` with a random session token's SHA256,
 * a mailbox ID, the address sealed for it in the keyed binary's vault format and a pick ID; returns
 * the route (its `{pick}` filled) with the `__Host-askhava-session` cookie, on the keyed binary when
 * `keyed` (the pages that read the vault, D-3674).
 */
function portalSeed(keyed: boolean): (state: PageState) => Seeded {
  return (state) => {
    const token = randomBytes(32).toString("hex");
    const [mailbox, pick] = [randomUUID(), randomUUID()];
    const sealed = keyed ? seal("privacy.business_mailbox", "address", mailbox, NOTICE_ADDRESS).toString("hex") : "";
    const sql = state.sql
      .replaceAll(":hash", createHash("sha256").update(token).digest("hex"))
      .replaceAll(":mailbox", mailbox)
      .replaceAll(":sealed", sealed)
      .replaceAll(":pick", pick);
    const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
    if (done.status !== 0) throw new Error(`seeding "${state.name}" failed: ${done.stderr}`);
    const path = state.route.replace("{pick}", pick);
    return { url: keyed ? `${keyedUrl()}${path}` : path, cookies: [{ name: "__Host-askhava-session", value: token }] };
  };
}

/** The business portal home (T300; BIZ-025, CARD-022): the owner signed in. */
const PORTAL_STATES: readonly PageState[] = [
  ["business portal home", "Your numbers this month", "There is a problem", businessSql("")],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: PORTAL_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** The released job's page (T300; PICK-024): a $0 release whose contact the vault no longer holds. */
const RELEASE_STATES: readonly PageState[] = [
  [
    "released job, details removed",
    "We removed the homeowner's details",
    "Tip: on your first call",
    businessSql(`,
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT ':pick', gen_random_uuid(), business_id, 1, 'page', 'free', now() FROM m RETURNING id, job_id, business_id),
    r AS (INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, contact_id, mail_chosen)
      SELECT id, job_id, business_id, 'no_fee', now(), gen_random_uuid(), false FROM p)`),
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: RELEASE_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** The billing page (T300; BIZ-015): the owner, nothing on the ledger yet. */
const BILLING_STATES: readonly PageState[] = [
  ["billing, empty statement", "Nothing on this statement yet.", "card ending in", businessSql("")],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: BILLING_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** The notice settings (T300; BIZ-036, SEC-024): the owner's verified address, opened from the vault. */
const NOTICES_STATES: readonly PageState[] = [
  [
    "notice settings, owner",
    NOTICE_ADDRESS,
    "Not confirmed yet",
    businessSql(`,
    v AS (INSERT INTO privacy.business_mailbox (id, address) VALUES (':mailbox', decode(':sealed', 'hex'))),
    c AS (INSERT INTO businesses.mailbox_check (business_id, member_id, mailbox_id, open, sent_at, window_at, verified_at)
      SELECT business_id, id, ':mailbox', false, now(), now(), now() FROM m)`),
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: NOTICES_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const CONFIRM_ROUTE = "/b/picks/{pick}/confirm";

/**
 * A fake owner's page pick waiting for Confirm, its fee row's mode, kind and cents as SQL over `t`,
 * then `extra`, which may use the pick row `p`.
 */
function confirmSql(mode: string, kind: string, cents: number, extra = ""): string {
  return businessSql(`,
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES (gen_random_uuid(), 'awaiting_confirm', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT ':pick', j.id, m.business_id, 1, 'page', 'awaiting_confirm', now() FROM j, m RETURNING id, job_id, business_id),
    f AS (INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents, terms_version, fixed_at)
      SELECT p.id, p.business_id, p.job_id, ${mode}, '${kind}', ${String(cents)}, t.authorization_version, now()
      FROM p, payments.fee_terms t)${extra}`);
}

/**
 * The card section's Stripe customer and the pick's one PaymentIntent, stored with fake IDs made
 * from the business and pick IDs (D-4950), so the page opens no Stripe object: the stand-in's IDs
 * restart with each run and collide with rows a reused database kept, which fell to CARD_OFF_LINE.
 */
const OPENED_SQL = `,
    c AS (INSERT INTO payments.customer (business_id, stripe_customer_id)
      SELECT business_id, 'cus_fake' || replace(business_id::text, '-', '') FROM p),
    i AS (INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at)
      SELECT id, business_id, job_id, 2500, 'pi_fake' || replace(id::text, '-', ''), false, now() FROM p)`;

/**
 * The Confirm page (T261; FEE-045, PICK-021, D-1871, D-3836, D-3838): a fee-free pick waiting for
 * the owner's Confirm whose stored fee mode is the other of `tiers` and `flat` than the mode in
 * force, so the page shows the fee-terms-changed line in place of the card section, opens no
 * PaymentIntent and loads neither Stripe.js nor the page script; on the keyed binary, whose Stripe
 * settings serve the page. T496 (A11Y-002, D-3838): an ordinary $25 charged pick under the terms in
 * force, whose card section shows with the fixture's fake Stripe Element mounted; its `hides` is
 * `CARD_OFF_LINE`, so a page whose card section fell to that line fails (D-4950).
 */
const CONFIRM_STATES: readonly PageState[] = [
  [
    "confirm page, fee terms changed",
    "The fee terms changed after the homeowner picked you",
    "Card payments can't be taken right now.",
    confirmSql("CASE t.mode WHEN 'tiers' THEN 'flat' ELSE 'tiers' END", "fee_free", 0),
  ],
  ["confirm page, card section", "Confirm and pay", "Card payments can't be taken right now.", confirmSql("t.mode", "charged", 2500, OPENED_SQL)],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: CONFIRM_ROUTE, name, shows, hides, sql, statusRegions: ["confirm-status"] }));

const CANT_TAKE_ROUTE = "/b/picks/{pick}/cant-take";

/**
 * The "Can't take it" page (T472; PICK-050, SEC-026, A11Y-007, D-3750): the signed-in owner on
 * her business's pick waiting for its Confirm; the page shows what the button does and one CSRF
 * button, and opening it changes nothing.
 */
const CANT_TAKE_STATES: readonly PageState[] = [
  [
    "can't take it, waiting pick",
    "This ends the homeowner",
    "find that page",
    businessSql(`,
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES (gen_random_uuid(), 'awaiting_confirm', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT ':pick', j.id, m.business_id, 1, 'page', 'awaiting_confirm', now() FROM j, m)`),
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: CANT_TAKE_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const INBOX_ROUTE = "/b/signup/inbox";

/**
 * The sign-up inbox check after activation (T462; BIZ-042, D-3676): the signed-in owner before the
 * sample is sent, and after a Spam answer with the mail app's steps. No vault is read.
 */
const INBOX_STATES: readonly PageState[] = [
  ["inbox check, nothing sent", "Send me a sample job card", "Send the sample again", businessSql("")],
  [
    "inbox check, sample in spam",
    "Move our emails to your inbox",
    "Send me a sample job card",
    businessSql(`,
    e AS (INSERT INTO businesses.email_check (member_id, business_id, sample_sent_at, answer, answered_at)
      SELECT id, business_id, now(), 'spam', now() FROM m)`),
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: INBOX_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const CAPABILITY_ROUTE = "/b/signup/capability";

/**
 * The sign-up capability step after the first sign-in (T464; BIZ-049, D-3676): the signed-in owner
 * of a fake active business with no saved step, the one-page form. No vault is read.
 */
const CAPABILITY_STATES: readonly PageState[] = [
  ["capability step, one-page form", "Which towns do you work in?", "Save and continue", businessSql("")],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: CAPABILITY_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/**
 * The public site pages (T102; WEB-001, WEB-012), the legal and data pages (T221; WEB-014,
 * TERMS-021, TERMS-023) and the services, coverage, help and ROC pages (T201; WEB-008, WEB-011,
 * WEB-015, WEB-023): no seed, the plain path. The old directory paths
 * answer with the home page (WEB-012).
 */
const SITE_PAGE_STATES: readonly PageState[] = [
  ["/", "home page", "without giving out your number", "Para propietarios"],
  ["/categories", "old categories path, home page", "without giving out your number", "Para propietarios"],
  ["/events", "old events path, home page", "without giving out your number", "Para propietarios"],
  ["/how-it-works", "how it works", "Tell us about a job you need done.", "Para propietarios"],
  ["/es/how-it-works", "how it works in Spanish", "Cuéntenos de un trabajo que necesita.", "Homeowners"],
  ["/terms", "terms, homeowner then business", "Business Terms", "Para propietarios"],
  ["/privacy", "privacy policy", "How long we keep it", "Para propietarios"],
  ["/sms-terms", "SMS terms", "How you agree", "Para propietarios"],
  ["/your-data", "homeowner data page", "What we never ask", "Para propietarios"],
  ["/es/your-data", "homeowner data page in Spanish", "Lo que nunca preguntamos", "Homeowners"],
  ["/business-data", "business data page", "A log of each email we send you", "Para propietarios"],
  ["/es/business-data", "business data page in Spanish", "Su registro de avisos", "Homeowners"],
  ["/services", "services, licensed trades", "Get quotes from licensed plumbers.", "Para propietarios"],
  ["/coverage", "what we cover now", "may charge roaming fees.", "Para propietarios"],
  ["/help", "help page", "Messages are not watched around the clock.", "Para propietarios"],
  ["/help/roc-complaint", "ROC complaint page", "File a complaint on the ROC website", "Para propietarios"],
  ["/accessibility", "accessibility statement", "If you use a relay service, you can reach us by text or email.", "Para propietarios"],
  ["/es", "home page in Spanish", "Su número nunca se comparte.", "Homeowners"],
  ["/business", "business page", "You quote once. No bidding rounds, no paid placement.", "Para propietarios"],
  ["/es/business", "business page in Spanish", "Sin rondas de ofertas, sin posiciones pagadas.", "Homeowners"],
  ["/business/help", "business help page", "Safety when you work alone", "Para propietarios"],
  ["/request", "request page and web chat, first question", "What job do you need done?", "Your chat with"],
].map(([route = "", name = "", shows = "", hides = ""]) => ({ route, name, shows, hides, sql: "", statusRegions: [] }));
const OWNER_SIGNIN_ROUTE = "/owner/sign-in";
const OWNER_STEP_UP_ROUTE = "/owner/step-up";

/** The owner sign-in form (T303, SEC-005): no seed, the plain path; no passkey is registered. */
const OWNER_SIGNIN_STATES: readonly PageState[] = [
  ["owner sign-in form", "The 6-digit code from your authenticator app, or one of your paper recovery codes.", "That didn't work", ""],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: OWNER_SIGNIN_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/**
 * The owner's step-up form (T303, SEC-043): a fake owner session (its own `portals.owner_session`
 * row, so `owner_principal` admits it) for the token whose SHA256 is `:hash`.
 */
const OWNER_STEP_UP_STATES: readonly PageState[] = [
  [
    "owner step-up form",
    "Settings, refunds and switches need a new code from your authenticator app first.",
    "That code didn't work",
    `WITH s AS (INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at)
      VALUES (decode(':hash', 'hex'), 'owner', gen_random_uuid(), now(), now() + interval '30 minutes') RETURNING id)
    INSERT INTO portals.owner_session (session_id, last_seen_at, factor_at) SELECT id, now(), now() FROM s`,
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: OWNER_STEP_UP_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** A fake owner session (T310): its own `portals.owner_session` row, for the token hashing to `:hash`. */
const OWNER_SESSION_SQL = `WITH s AS (INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at)
      VALUES (decode(':hash', 'hex'), 'owner', gen_random_uuid(), now(), now() + interval '30 minutes') RETURNING id)
    INSERT INTO portals.owner_session (session_id, last_seen_at, factor_at) SELECT id, now(), now() FROM s`;
const OWNER_HOME_ROUTE = "/owner/home";
const OWNER_SETTINGS_ROUTE = "/owner/settings";

/** The owner home (T310; OPS-003, FEE-007, BIZ-027): the switches, fee mode, rates and sign-ups. */
const OWNER_HOME_STATES: readonly PageState[] = [
  ["owner home", "Pause or resume", "Each change needs a code from your authenticator app", OWNER_SESSION_SQL],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: OWNER_HOME_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** The owner settings (T310; LIC-002, CON-035, PICK-048): the category switches and both forms. */
const OWNER_SETTINGS_STATES: readonly PageState[] = [
  ["owner settings", "Each change needs a code from your authenticator app", "Pause or resume", OWNER_SESSION_SQL],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: OWNER_SETTINGS_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const OWNER_RELEASE_ROUTE = "/owner/release";

/** The owner's release button (T540; PICK-022, PICK-023): the empty form, no refusal. */
const OWNER_RELEASE_STATES: readonly PageState[] = [
  ["owner release button", "Re-send the release notice", "Nothing was sent", OWNER_SESSION_SQL],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: OWNER_RELEASE_ROUTE, name, shows, hides, sql, statusRegions: ["release-status"] }));

const OWNER_HELP_ROUTE = "/owner/help";

/** The owner help page (T530; OPS-036): no open request, no refusal. */
const OWNER_HELP_STATES: readonly PageState[] = [
  ["owner help requests", "No open requests", "Nothing was sent", OWNER_SESSION_SQL],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: OWNER_HELP_ROUTE, name, shows, hides, sql, statusRegions: ["help-status"] }));

const OWNER_REFUND_ROUTE = "/owner/refunds/{pick}";
const OWNER_AUDIT_LOG_ROUTE = "/owner/audit-log";
const OWNER_HEALTH_ROUTE = "/owner/health";

/**
 * The owner tools (T304; FEE-026, OPS-010, OPS-008, D-5159): a fake pick's refund review step, the
 * audit log with a seeded row and the health page with a seeded job card, so each table renders.
 */
const OWNER_TOOLS_STATES: readonly PageState[] = [
  [OWNER_REFUND_ROUTE, "owner refund review", "Disputes on this charge", "Error:", ""],
  [
    OWNER_AUDIT_LOG_ROUTE,
    "owner audit log, rows",
    "Audit log rows",
    "Nothing more to show.",
    `; INSERT INTO ops.audit_log (actor_kind, action, subject_kind, subject_id, field)
      VALUES ('system', 'ops.fake_check', 'ops.fake_record', gen_random_uuid(), 'fake_field')`,
  ],
  [
    OWNER_HEALTH_ROUTE,
    "owner health page, a card to check",
    "Job cards to check",
    "so there are none to check",
    `; INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at)
      VALUES (gen_random_uuid(), gen_random_uuid(), false, now())`,
  ],
].map(([route = "", name = "", shows = "", hides = "", extra = ""]) => ({ route, name, shows, hides, sql: OWNER_SESSION_SQL + extra, statusRegions: [] }));

const COMPLAINTS_ROUTE = "/b/complaints";
const EMERGENCY_ROUTE = "/b/emergency-emails";
const NOTICE_LOG_ROUTE = "/b/picks/{pick}/notices";
const JOB_TYPES_ROUTE = "/b/job-types";

/** The complaints page (T306; BIZ-029): the owner, one fake complaint with its reply box open. */
const COMPLAINTS_STATES: readonly PageState[] = [
  [
    "complaints, reply box open",
    "No show",
    "Your reply was not sent",
    businessSql(`,
    k AS (INSERT INTO businesses.complaint (business_id, job_id, kind, safety, received_at)
      SELECT business_id, gen_random_uuid(), 'no_show', false, now() - interval '1 hour' FROM m)`),
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: COMPLAINTS_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** The emergency-email switch (T306; BIZ-007, BIZ-042): the owner, switch off, with the after-hours box. */
const EMERGENCY_STATES: readonly PageState[] = [
  ["emergency emails, switch off", "Turn on emergency emails at any hour", "Did your phone alert you", businessSql("")],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: EMERGENCY_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** The Job types page (T375; BIZ-004, BIZ-049): the owner of a fake business with no license, so only no-license rows show. */
const JOB_TYPES_STATES: readonly PageState[] = [
  ["job types, nothing saved yet", "Your job types", "Nothing was saved", businessSql("")],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: JOB_TYPES_ROUTE, name, shows, hides, sql, statusRegions: ["job-types-result"] }));

/** The job's notice log (T306; BIZ-045): the owner's pick on a carded job, nothing sent yet; reads the vault. */
const NOTICE_LOG_STATES: readonly PageState[] = [
  [
    "notice log, nothing sent yet",
    "What we sent you about job",
    "find that page",
    businessSql(`,
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES (gen_random_uuid(), 'awaiting_confirm', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    c AS (INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) SELECT j.id, m.business_id, false, now() FROM j, m),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT ':pick', j.id, m.business_id, 1, 'page', 'awaiting_confirm', now() FROM j, m RETURNING id, job_id, business_id),
    f AS (INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents, terms_version, fixed_at)
      SELECT p.id, p.business_id, p.job_id, t.mode, 'fee_free', 0, t.authorization_version, now() FROM p, payments.fee_terms t)`),
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: NOTICE_LOG_ROUTE, name, shows, hides, sql, statusRegions: [] }));

const WHY_CHARGED_ROUTE = "/b/charges/{pick}";

/**
 * "Why was I charged?" (T562; FEE-031, D-3890): the owner's pick charged $25 on a fake $480 firm
 * quote; with no job facts the basis is derived on the default facts and gives the fee fixed at
 * the pick, so the basis line shows. Reads nothing from the vault.
 */
const WHY_CHARGED_STATES: readonly PageState[] = [
  [
    "why charged, basis shown",
    "The fee basis from your quote is $480.00",
    "find that page",
    businessSql(`,
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES (gen_random_uuid(), 'awaiting_confirm', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    q AS (INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes, original,
      ai_read, confirmed, received_at) SELECT j.id, m.business_id, 'firm', 48000, false, '', 'Fake quote wording',
      false, true, now() FROM j, m),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT ':pick', j.id, m.business_id, 1, 'page', 'awaiting_confirm', now() FROM j, m RETURNING id, job_id, business_id),
    f AS (INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents, terms_version, fixed_at)
      SELECT p.id, p.business_id, p.job_id, t.mode, 'charged', 2500, t.authorization_version, now() FROM p, payments.fee_terms t),
    i AS (INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at)
      SELECT id, business_id, job_id, 2500, 'pi_fake' || replace(id::text, '-', ''), false, now() FROM p RETURNING pick_id),
    a AS (INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at, decided_at)
      SELECT pick_id, 1, gen_random_uuid(), 'succeeded', now(), now() FROM i RETURNING pick_id, press_id),
    e AS (INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no, idempotency_label,
      stripe_customer_id, stripe_intent_id, amount_cents, pressed_at, charged_at)
      SELECT p.id, p.business_id, p.job_id, a.press_id, 1, 'pick-' || p.id::text || '-try1',
        'cus_fake' || replace(p.business_id::text, '-', ''), 'pi_fake' || replace(p.id::text, '-', ''), 2500, now(), now()
      FROM p, a)`),
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: WHY_CHARGED_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** The business step-up page (T364, SEC-038): the owner signed in, no code sent yet. */
const BUSINESS_STEP_UP_ROUTE = "/b/step-up";
const BUSINESS_STEP_UP_STATES: readonly PageState[] = [
  ["business step-up, send a code", "Send me a code", "That code didn't work", businessSql("")],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: BUSINESS_STEP_UP_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** The homeowner sign-in form (T307, SEC-008): no seed, the plain path. */
const HOMEOWNER_SIGNIN_ROUTE = "/h/sign-in";
const HOMEOWNER_SIGNIN_STATES: readonly PageState[] = [
  ["homeowner sign-in form", "Your mobile number", "We couldn't sign you in", ""],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: HOMEOWNER_SIGNIN_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/**
 * The homeowner portal home (T307; PRIV-021, CON-035): a fake contact (phone and street sealed in
 * the keyed binary's vault format) and a homeowner session on it for the token hashing to `:hash`.
 */
const HOMEOWNER_HOME_ROUTE = "/h";
const HOMEOWNER_HOME_STATES: readonly PageState[] = [
  [
    "homeowner portal home, texts on",
    "Turn texts off",
    "We couldn't sign you in",
    `WITH c AS (INSERT INTO privacy.contact (id, phone, address) VALUES (':contact', decode(':phone', 'hex'), decode(':street', 'hex')) RETURNING id)
    INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at)
      SELECT decode(':hash', 'hex'), 'homeowner', id, now(), now() + interval '30 days' FROM c`,
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: HOMEOWNER_HOME_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** Seeds a homeowner home state on the keyed binary (its vault reads her record) with her session cookie. */
function homeownerSeed(state: PageState): Seeded {
  const token = randomBytes(32).toString("hex");
  const contact = randomUUID();
  const sql = state.sql
    .replaceAll(":hash", createHash("sha256").update(token).digest("hex"))
    .replaceAll(":contact", contact)
    .replaceAll(":phone", seal("privacy.contact", "phone", contact, "+19285550164").toString("hex"))
    .replaceAll(":street", seal("privacy.contact", "address", contact, "64 Example Way").toString("hex"));
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding "${state.name}" failed: ${done.stderr}`);
  return { url: `${keyedUrl()}${state.route}`, cookies: [{ name: "__Host-askhava-session", value: token }] };
}

/**
 * The homeowner's photo upload page (T674; INT-044, PRIV-015, A11Y-027): a fake contact with a
 * homeowner session, her request and its job; the seed fills `{job}` with the job's number.
 */
const PHOTO_UPLOAD_ROUTE = "/h/requests/{job}/photos";
const PHOTO_UPLOAD_STATES: readonly PageState[] = [
  [
    "homeowner photo upload form",
    "No photo is fine too.",
    "Fix this to add your photo",
    `WITH c AS (INSERT INTO privacy.contact (id, phone) VALUES (':contact', decode(':phone', 'hex')) RETURNING id),
    s AS (INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at)
      SELECT decode(':hash', 'hex'), 'homeowner', id, now(), now() + interval '30 days' FROM c),
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES (gen_random_uuid(), 'intake', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    h AS (INSERT INTO intake.homeowner (contact_id, created_at) SELECT id, now() FROM c RETURNING id),
    v AS (INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id, consent_template,
      consent_sent_at, opened_at) SELECT h.id, j.id, 'en', gen_random_uuid(), 'consent', now(), now() FROM h, j)
    SELECT n FROM n`,
  ],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: PHOTO_UPLOAD_ROUTE, name, shows, hides, sql, statusRegions: [] }));

/** Seeds a photo upload state on the keyed binary and opens her request's page with her session cookie. */
function photoUploadSeed(state: PageState): Seeded {
  const token = randomBytes(32).toString("hex");
  const contact = randomUUID();
  const sql = state.sql
    .replaceAll(":hash", createHash("sha256").update(token).digest("hex"))
    .replaceAll(":contact", contact)
    .replaceAll(":phone", seal("privacy.contact", "phone", contact, "+19285550165").toString("hex"));
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-qtA", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding "${state.name}" failed: ${done.stderr}`);
  const url = `${keyedUrl()}${state.route.replace("{job}", done.stdout.trim())}`;
  return { url, cookies: [{ name: "__Host-askhava-session", value: token }] };
}

/** The router's fallback (`.fallback(not_found)` in app/server/src/lib.rs, T615, D-4252): an unknown path's 404 page. */
const NOT_FOUND_ROUTE = "fallback(not_found)";

/** The styled 404 page, opened at a path no route serves; it answers 404, not 200 (WEB-027). */
const NOT_FOUND_STATES: readonly PageState[] = [
  { route: NOT_FOUND_ROUTE, name: "unknown path, 404 page", statusRegions: [], shows: "We can't find that page.", hides: "Para propietarios", sql: "", status: 404 },
];

/** Every page's entry: its route, its states and its seed function. Append a new page's entry. */
export const PAGE_ENTRIES: readonly PageEntry[] = [
  { route: QUOTE_ROUTE, seed: tokenSeed, states: QUOTE_STATES },
  { route: FEEDBACK_ROUTE, seed: tokenSeed, states: FEEDBACK_STATES },
  { route: CHECK_IN_ROUTE, seed: checkInSeed, states: CHECK_IN_STATES },
  { route: SIGNIN_ROUTE, seed: (state) => state.route, states: SIGNIN_STATES },
  { route: SIGNIN_LINK_ROUTE, seed: tokenSeed, states: SIGNIN_LINK_STATES },
  { route: SIGNUP_ROUTE, seed: (state) => state.route, states: SIGNUP_STATES },
  { route: ACTION_ROUTE, seed: actionSeed, states: ACTION_STATES },
  { route: PORTAL_ROUTE, seed: portalSeed(false), states: PORTAL_STATES },
  { route: RELEASE_ROUTE, seed: portalSeed(true), states: RELEASE_STATES },
  { route: BILLING_ROUTE, seed: portalSeed(false), states: BILLING_STATES },
  { route: NOTICES_ROUTE, seed: portalSeed(true), states: NOTICES_STATES },
  { route: CONFIRM_ROUTE, seed: portalSeed(true), states: CONFIRM_STATES },
  { route: CANT_TAKE_ROUTE, seed: portalSeed(false), states: CANT_TAKE_STATES },
  { route: INBOX_ROUTE, seed: portalSeed(false), states: INBOX_STATES },
  { route: CAPABILITY_ROUTE, seed: portalSeed(false), states: CAPABILITY_STATES },
  { route: "/", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/") },
  { route: "/categories", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/categories") },
  { route: "/events", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/events") },
  { route: "/how-it-works", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/how-it-works") },
  { route: "/es/how-it-works", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/es/how-it-works") },
  { route: "/terms", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/terms") },
  { route: "/privacy", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/privacy") },
  { route: "/sms-terms", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/sms-terms") },
  { route: "/your-data", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/your-data") },
  { route: "/es/your-data", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/es/your-data") },
  { route: "/business-data", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/business-data") },
  { route: "/es/business-data", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/es/business-data") },
  { route: "/services", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/services") },
  { route: "/coverage", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/coverage") },
  { route: "/help", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/help") },
  { route: "/help/roc-complaint", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/help/roc-complaint") },
  { route: "/accessibility", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/accessibility") },
  { route: "/es", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/es") },
  { route: "/business", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/business") },
  { route: "/es/business", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/es/business") },
  { route: "/business/help", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/business/help") },
  { route: "/request", seed: (state) => state.route, states: SITE_PAGE_STATES.filter((s) => s.route === "/request") },
  { route: OWNER_SIGNIN_ROUTE, seed: (state) => state.route, states: OWNER_SIGNIN_STATES },
  { route: OWNER_STEP_UP_ROUTE, seed: portalSeed(false), states: OWNER_STEP_UP_STATES },
  { route: COMPLAINTS_ROUTE, seed: portalSeed(false), states: COMPLAINTS_STATES },
  { route: EMERGENCY_ROUTE, seed: portalSeed(false), states: EMERGENCY_STATES },
  { route: NOTICE_LOG_ROUTE, seed: portalSeed(true), states: NOTICE_LOG_STATES },
  { route: WHY_CHARGED_ROUTE, seed: portalSeed(false), states: WHY_CHARGED_STATES },
  { route: BUSINESS_STEP_UP_ROUTE, seed: portalSeed(false), states: BUSINESS_STEP_UP_STATES },
  { route: OWNER_HOME_ROUTE, seed: portalSeed(false), states: OWNER_HOME_STATES },
  { route: OWNER_SETTINGS_ROUTE, seed: portalSeed(false), states: OWNER_SETTINGS_STATES },
  { route: OWNER_RELEASE_ROUTE, seed: portalSeed(false), states: OWNER_RELEASE_STATES },
  { route: OWNER_HELP_ROUTE, seed: portalSeed(false), states: OWNER_HELP_STATES },
  { route: OWNER_REFUND_ROUTE, seed: portalSeed(false), states: OWNER_TOOLS_STATES.filter((s) => s.route === OWNER_REFUND_ROUTE) },
  { route: OWNER_AUDIT_LOG_ROUTE, seed: portalSeed(false), states: OWNER_TOOLS_STATES.filter((s) => s.route === OWNER_AUDIT_LOG_ROUTE) },
  { route: OWNER_HEALTH_ROUTE, seed: portalSeed(false), states: OWNER_TOOLS_STATES.filter((s) => s.route === OWNER_HEALTH_ROUTE) },
  { route: JOB_TYPES_ROUTE, seed: portalSeed(false), states: JOB_TYPES_STATES },
  { route: HOMEOWNER_SIGNIN_ROUTE, seed: (state) => state.route, states: HOMEOWNER_SIGNIN_STATES },
  { route: HOMEOWNER_HOME_ROUTE, seed: homeownerSeed, states: HOMEOWNER_HOME_STATES },
  { route: PHOTO_UPLOAD_ROUTE, seed: photoUploadSeed, states: PHOTO_UPLOAD_STATES },
  { route: NOT_FOUND_ROUTE, seed: () => "/no-such-page", states: NOT_FOUND_STATES },
];

/** Every state of every page, in entry order. */
export const PAGE_STATES: readonly PageState[] = PAGE_ENTRIES.flatMap((entry) => entry.states);
