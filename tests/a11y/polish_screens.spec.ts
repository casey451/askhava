// The polish screens (T608; WEB-027, WEB-029, WEB-030, A11Y-009, A11Y-033, PICK-009; DESIGN_SPEC
// sections 6, 7 and 9; D-4246, D-4252).
//
// For the home page and every quote-page state (every state in `STATES_TO_CHECK` with
// `POLISH_SCREENS=all`, for the reviewers of T609 to T612), at 320 by 740, 390 by 844, 768 by 1024
// and 1440 by 900, it fails when the page scrolls sideways, or when any animation from
// `document.getAnimations()` ends (delay plus duration times iterations) after 900 ms or repeats
// more than 3 times; then it waits for every animation to finish and saves a first-screen and a
// full-page PNG under `test-results/polish/` (never committed) for the screenshot review. On the
// render after a failed pick (`?choose=1` on a tiers quote) and on the just-closed render, no
// element has an animation (D-4252). A second quote seeded after the quote page loaded, fetched
// when Playwright's clock runs the 30 s poll, appears with no animation (DESIGN_SPEC `:127`).
// Fake data only: every seeded name and word is a sample.
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { STATES_TO_CHECK, expect, open, test, type PageState } from "./fixtures";
import { tokenSeed } from "./page_states";

/** The four screens of the screenshot review. */
const SCREENS = [
  { width: 320, height: 740 },
  { width: 390, height: 844 },
  { width: 768, height: 1024 },
  { width: 1440, height: 900 },
];

/** WEB-029: every animation is finished within 900 ms of load and repeats at most 3 times. */
const END_MS = 900;
const REPEATS = 3;

const QUOTE_ROUTE = "/q/{token}";

const STATES = process.env.POLISH_SCREENS === "all"
  ? STATES_TO_CHECK
  : STATES_TO_CHECK.filter((s) => s.route === QUOTE_ROUTE || (s.route === "/" && s.name === "home page"));

/** Runs in the page: each animation that ends after `end` ms or repeats more than `repeats` times. */
function longAnimations({ end, repeats }: { end: number; repeats: number }): string[] {
  return document.getAnimations().flatMap((a) => {
    const timing = a.effect?.getComputedTiming();
    const ends = Number(timing?.endTime ?? 0);
    const times = timing?.iterations ?? 1;
    const name = a instanceof CSSAnimation ? a.animationName : a.constructor.name;
    const out: string[] = [];
    if (ends > end) out.push(`${name} ends at ${String(ends)} ms`);
    if (times > repeats) out.push(`${name} repeats ${String(times)} times`);
    return out;
  });
}

/** Waits for the fonts and for every running animation to finish. */
async function settled(page: Page): Promise<void> {
  await page.evaluate(async () => {
    await document.fonts.ready;
    await Promise.all(document.getAnimations().map((a) => a.finished.then(() => undefined)));
  });
}

/** A file-name-safe form of a state's route and name. */
function slug(state: PageState): string {
  return `${state.route}-${state.name}`.replace(/[^a-z0-9]+/gi, "-").replace(/^-|-$/g, "").toLowerCase();
}

/** Runs `sql` on the suite's database (fake data only). */
function psql(sql: string): void {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding failed: ${done.stderr}`);
}

/** One sample quote of `format` for the job whose quote token hashes to `:hash`, `n` minutes in. */
function quoteSql(n: number, tiers: boolean): string {
  const [format, names, cents, amount] = tiers
    ? ["tiers", "ARRAY['Basic', 'Plus']", "ARRAY[10000, 80000]::bigint[]", "NULL"]
    : ["firm", "NULL", "NULL", "19000"];
  return `INSERT INTO quotes.quote (job_id, business_id, format, tier_names, tier_cents, amount_cents,
      confirm_on_site, notes, original, ai_read, confirmed, received_at)
    SELECT job_id, gen_random_uuid(), '${format}', ${names}, ${cents}, ${amount}, false, '',
      'Sample wording from business ${String(n)}', false, true, now() + interval '${String(n)} minute'
    FROM jobs.quote_token WHERE token_hash = decode(':hash', 'hex')`;
}

/** Seeds a job in `status` with its fake request and one sample quote; returns its path and its token's hash. */
function openJob(status: string, tiers: boolean): { path: string; hash: string } {
  const job = `WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('${status}', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    h AS (INSERT INTO intake.homeowner (id, contact_id, created_at)
      VALUES (gen_random_uuid(), gen_random_uuid(), now()) RETURNING id),
    c AS (INSERT INTO intake.conversation (id, homeowner_id, job_id, language, first_inbox_id,
      consent_template, consent_sent_at, opened_at)
      SELECT gen_random_uuid(), h.id, j.id, 'en', gen_random_uuid(), 'consent', now(), now() FROM h, j)
    INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode(':hash', 'hex'), now() FROM j`;
  const state: PageState = { route: QUOTE_ROUTE, name: "polish open job", statusRegions: [], shows: "", hides: "", sql: job };
  const path = tokenSeed(state);
  const hash = createHash("sha256").update(path.split("/").pop() ?? "").digest("hex");
  psql(quoteSql(1, tiers).replace(":hash", hash));
  return { path, hash };
}

/** Runs in the page: the number of animations on the whole document. */
function animationCount(): number {
  return document.getAnimations().length;
}

for (const state of STATES) {
  test(`WEB-029 WEB-030 D-4252: ${state.route} "${state.name}" at every review screen`, async ({ page, pageUrl }) => {
    test.setTimeout(120_000);
    const url = pageUrl(state);
    for (const { width, height } of SCREENS) {
      await open(page, url, width, height);
      await expect(page.locator("body")).toContainText(state.shows);
      const at = `${String(width)} by ${String(height)}`;
      expect(await page.evaluate(longAnimations, { end: END_MS, repeats: REPEATS }), at).toEqual([]);
      await settled(page);
      const sideways = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
      expect(sideways, `sideways scroll at ${at}`).toBeLessThanOrEqual(0);
      const file = `test-results/polish/${slug(state)}-${String(width)}`;
      await page.screenshot({ path: `${file}-first.png` });
      await page.screenshot({ path: `${file}-full.png`, fullPage: true });
    }
  });
}

test("WEB-029 D-4252: the render after a failed pick has no animation", async ({ page }) => {
  const { path } = openJob("quotes_ready", true);
  await open(page, `${path}?choose=1`, 390, 844);
  await expect(page.locator("#tier-summary")).toBeVisible();
  expect(await page.evaluate(animationCount)).toBe(0);
});

test("WEB-029 D-4252: the just-closed render has no animation", async ({ page }) => {
  const { path } = openJob("open", false);
  await open(page, path, 390, 844);
  const [answer] = await Promise.all([
    page.waitForResponse((r) => r.request().method() === "POST"),
    page.getByRole("button", { name: /^Close request \d+$/ }).click(),
  ]);
  expect(answer.status(), await answer.text()).toBe(200);
  await expect(page.locator("#closed-note")).toBeVisible();
  expect(await page.evaluate(animationCount)).toBe(0);
});

test("WEB-029: a first-render quote card carries rise", async ({ page }) => {
  const { path } = openJob("open", false);
  await open(page, path, 390, 844);
  await expect(page.locator("#quotes > li")).toHaveCount(1);
  await expect(page.locator("#quotes > li").first()).toHaveClass(/\brise\b/);
});

test("WEB-029 PICK-009: a quote added after load appears with no animation", async ({ page }) => {
  await page.clock.install();
  const { path, hash } = openJob("open", false);
  await open(page, path, 390, 844);
  await expect(page.locator("#quotes > li")).toHaveCount(1);
  psql(quoteSql(2, false).replace(":hash", hash));
  await page.clock.runFor(30_000);
  const added = page.locator("#quotes > li").nth(1);
  await expect(added).toBeVisible();
  await expect(added).not.toHaveClass(/\brise\b/);
  expect(await added.evaluate((el) => el.getAnimations({ subtree: true }).length)).toBe(0);
});
