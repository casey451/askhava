/// <reference types="node" />
// The Ally suite's page list and checks (T053; A11Y-001 to A11Y-004, A11Y-006 to A11Y-009,
// A11Y-017, A11Y-018, A11Y-025, A11Y-026; docs/test-design.md section 6; D-1238 to D-1240).
//
// `ROUTES` is the route table read from the Rust sources: every `.route(PATH, get(...))` in
// app/*/src, with a `const NAME: &str = "..."` path resolved, the test-control `/__test/` routes left
// out. The page states are data in `page_states.ts` (T267, D-1788, TD-A11Y-03): each route's entry
// names its states and its seed function; `PAGE_STATES` is re-exported here, and a GET route with no
// entry fails `axe_all_pages.spec.ts`, so a new page can't be skipped. Which states run
// (`STATES_TO_CHECK`), the route table, `NOT_PAGES`, the axe tags and the fixture stay here, so the
// shared data file can never narrow what is checked. `pageUrl` seeds a state once per worker with
// its own route's seed function. The check functions run in the page and return a
// list of problems, empty when the page passes; the specs assert it is empty, and their planted
// cases assert a planted fault is reported. Fake data only.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { AxeBuilder } from "@axe-core/playwright";
import { test as base, expect, type Page } from "@playwright/test";
import { PAGE_ENTRIES, PAGE_STATES, type PageState } from "./page_states";

/** The three widths every page and state is checked at (A11Y-001, A11Y-002). */
export const WIDTHS = [320, 768, 1280] as const;

/** The axe-core rule tags A11Y-001 names. */
export const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

/** axe "incomplete" results read by hand, as "rule id on route": none yet (A11Y-025). */
export const REVIEWED_INCOMPLETE: readonly string[] = [];

/**
 * Routing calls that serve no page, as `[entry, reason]`. An entry is a GET route's path
 * or a `kind(argument)` text from `otherRoutingCalls`; anything not listed here fails A11Y-002.
 */
const NOT_PAGES: readonly (readonly [string, string])[] = [
  ["/static/portals/feedback.css", "the feedback page's stylesheet, checked through /f/{token} (T230)"],
];
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

export { PAGE_STATES, type PageState };

/** The `(route, state)` pairs to run: every state of every route in the route table. */
export const STATES_TO_CHECK = PAGE_STATES.filter((state) => ROUTES.includes(state.route));

/**
 * Seeds `state` with its own route's seed function and returns the path that opens it
 * (TD-A11Y-03). A route with no entry, or with more than one, fails rather than guess.
 */
function seed(state: PageState): string {
  const entries = PAGE_ENTRIES.filter((entry) => entry.route === state.route);
  const [entry] = entries;
  if (entry === undefined || entries.length > 1) {
    throw new Error(`${String(entries.length)} page_states.ts entries for ${state.route}; want exactly one`);
  }
  return entry.seed(state);
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
