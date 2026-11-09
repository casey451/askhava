/// <reference types="node" />
// The Ally page-state registry (T267; A11Y-002, TD-A11Y-03; D-1239, D-1772, D-1788): data only.
//
// A shared path: a task that adds a GET route appends that route's `PAGE_ENTRIES` entry here, with
// fake data only, and never removes or weakens another task's entry. Each entry names its route,
// its states and the seed function that stores one state with fake data and returns the path that
// opens it. The checks (the route table, `NOT_PAGES`, the axe tags, which states run) live in
// `fixtures.ts`, owner-only, so nothing here can narrow what is checked. Fake data only.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";

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

/** One route's states and how to seed them. */
export interface PageEntry {
  route: string;
  /** Seeds `state` with fake data and returns the path that opens it. */
  seed: (state: PageState) => string;
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

/** Every page's entry: its route, its states and its seed function. Append a new page's entry. */
export const PAGE_ENTRIES: readonly PageEntry[] = [
  { route: QUOTE_ROUTE, seed: tokenSeed, states: QUOTE_STATES },
  { route: FEEDBACK_ROUTE, seed: tokenSeed, states: FEEDBACK_STATES },
];

/** Every state of every page, in entry order. */
export const PAGE_STATES: readonly PageState[] = PAGE_ENTRIES.flatMap((entry) => entry.states);
