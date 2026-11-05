/// <reference types="node" />
// The business Confirm page's card step in a browser (T224; TD-FEE-25, TD-SEC-04; FEE-014,
// FEE-016, PICK-021, PICK-022; D-1479 to D-1481).
//
// The Confirm route is not served yet: the readers of the fee fixed at the pick, the job type and
// the business session, and the Stripe adapter's unconfirmed create and confirm, come in later
// tasks (D-1480). So the page's script is checked here against a stand-in page: Playwright answers
// the Confirm address with the markup `app/portals/templates/confirm_pay.html` gives (its ids and
// form fields; the page's words are checked in `app/portals/src/confirm_pay.rs`), answers
// `js.stripe.com/v3/` with a fake Stripe that records each call (no network, no Stripe account),
// serves `app/portals/static/confirm_pay.ts` compiled by TypeScript, and answers each press with
// the reply the server would give. The served app is checked for TD-SEC-04: the homeowner's quote
// page loads no Stripe.js. Fake data only.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { Page, Route } from "@playwright/test";
import ts from "typescript";
import { test, expect } from "./fixtures";

const ROOT = join(import.meta.dirname, "..", "..");
const CONFIRM = "/b/picks/00000000-0000-0000-0000-000000000007/confirm";

const PAGE = `<!doctype html><html lang="en"><head><title>Confirm request 1042 | Ask Hava</title></head><body><main>
<h1>Confirm request 1042</h1>
<form method="post" action="${CONFIRM}" id="confirm-form" data-stripe-key="pk_test_fake" novalidate>
<input type="hidden" name="csrf" value="fake-csrf">
<input type="hidden" name="press" value="press-1">
<input type="hidden" name="payment_method" value="">
<div id="card-element"></div>
<p><input type="checkbox" id="save-card" name="save_card" value="yes"><label for="save-card">Save this card for next time</label></p>
<div id="confirm-status" role="status"></div>
<div id="confirm-error" role="alert"></div>
<button type="submit" id="confirm-button">Confirm and pay $75</button>
</form>
<script src="https://js.stripe.com/v3/"></script>
<script type="module" src="/static/portals/confirm_pay.js"></script>
</main></body></html>`;

const FAKE_STRIPE = `window.stripeCalls = [];
window.Stripe = (key) => {
  window.stripeCalls.push("init " + key);
  return {
    elements: () => ({ create: () => ({ mount: (at) => window.stripeCalls.push("mount " + at) }) }),
    createPaymentMethod: async () => ({ paymentMethod: { id: "pm_fake_card" } }),
    handleNextAction: async ({ clientSecret }) => { window.stripeCalls.push("3ds " + clientSecret); return {}; },
  };
};`;

function compiledScript(): string {
  const source = readFileSync(join(ROOT, "app/portals/static/confirm_pay.ts"), "utf8");
  return ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
}

/** Serves the stand-in page, fake Stripe.js and the script; answers presses from `replies`. */
async function standIn(page: Page, replies: object[]): Promise<string[]> {
  const posted: string[] = [];
  await page.route("https://js.stripe.com/v3/", (route) => route.fulfill({ contentType: "text/javascript", body: FAKE_STRIPE }));
  await page.route("**/static/portals/confirm_pay.js", (route) => route.fulfill({ contentType: "text/javascript", body: compiledScript() }));
  await page.route(`**${CONFIRM}`, async (route: Route) => {
    if (route.request().method() === "GET") return route.fulfill({ contentType: "text/html", body: PAGE });
    posted.push(route.request().postData() ?? "");
    return route.fulfill({ contentType: "application/json", body: JSON.stringify(replies.shift() ?? {}) });
  });
  await page.goto(CONFIRM);
  await expect.poll(() => page.evaluate(() => (window as unknown as { stripeCalls: string[] }).stripeCalls)).toContain("mount #card-element");
  return posted;
}

test("TD-FEE-25 FEE-016: a decline shows at once and the next press uses the next attempt's press ID", async ({ page }) => {
  const posted = await standIn(page, [
    { state: "declined", message: "Error: your card was declined. Nothing was charged. Try again or use another card.", next_press: "press-2" },
    { state: "waiting", message: "Payment sent. We'll show the homeowner's details once Stripe confirms it.", next_press: null },
  ]);
  await expect(page.locator("#save-card")).not.toBeChecked();
  await page.getByRole("button", { name: "Confirm and pay $75" }).click();
  await expect(page.locator("#confirm-error")).toHaveText(/^Error: your card was declined/);
  expect(posted[0]).toContain("press=press-1");
  expect(posted[0]).toContain("payment_method=pm_fake_card");
  expect(posted[0]).not.toMatch(/4242|cvc|card_number/i);
  await page.getByRole("button", { name: "Confirm and pay $75" }).click();
  await expect(page.locator("#confirm-status")).toHaveText(/once Stripe confirms it/);
  await expect(page.locator("#confirm-error")).toHaveText("");
  expect(posted[1]).toContain("press=press-2");
});

test("TD-FEE-25 FEE-014: a bank approval runs Stripe's 3-D Secure step in the page", async ({ page }) => {
  await standIn(page, [
    { state: "approve", message: "Your bank wants to approve this payment. Follow its steps in the window.", next_press: null, client_secret: "pi_fake_secret_fake" },
  ]);
  await page.getByRole("button", { name: "Confirm and pay $75" }).click();
  await expect(page.locator("#confirm-status")).toHaveText(/Your bank wants to approve/);
  await expect.poll(() => page.evaluate(() => (window as unknown as { stripeCalls: string[] }).stripeCalls)).toContain("3ds pi_fake_secret_fake");
  await expect(page).toHaveURL(new RegExp(`${CONFIRM}$`));
});

test("TD-SEC-04: a page of the served app other than Confirm loads no Stripe.js", async ({ page }) => {
  const loads: string[] = [];
  page.on("request", (request) => loads.push(request.url()));
  await page.goto("/q/not-a-real-token");
  expect(loads.filter((url) => url.includes("stripe.com"))).toEqual([]);
  expect(await page.locator("script[src*='stripe']").count()).toBe(0);
});
