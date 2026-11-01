/// <reference types="node" />
// The Ally suite's page list and checks (T053; A11Y-001 to A11Y-004, A11Y-006 to A11Y-009,
// A11Y-017, A11Y-018, A11Y-025, A11Y-026; docs/test-design.md section 6; D-1238 to D-1240).
//
// `ROUTES` is the route table read from the Rust sources: every `.route(PATH, get(...))` in
// app/*/src, with a `const NAME: &str = "..."` path resolved, the test-control `/__test/` routes left
// out. `PAGE_STATES` names each GET route's states and how to seed one with fake data; a GET route
// with no entry fails `axe_all_pages.spec.ts`, so a new page can't be skipped. `pageUrl` seeds a
// state through psql on DATABASE_URL (the database the web binary of tests/e2e/webserver.ts uses)
// once per worker. Quote-page tokens are random 64-hex strings stored only as their SHA256, the
// form `jobs::api::quote_token::resolve` looks up. The check functions run in the page and return a
// list of problems, empty when the page passes; the specs assert it is empty, and their planted
// cases assert a planted fault is reported. Fake data only.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { AxeBuilder } from "@axe-core/playwright";
import { test as base, expect, type Page } from "@playwright/test";

/** The three widths every page and state is checked at (A11Y-001, A11Y-002). */
export const WIDTHS = [320, 768, 1280] as const;

/** The axe-core rule tags A11Y-001 names. */
export const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

/** axe "incomplete" results read by hand, as "rule id on route": none yet (A11Y-025). */
export const REVIEWED_INCOMPLETE: readonly string[] = [];

/**
 * Routing calls that serve no page, as `[entry, reason]`: none yet. An entry is a GET route's path
 * or a `kind(argument)` text from `otherRoutingCalls`; anything not listed here fails A11Y-002.
 */
const NOT_PAGES: readonly (readonly [string, string])[] = [];
const notAPage = (entry: string): boolean => NOT_PAGES.some(([listed]) => listed === entry);

const REPO_ROOT = join(import.meta.dirname, "..", "..");

function rustFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "tests" ? [] : rustFiles(path);
    return entry.name.endsWith(".rs") ? [path] : [];
  });
}

/** The text of every Rust source file under app/<crate>/src, tests left out. */
function appSources(): string {
  const apps = join(REPO_ROOT, "app");
  const sources = readdirSync(apps).flatMap((crate) => rustFiles(join(apps, crate, "src")));
  return sources.map((file) => readFileSync(file, "utf8")).join("\n");
}

/** A route's path; an "unparsed route" entry when it is not a string literal or a known constant. */
function routePath(argument: string, consts: Map<string, string>): string {
  const strict = /^\s*(?:"([^"]+)"|([A-Z0-9_]+))\s*$/.exec(argument);
  const path = strict?.[1] ?? consts.get(strict?.[2] ?? "");
  return path ?? `unparsed route ${argument.trim()}`;
}

/** `route_service`, `fallback` and `nest` calls: each serves paths this scan can't list (A11Y-002). */
function otherRoutingCalls(text: string): string[] {
  const calls = /\.(route_service|fallback_service|fallback|nest_service|nest)\(\s*([^,)]*)/g;
  return [...text.matchAll(calls)].map((m) => `${m[1] ?? ""}(${(m[2] ?? "").trim()})`);
}

/**
 * Every GET route the app's modules declare, read from their sources (A11Y-002). Every
 * `.route(` call is matched; one whose path the scan can't read becomes an "unparsed route" entry,
 * and every other routing call becomes an entry too, so the coverage test fails on it until it gets
 * page states or a reasoned `NOT_PAGES` line.
 */
function readRouteTable(): string[] {
  const text = appSources();
  const consts = new Map([...text.matchAll(/const ([A-Z0-9_]+): &str = "([^"]*)";/g)].map((m) => [m[1] ?? "", m[2] ?? ""]));
  const call = /\.route\(\s*([^,]+),([\s\S]*?)(?=\.route\(|\.route_service\(|\.fallback|\.nest|\.with_state|\.layer|;)/g;
  const routes = [...text.matchAll(call)].flatMap((m) => {
    const path = routePath(m[1] ?? "", consts);
    return path.startsWith("unparsed route") || /\bget\(/.test(m[2] ?? "") ? [path] : [];
  });
  const all = [...routes, ...otherRoutingCalls(text)];
  return [...new Set(all)].filter((route) => !route.startsWith("/__test/") && !notAPage(route));
}

/** The app's GET routes, the page list's source. */
export const ROUTES = readRouteTable();

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

/** Every state of every page (docs/test-design.md section 6: empty, filled, after pick, ...). */
export const PAGE_STATES: readonly PageState[] = [
  ["no quotes yet", "No quotes yet.", "Quote #1", quotePage("open", 0)],
  ["three quotes, #2 gone", "Pick #3", "Pick #2", quotePage("quotes_ready", 3)],
  ["paused by CANCEL", "is still open.", "No quotes yet.", quotePage("open", 1, PAUSED)],
  ["after a pick", "Quote #3", "Pick #", quotePage("awaiting_confirm", 3)],
  ["closed", "Quote #2 (no longer available)", "Pick #", quotePage("closed", 3)],
].map(([name = "", shows = "", hides = "", sql = ""]) => ({ route: QUOTE_ROUTE, name, shows, hides, sql, statusRegions: ["quote-status"] }));

/** The `(route, state)` pairs to run: every state of every route in the route table. */
export const STATES_TO_CHECK = PAGE_STATES.filter((state) => ROUTES.includes(state.route));

/** Seeds `state` and returns the path that opens it. */
function seed(state: PageState): string {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  const database = process.env.DATABASE_URL ?? "";
  const done = spawnSync("psql", [database, "-v", "ON_ERROR_STOP=1", "-q", "-c", state.sql.replace(":hash", hash)], {
    encoding: "utf8",
  });
  if (done.status !== 0) throw new Error(`seeding "${state.name}" failed: ${done.stderr}`);
  return state.route.replace("{token}", token);
}

/** Fixtures: `pageUrl(state)` seeds a state once per worker and returns its path. */
export const test = base.extend<object, { pageUrl: (state: PageState) => string }>({
  pageUrl: [
    async ({}, use) => {
      const paths = new Map<PageState, string>();
      await use((state) => {
        const path = paths.get(state) ?? seed(state);
        paths.set(state, path);
        return path;
      });
    },
    { scope: "worker" },
  ],
});

export { expect };

/** Opens `state` at `width` wide and `height` tall and waits for it to load. */
export async function open(page: Page, url: string, width: number, height = 900): Promise<void> {
  await page.setViewportSize({ width, height });
  const reply = await page.goto(url);
  expect(reply?.status(), url.replace(/[0-9a-f]{64}/, "{token}")).toBe(200);
}

/** axe-core with A11Y-001's tags: violations and incomplete (needs review) results, as "rule: targets". */
export async function axeScan(page: Page, rules?: string[]): Promise<{ violations: string[]; incomplete: string[] }> {
  const builder = new AxeBuilder({ page }).withTags(WCAG_TAGS);
  const results = await (rules === undefined ? builder : builder.withRules(rules)).analyze();
  const name = (r: { id: string; nodes: { target: unknown }[] }): string =>
    `${r.id}: ${r.nodes.map((n) => JSON.stringify(n.target)).join(", ")}`;
  return { violations: results.violations.map(name), incomplete: results.incomplete.map(name) };
}

/** WCAG 1.4.12's text-spacing override (A11Y-009). */
export const TEXT_SPACING = `* { line-height: 1.5 !important; letter-spacing: 0.12em !important;
  word-spacing: 0.16em !important; } p { margin-bottom: 2em !important; }`;

/** The elements a pointer can activate (A11Y-006). Runs in the page. */
export const TARGETS =
  'a[href], button, input:not([type="hidden"]), select, textarea, summary, [role="button"], [role="link"], [tabindex="0"]';
