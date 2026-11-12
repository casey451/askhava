/// <reference types="node" />
// Lab Core Web Vitals of the pages D-3292 names (T418; TD-OPS-43 reads the file this writes).
//
// Each page opens in a fresh Chromium context at 390x844 with the CPU throttled 4x and a slow mobile
// network (150 ms round trip, about 1.6 Mbps down, 750 kbps up) set through the DevTools protocol.
// An init script observes, buffered: Largest Contentful Paint (the last entry before the first
// input), Cumulative Layout Shift (the sum of shifts without recent input) and a lab stand-in for
// INP: the longest Event Timing duration (first-input, and every event of 16 ms or more) for one
// click and one key press on the page's main control (the first button, link or field in <main>),
// with the click's and the form's default action cancelled so the page stays put. The figures go to
// one JSON file (PERF_VITALS_OUT, else test-results/web-vitals.json) labelled "lab, not field",
// beside the goals (LCP 2.5 s, INP 200 ms, CLS 0.1) and the aspirational figures (LCP 1.5 s, INP
// 150 ms). The test asserts only that each figure was measured, never a threshold; perf/compare.py
// warns on a missed goal. A page whose address is not served yet (only the quote page is today;
// the others are their planned addresses, as perf/bench.py lists them) is recorded as "not exposed"
// with its status. The quote page is seeded through psql on DATABASE_URL as
// tests/e2e/pick_flow.spec.ts seeds it; the file holds page names and figures, never the address.
// Fake data only (BUILD_BRIEF.md:66).
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import type { Browser, Page } from "@playwright/test";
import { test, expect } from "../e2e/fixtures";

const ROOT = join(import.meta.dirname, "..", "..");
const OUT = process.env.PERF_VITALS_OUT ?? join(ROOT, "test-results", "web-vitals.json");
const LABEL = "lab, not field";
const VIEWPORT = { width: 390, height: 844 };
const NETWORK = { offline: false, latency: 150, downloadThroughput: (1.6 * 1024 * 1024) / 8, uploadThroughput: (750 * 1024) / 8 };

interface Vitals {
  lcp_ms: number | null;
  inp_ms: number | null;
  cls: number;
}

type Figure = (Vitals & { label: string }) | string;

function seedQuotePage(): string {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  const sql = `WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    t AS (INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('${hash}', 'hex'), now() FROM j)
    INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes,
      original, ai_read, confirmed, received_at)
    SELECT j.id, gen_random_uuid(), 'firm', 19000, false, '', 'Sample wording from business ' || q.n,
      false, true, now() + q.n * interval '1 minute'
    FROM j, generate_series(1, 3) AS q(n)`;
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding the quote page failed: ${done.stderr}`);
  return `/q/${token}`;
}

/** The pages D-3292 names, by name; an address not served yet is the planned one. */
const PAGES: readonly [string, () => string][] = [
  ["home", () => "/"],
  ["zip_result", () => "/?zip=86403"],
  ["intake_first_step", () => "/request"],
  ["quote_page", seedQuotePage],
  ["confirm", () => "/b/picks/00000000-0000-0000-0000-000000000007/confirm"],
  ["business_dashboard", () => "/b"],
];

/** Runs in the page before its own scripts: the observers and the cancelled default actions. */
function observe(): void {
  const found = { lcp: -1, cls: 0, inp: -1, input: false };
  (window as unknown as { vitals: typeof found }).vitals = found;
  const watch = (type: string, take: (entry: PerformanceEntry) => void, extra: object = {}): void => {
    new PerformanceObserver((list) => {
      list.getEntries().forEach(take);
    }).observe({ type, buffered: true, ...extra });
  };
  watch("largest-contentful-paint", (entry) => {
    if (!found.input) found.lcp = entry.startTime;
  });
  watch("layout-shift", (entry) => {
    const shift = entry as PerformanceEntry & { value: number; hadRecentInput: boolean };
    if (!shift.hadRecentInput) found.cls += shift.value;
  });
  const timing = (entry: PerformanceEntry): void => {
    found.input = true;
    found.inp = Math.max(found.inp, entry.duration);
  };
  watch("first-input", timing);
  watch("event", timing, { durationThreshold: 16 });
  for (const kind of ["click", "submit"]) {
    document.addEventListener(kind, (event) => {
      event.preventDefault();
    }, true);
  }
}

/** Waits two frames and a moment, so pending paints and Event Timing entries are delivered. */
async function settle(page: Page): Promise<{ lcp: number; cls: number; inp: number }> {
  return page.evaluate(async () => {
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done, 500))));
    return (window as unknown as { vitals: { lcp: number; cls: number; inp: number } }).vitals;
  });
}

async function measure(browser: Browser, baseURL: string, path: string): Promise<Vitals> {
  const context = await browser.newContext({ viewport: VIEWPORT, baseURL });
  try {
    const page = await context.newPage();
    const cdp = await context.newCDPSession(page);
    await cdp.send("Network.enable");
    await cdp.send("Network.emulateNetworkConditions", NETWORK);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: 4 });
    await page.addInitScript(observe);
    await page.goto(path, { waitUntil: "load" });
    const before = await settle(page);
    const control = page.locator("main").locator("button, a[href], input:not([type=hidden]), select, textarea").first();
    await control.click();
    await control.press("Tab");
    const after = await settle(page);
    return { lcp_ms: before.lcp < 0 ? null : before.lcp, inp_ms: after.inp < 0 ? null : after.inp, cls: after.cls };
  } finally {
    await context.close();
  }
}

test("TD-OPS-43: each served page's lab LCP, INP stand-in and CLS are measured and written", async ({ browser, request, baseURL }) => {
  const pages: Record<string, Figure> = {};
  for (const [name, address] of PAGES) {
    await test.step(name, async () => {
      const path = address();
      const status = (await request.get(path, { maxRedirects: 0 })).status();
      if (status !== 200) {
        pages[name] = `not exposed: GET answers ${String(status)}`;
        return;
      }
      const figures = await measure(browser, baseURL ?? "", path);
      expect(figures.lcp_ms, `${name} LCP`).not.toBeNull();
      expect(figures.inp_ms, `${name} INP stand-in`).not.toBeNull();
      pages[name] = { ...figures, label: LABEL };
    });
  }
  expect(typeof pages.quote_page, "the quote page is served and measured").toBe("object");
  const record = {
    label: LABEL,
    conditions: { browser: "chromium", viewport: VIEWPORT, cpu_slowdown: 4, network: NETWORK },
    goals: { lcp_ms: 2500, inp_ms: 200, cls: 0.1 },
    aspirational: { lcp_ms: 1500, inp_ms: 150 },
    pages,
  };
  mkdirSync(dirname(OUT), { recursive: true });
  writeFileSync(OUT, `${JSON.stringify(record, null, 1)}\n`, "utf8");
});
