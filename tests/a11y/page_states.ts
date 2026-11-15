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
  // D-1516: while the pick waits for Confirm the other available quotes keep their Pick buttons
  // (PICK-032, TD-PICK-45); once the business confirmed (Payment pending) none shows.
  ["after a pick", "Pick #1", "Pick #2", quotePage("awaiting_confirm", 3)],
  ["after the business confirmed", "Quote #3", "Pick #", quotePage("payment_pending", 3)],
  ["closed", "Quote #2 (no longer available)", "Pick #", quotePage("closed", 3)],
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

/** The email action links' pages (T301, BIZ-038): an action page and the priced quote form. */
const ACTION_STATES: readonly PageState[] = [
  ["action page: pause job cards", "Pause job cards", "Send quote for job", PAUSE_LINK],
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
  { route: INBOX_ROUTE, seed: portalSeed(false), states: INBOX_STATES },
];

/** Every state of every page, in entry order. */
export const PAGE_STATES: readonly PageState[] = PAGE_ENTRIES.flatMap((entry) => entry.states);
