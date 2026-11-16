// The base layout with JavaScript off, served through the whole app (T101; TD-WEB-01, TD-A11Y-01,
// TD-A11Y-02; WEB-001, A11Y-003, A11Y-004, A11Y-017, A11Y-026; D-3144, D-3877, D-3878).
//
// No site page exists until T102, so this runs on a page that extends base.html: a quote page,
// seeded through psql on DATABASE_URL as static_served.spec.ts does. With JavaScript off the page
// must render whole: lang, a skip link into main as the first Tab stop, one h1, the help link in
// the header, the evergreen look from /static/site/site.css (served by build_router's site mount),
// and no sticky bar. The site stylesheet's mount answers 404 for an unknown name. The WEB-007
// banner's hidden and shown cases are rendered in app/site/src/routes.rs. Fake data only.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import { test, expect } from "./fixtures";

function psql(sql: string): void {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed: ${done.stderr}`);
}

/** A job in Quotes ready with one fake quote; returns the quote page's path. */
function seedQuotePage(): string {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  psql(`WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j)
    INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('${hash}', 'hex'), now() FROM j`);
  psql(`INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes,
      original, ai_read, confirmed, received_at)
    SELECT job_id, gen_random_uuid(), 'firm', 19000, false, '', 'Sample wording T101', false, true, now()
    FROM jobs.quote_token WHERE token_hash = decode('${hash}', 'hex')`);
  return `/q/${token}`;
}

test.use({ javaScriptEnabled: false });

test("TD-WEB-01 TD-A11Y-01 WEB-001 A11Y-003 A11Y-004 A11Y-017 A11Y-026: the base layout works without JavaScript", async ({ page }) => {
  const sheet = page.waitForResponse((r) => new URL(r.url()).pathname === "/static/site/site.css");
  const answer = await page.goto(seedQuotePage());
  expect(answer?.status()).toBe(200);
  const served = await sheet;
  expect(served.status()).toBe(200);
  expect(served.headers()["content-type"]).toContain("text/css");
  expect(await page.locator("html").getAttribute("lang")).toBe("en");
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.locator("main")).toHaveCount(1);
  await expect(page.locator("header nav a[href='/help']")).toHaveText("Help");
  await page.keyboard.press("Tab");
  expect(await page.evaluate(() => document.activeElement?.getAttribute("href"))).toBe("#main");
  const look = await page.evaluate(() => {
    const css = (selector: string): CSSStyleDeclaration => {
      const node = document.querySelector(selector);
      if (node === null) throw new Error(`no ${selector}`);
      return getComputedStyle(node);
    };
    const bars = [...document.querySelectorAll("body *")].filter((el) => ["fixed", "sticky"].includes(getComputedStyle(el).position));
    return { page: css("html").backgroundColor, ink: css("body").color, link: css("header a").color, help: css("header nav a").color, bars: bars.length };
  });
  expect(look.page).toBe("rgb(247, 245, 240)");
  expect(look.ink).toBe("rgb(30, 41, 38)");
  expect(look.link).toBe("rgb(36, 88, 73)");
  expect(look.help).toBe(look.link);
  expect(look.bars).toBe(0);
});

test("WEB-001: the site's file mount answers only for its listed files", async ({ request }) => {
  expect((await request.get("/static/site/site.css")).status()).toBe(200);
  for (const path of ["/static/site/nope.css", "/static/site/"]) {
    expect((await request.get(path)).status(), path).toBe(404);
  }
});
