/// <reference types="node" />
// The quote page's pick review step in a browser (T055; TD-PICK-08, TD-PICK-24, TD-PICK-28,
// TD-PICK-30, TD-A11Y-02; PICK-016, PICK-017, A11Y-014, A11Y-031; D-1248 to D-1250).
//
// A job in Quotes ready with three quotes (#2 no longer available) is seeded through psql on
// DATABASE_URL, as tests/a11y/fixtures.ts does. Under the site's `Referrer-Policy: no-referrer` the
// browser sends `Origin: null` on a same-origin form POST, which `security::is_cross_site` (T051's)
// refuses; that is logged for a fix in D-1250. Until it is fixed, `page.route` re-sends each POST to
// the quote page with the https Origin a same-origin request has, so the step itself is tested. The stored page
// pick and its result page are T187's (D-1248): here the final Pick returns to the card and nothing
// is picked. The Spanish Terms line is checked in app/portals/src/tests/pick_flow.rs until the page
// reads the request's language (D-1249). Fake data only.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { Page } from "@playwright/test";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { test, expect } from "./fixtures";

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
      original, ai_read, confirmed, received_at, unavailable_at)
    SELECT j.id, gen_random_uuid(), 'firm', 19000, false, '', 'Sample wording from business ' || q.n,
      false, true, now() + q.n * interval '1 minute', CASE WHEN q.n = 2 THEN now() END
    FROM j, generate_series(1, 3) AS q(n)`;
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding the quote page failed: ${done.stderr}`);
  return `/q/${token}`;
}

/** Re-sends every POST with a same-origin https Origin (see the header and D-1250). */
async function httpsOrigin(page: Page): Promise<void> {
  await page.route("**/q/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const origin = new URL(request.url()).host;
    const headers = { ...request.headers(), origin: `https://${origin}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

async function openReview(page: Page, quote: number): Promise<string> {
  const path = seedQuotePage();
  await httpsOrigin(page);
  await page.goto(path);
  const answer = page.waitForResponse((r) => r.request().method() === "POST");
  await page.getByRole("button", { name: `Pick #${String(quote)}` }).click();
  expect((await answer).status(), "the review step's POST").toBe(200);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Check your pick: quote #${String(quote)}`);
  return path;
}

test("TD-PICK-08 TD-PICK-24 PICK-017: the review step says what happens next, with Go back", async ({ page }) => {
  const path = await openReview(page, 1);
  const main = page.locator("main");
  await expect(main).toContainText("You're picking one business for this request. If it falls through, you can pick another quote.");
  await expect(main).toContainText("You hire the business directly. Ask Hava isn't part of your agreement with it.");
  await expect(main).toContainText("or reply 1 YES");
  await page.getByRole("link", { name: "Go back" }).click();
  await expect(page).toHaveURL(new RegExp(`${path}#quote-1$`));
  await expect(page.getByRole("button", { name: "Pick #1" })).toBeVisible();
});

test("TD-PICK-30 PICK-016: only the Terms line beside the final Pick, body size, link underlined in the link colour", async ({ page }) => {
  await openReview(page, 3);
  const beside = page.locator("#final-pick + p");
  await expect(beside).toHaveText("By picking, you agree to the Terms");
  await expect(page.locator("#final-pick ~ *")).toHaveCount(1);
  const look = await page.evaluate(() => {
    const style = (selector: string): CSSStyleDeclaration => {
      const node = document.querySelector(selector);
      if (node === null) throw new Error(`no ${selector}`);
      return getComputedStyle(node);
    };
    const line = style("#terms-line");
    const link = style("#terms-line a");
    return {
      lineSize: line.fontSize,
      bodySize: style("main li").fontSize,
      lineColour: line.color,
      linkColour: link.color,
      siteLinkColour: style("header a").color,
      underline: link.textDecorationLine,
    };
  });
  expect(look.lineSize).toBe(look.bodySize);
  expect(look.underline).toContain("underline");
  expect(look.linkColour).toBe(look.siteLinkColour);
  expect(look.linkColour).not.toBe(look.lineColour);
  const text = await page.locator("body").innerText();
  for (const banned of ["$100", "liability", "Liability", "key terms", "Key terms"]) expect(text).not.toContain(banned);
});

test("TD-PICK-28: the review step shows no Confirm end time", async ({ page }) => {
  await openReview(page, 1);
  const text = await page.locator("main").innerText();
  for (const banned of ["until", "confirm by", "a.m.", "p.m.", "business hours"]) expect(text).not.toContain(banned);
});

test("TD-A11Y-02 A11Y-014: polite status region on the page, the final Pick is never disabled and changes nothing yet", async ({ page }) => {
  const path = await openReview(page, 1);
  await expect(page.getByRole("button", { name: "Pick #1" })).toBeEnabled();
  await page.getByRole("button", { name: "Pick #1" }).click();
  await expect(page).toHaveURL(new RegExp(`${path}#quote-1$`));
  const status = page.locator("#quote-status");
  await expect(status).toHaveAttribute("role", "status");
  await expect(status).not.toHaveAttribute("aria-live", "assertive");
  await expect(status).toBeEmpty();
  await expect(page.getByRole("button", { name: "Pick #1" })).toBeVisible();
});

test("TD-PICK-24: the review step has no axe violation at 320, 768 and 1280 px", async ({ page }) => {
  await openReview(page, 1);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 900 });
    const { violations } = await axeScan(page);
    expect(violations, `${String(width)} px`).toEqual([]);
  }
});
