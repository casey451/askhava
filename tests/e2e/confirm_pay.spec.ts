/// <reference types="node" />
// The business Confirm page's card step in a browser (T224; TD-FEE-25, TD-SEC-04; FEE-014,
// FEE-016, PICK-021, PICK-022; D-1479 to D-1481).
//
// The Confirm route is not served yet: the readers of the fee fixed at the pick, the job type and
// the business session, and the Stripe adapter's unconfirmed create and confirm, come in later
// tasks (D-1480). So the page's script is checked here against a stand-in page: Playwright answers
// the Confirm address with `confirmPage`, a hand copy of the markup
// `app/portals/templates/confirm_pay.html` gives (its ids, form fields and `data-paid`; keep the two
// in step until the route is served, D-1479; the page's words are checked in
// `app/portals/src/confirm_pay.rs`), answers
// `js.stripe.com/v3/` with a fake Stripe that records each call (no network, no Stripe account),
// serves `app/portals/static/confirm_pay.ts` compiled by TypeScript, and answers each press with
// the reply the server would give. The served app is checked for TD-SEC-04: a real homeowner quote
// page, seeded through psql on DATABASE_URL as pick_flow.spec.ts does, loads no Stripe.js. Fake
// data only.
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { Page, Route } from "@playwright/test";
import ts from "typescript";
import { test, expect } from "./fixtures";

const ROOT = join(import.meta.dirname, "..", "..");
const CONFIRM = "/b/picks/00000000-0000-0000-0000-000000000007/confirm";

const confirmPage = (paid: boolean): string => `<!doctype html><html lang="en"><head><title>Confirm job 1042 | Ask Hava</title></head><body><main>
<h1>Confirm job 1042</h1>
<form method="post" action="${CONFIRM}" id="confirm-form" data-stripe-key="pk_test_fake" data-paid="${paid ? "yes" : "no"}" novalidate>
<input type="hidden" name="csrf" value="fake-csrf">
<input type="hidden" name="press" value="press-1">
<input type="hidden" name="payment_method" value="">
<div id="card-element"></div>
<p><input type="checkbox" id="save-card" name="save_card" value="yes"><label for="save-card">Save this card for next time</label></p>
<div id="confirm-status" role="status"></div>
<div id="confirm-error" role="alert"></div>
<button type="submit" id="confirm-button">${paid ? "Confirm and pay $75" : "Confirm"}</button>
</form>
<script src="https://js.stripe.com/v3/"></script>
<script type="module" src="/static/portals/confirm_pay.js"></script>
</main></body></html>`;

const FAKE_STRIPE = `window.stripeCalls = [];
window.Stripe = (key) => {
  window.stripeCalls.push("init " + key);
  return {
    elements: () => ({ create: () => ({ mount: (at) => window.stripeCalls.push("mount " + at) }) }),
    createPaymentMethod: async () => { window.stripeCalls.push("card"); return { paymentMethod: { id: "pm_fake_card" } }; },
    handleNextAction: async ({ clientSecret }) => { window.stripeCalls.push("3ds " + clientSecret); return {}; },
  };
};`;

function compiledScript(): string {
  const source = readFileSync(join(ROOT, "app/portals/static/confirm_pay.ts"), "utf8");
  return ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
}

type Reply = object | { status: number; body: string };

/** Serves the stand-in page, fake Stripe.js and the script; answers presses from `replies`. */
async function standIn(page: Page, replies: Reply[], paid = true): Promise<string[]> {
  const posted: string[] = [];
  await page.route("https://js.stripe.com/v3/", (route) => route.fulfill({ contentType: "text/javascript", body: FAKE_STRIPE }));
  await page.route("**/static/portals/confirm_pay.js", (route) => route.fulfill({ contentType: "text/javascript", body: compiledScript() }));
  await page.route(`**${CONFIRM}`, async (route: Route) => {
    if (route.request().method() === "GET") return route.fulfill({ contentType: "text/html", body: confirmPage(paid) });
    posted.push(route.request().postData() ?? "");
    const next = replies.shift() ?? {};
    if ("status" in next) return route.fulfill(next);
    return route.fulfill({ contentType: "application/json", body: JSON.stringify(next) });
  });
  await page.goto(CONFIRM);
  await expect.poll(() => stripeCalls(page)).toContain("mount #card-element");
  return posted;
}

function stripeCalls(page: Page): Promise<string[]> {
  return page.evaluate(() => (window as unknown as { stripeCalls: string[] }).stripeCalls);
}

/** A job in Quotes ready with one quote, its link token stored; returns the page path. */
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
    SELECT j.id, gen_random_uuid(), 'firm', 19000, false, '', 'Sample wording from a business',
      false, true, now() FROM j`;
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding the quote page failed: ${done.stderr}`);
  return `/q/${token}`;
}

test("TD-FEE-25 FEE-016: a decline shows at once and the next press uses the next attempt's press ID", async ({ page }) => {
  const posted = await standIn(page, [
    { state: "declined", message: "Your card was declined. Nothing was charged. Try again or use another card.", next_press: "press-2" },
    { state: "waiting", message: "Payment sent. You'll get the homeowner's details as soon as Stripe confirms it.", next_press: null },
  ]);
  await expect(page.locator("#save-card")).not.toBeChecked();
  await page.getByRole("button", { name: "Confirm and pay $75" }).click();
  await expect(page.locator("#confirm-error")).toHaveText(/^Your card was declined/);
  expect(posted[0]).toContain("press=press-1");
  expect(posted[0]).toContain("payment_method=pm_fake_card");
  expect(posted[0]).not.toMatch(/4242|cvc|card_number/i);
  await page.getByRole("button", { name: "Confirm and pay $75" }).click();
  await expect(page.locator("#confirm-status")).toHaveText(/as soon as Stripe confirms it/);
  await expect(page.locator("#confirm-error")).toHaveText("");
  expect(posted[1]).toContain("press=press-2");
});

test("TD-FEE-25 FEE-014: a bank approval runs Stripe's 3-D Secure step in the page", async ({ page }) => {
  await standIn(page, [
    { state: "approve", message: "Your bank wants to check this payment. Follow the steps in the window it opens.", next_press: null, client_secret: "pi_fake_secret_fake" },
  ]);
  await page.getByRole("button", { name: "Confirm and pay $75" }).click();
  await expect(page.locator("#confirm-status")).toHaveText(/Your bank wants to check/);
  await expect.poll(() => stripeCalls(page)).toContain("3ds pi_fake_secret_fake");
  await expect(page).toHaveURL(new RegExp(`${CONFIRM}$`));
});

test("TD-FEE-40 FEE-045: a $0 Confirm posts no card unless the save box is ticked", async ({ page }) => {
  const waiting = { state: "waiting", message: "Payment sent. You'll get the homeowner's details as soon as Stripe confirms it.", next_press: null };
  const posted = await standIn(page, [{ status: 500, body: "" }, waiting, waiting], false);
  await page.getByRole("button", { name: "Confirm", exact: true }).click();
  await expect(page.locator("#confirm-error")).toHaveText(/press the button again/);
  expect(posted[0]).toMatch(/payment_method=(&|$)/);
  expect(await stripeCalls(page)).not.toContain("card");
  await page.getByRole("button", { name: "Confirm", exact: true }).click();
  await expect(page.locator("#confirm-status")).toHaveText(/as soon as Stripe confirms it/);
  await expect(page.locator("#confirm-error")).toHaveText("");
  expect(posted[1]).toContain("press=press-1");
  expect(posted[1]).toMatch(/payment_method=(&|$)/);
  await page.locator("#save-card").check();
  await page.getByRole("button", { name: "Confirm", exact: true }).click();
  await expect.poll(() => posted.length).toBe(3);
  expect(posted[2]).toContain("payment_method=pm_fake_card");
  expect(posted[2]).toContain("save_card=yes");
});

test("FEE-041: an over-limit answer shows the portal link", async ({ page }) => {
  await standIn(page, [
    { state: "ended", message: "This would go over your monthly fee limit, so nothing was charged and the pick has ended. You can change the limit in your portal.", next_press: null, link: "/b" },
  ]);
  await page.getByRole("button", { name: "Confirm and pay $75" }).click();
  await expect(page.locator("#confirm-status")).toContainText("monthly fee limit");
  await expect(page.locator("#confirm-status").getByRole("link", { name: "Go to your portal" })).toHaveAttribute("href", "/b");
});

test("TD-SEC-04: a served page other than Confirm (the homeowner's quote page) loads no Stripe.js", async ({ page }) => {
  const loads: string[] = [];
  page.on("request", (request) => loads.push(request.url()));
  const answer = await page.goto(seedQuotePage());
  expect(answer?.status(), "a real page, not an error page").toBe(200);
  await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
  expect(loads.filter((url) => url.includes("stripe.com"))).toEqual([]);
  expect(await page.locator("script[src*='stripe']").count()).toBe(0);
});
