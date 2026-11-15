/// <reference types="node" />
// The served Confirm and pay journey in the browser (T495, D-3813; TD-FEE-45, TD-PICK-43 and
// TD-TERMS-11's browser halves; FEE-014, FEE-016, FEE-036, CON-029, PICK-021, PICK-022, PICK-050,
// TERMS-033, SEC-019). On the keyed web binary (T452, T492's Stripe stand-in settings). Each test
// seeds through psql, as tests/a11y/page_states.ts does: a business, its owner's session, a job
// waiting for Confirm and the pick's $25 fee; after the first load (which makes the stand-in
// customer) it stores the card authorization the page shows (FEE-015). Stripe.js is a fake whose card
// gives a stand-in test PaymentMethod and whose 3-D Secure step waits for the test; "charged once" is
// read from the Stripe stand-in's own charges. POSTs are re-sent with an https Origin (D-1250). No
// saved-card picker is served (D-3846); no session gets the sign-in answer (D-3847). The release
// waits on the signed webhook, so the server says "waiting", never "paid", here. $0, try 2 and
// FEE-041 are server-only (app/server/tests/confirm_route.rs). Fake data only; nothing is logged.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import type { APIRequestContext, Page } from "@playwright/test";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { test, expect } from "./fixtures";
import { keyedUrl } from "./keyed";

/** The Stripe stand-in on the stand-in server (tests/e2e/webserver.ts `STAND_IN_PORT`). */
const STRIPE = "http://127.0.0.1:4010/stripe";
/** Any `sk_test_` key: the stand-in checks only the prefix. */
const STAND_IN_AUTH = { authorization: `Bearer sk_test_${"e2e".repeat(4)}` };
const SESSION = "__Host-askhava-session";
/** `payments::api::AUTHORIZATION_VERSION`: a Confirm needs an acceptance of at least it (`required_version`). */
const AUTHORIZATION_VERSION = 2;
/** The page script's lines (app/portals/static/confirm_pay.ts) and the server's waiting line. */
const TRY_AGAIN = "We couldn't get an answer. Check your connection and press the button again.";
const CHECKING = "Checking with your bank. Stay on this page for the answer.";
const WAITING = /^Payment sent\. You'll get the homeowner's details as soon as Stripe confirms it\.$/;
const PAUSED = /^The homeowner has paused this job, so it can't be confirmed now\. Nothing was charged\.$/;

const FAKE_STRIPE = `window.stripeCalls = [];
window.Stripe = (key) => {
  window.stripeCalls.push(key.startsWith("pk_test_") ? "init test key" : "init other key");
  return {
    elements: () => ({ create: () => ({ mount: (at) => window.stripeCalls.push("mount " + at) }) }),
    createPaymentMethod: async () => ({ paymentMethod: { id: window.fakeCard || "pm_card_visa" } }),
    handleNextAction: async ({ clientSecret }) => {
      window.stripeCalls.push("3ds " + clientSecret);
      const how = await new Promise((settle) => { window.settleBank = settle; });
      if (how === "error") throw new Error("the bank step failed");
      return how === "cancel" ? { error: { message: "canceled" } } : {};
    },
  };
};`;

/** Runs `sql` through psql on DATABASE_URL and returns its unaligned output. */
function psql(sql: string): string {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-qtA", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding failed with status ${String(done.status)}`);
  return done.stdout.trim();
}

/** A business session for member `m` (a CTE name) of the token whose SHA256 is `hash`. */
function sessionSql(hash: string, m: string): string {
  return `s AS (INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at)
      SELECT decode('${hash}', 'hex'), 'business', id, now(), now() + interval '1 day' FROM ${m} RETURNING id),
    bs AS (INSERT INTO portals.business_session (session_id, business_id) SELECT s.id, ${m}.business_id FROM s, ${m})`;
}

/** A fresh session token and its SHA256. */
function token(): { value: string; hash: string } {
  const value = randomBytes(32).toString("hex");
  return { value, hash: createHash("sha256").update(value).digest("hex") };
}

interface Seeded { business: string; pick: string; job: string; owner: string }

/** An active business, its owner's session and a pick waiting for Confirm with a $25 fee due. */
function seedPick(): Seeded {
  const [business, pick, job] = [randomUUID(), randomUUID(), randomUUID()];
  const owner = token();
  psql(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id),
    ${sessionSql(owner.hash, "m")},
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES ('${job}', 'awaiting_confirm', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT '${pick}', j.id, m.business_id, 1, 'page', 'awaiting_confirm', now() FROM j, m RETURNING id)
    INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents, terms_version, fixed_at)
      SELECT p.id, '${business}', '${job}', t.mode, 'charged', 2500, t.authorization_version, now()
      FROM p, payments.fee_terms t`);
  return { business, pick, job, owner: owner.value };
}

/** A member of `business` with only the given role (SEC-019); returns the session token. */
function seedMember(business: string, role: "cards_and_quotes" | "confirm_and_billing"): string {
  const member = token();
  const [cards, billing] = [role === "cards_and_quotes", role === "confirm_and_billing"];
  psql(`WITH m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) VALUES ('${business}', gen_random_uuid(), false, ${String(cards)},
      ${String(billing)}, now()) RETURNING id, business_id),
    ${sessionSql(member.hash, "m")}
    SELECT 1`);
  return member.value;
}

/** The homeowner's sole-word CANCEL pause on `job` (CON-029), as `intake` stores it. */
function pause(job: string): void {
  psql(`WITH h AS (INSERT INTO intake.homeowner (contact_id, created_at) VALUES (gen_random_uuid(), now()) RETURNING id),
    c AS (INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id, consent_template,
      consent_sent_at, opened_at) SELECT id, '${job}', 'en', gen_random_uuid(), 'consent_first', now(), now() FROM h RETURNING id)
    INSERT INTO intake.homeowner_pause (conversation_id, job_id, inbox_id, paused_at)
      SELECT id, '${job}', gen_random_uuid(), now() FROM c`);
}

/** The pick's Confirm-page business-terms rows (TERMS-033). */
function termsRows(pick: string): number {
  return Number(psql(`SELECT count(*) FROM businesses.terms_acceptance WHERE pick_id = '${pick}' AND channel = 'confirm_page'`));
}

/** The stand-in's succeeded charges naming `pick` in their metadata (FEE-016). */
async function charges(request: APIRequestContext, pick: string): Promise<number> {
  const reply = await request.get(`${STRIPE}/v1/charges?created[gte]=0&limit=100`, { headers: STAND_IN_AUTH });
  const listed = (await reply.json()) as { data: { metadata?: { pick_id?: string }; status: string }[] };
  return listed.data.filter((c) => c.metadata?.pick_id === pick && c.status === "succeeded").length;
}
const confirmPath = (pick: string): string => `/b/picks/${pick}/confirm`;

/** Signs `page` in with `session` (none clears it), the cookie set as tests/a11y/fixtures.ts sets it. */
async function signIn(page: Page, session: string | null): Promise<void> {
  await page.context().clearCookies();
  if (session === null) return;
  const cookie = { name: SESSION, value: session, domain: "127.0.0.1", path: "/", secure: true, httpOnly: true };
  await page.context().addCookies([{ ...cookie, sameSite: "Lax" }]);
}

/**
 * Answers Stripe.js with the fake, re-sends the page's POSTs with a same-origin https Origin
 * (D-1250) and holds its JSON state read until `held` settles; returns the posted bodies.
 */
async function serve(page: Page, card: string, held: Promise<void> = Promise.resolve()): Promise<string[]> {
  const posted: string[] = [];
  await page.addInitScript((pm) => {
    (window as unknown as { fakeCard: string }).fakeCard = pm;
  }, card);
  await page.route("https://js.stripe.com/v3/", (route) => route.fulfill({ contentType: "text/javascript", body: FAKE_STRIPE }));
  await page.route("**/b/picks/*/confirm", async (route) => {
    const request = route.request();
    if (request.method() === "GET") {
      if ((request.headers().accept ?? "").includes("application/json")) await held;
      return route.continue();
    }
    posted.push(request.postData() ?? "");
    const headers = { ...request.headers(), origin: `https://${new URL(request.url()).host}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
  return posted;
}
const stripeCalls = (page: Page): Promise<string[]> => page.evaluate(() => (window as unknown as { stripeCalls: string[] }).stripeCalls);

/** Opens the owner's Confirm page with the fake card `card`, then stores the authorization it shows. */
async function openAsOwner(page: Page, seeded: Seeded, card: string, held?: Promise<void>): Promise<string[]> {
  const posted = await serve(page, card, held);
  await signIn(page, seeded.owner);
  const reply = await page.goto(keyedUrl() + confirmPath(seeded.pick));
  expect(reply?.status()).toBe(200);
  await expect.poll(() => stripeCalls(page)).toEqual(["init test key", "mount #card-element"]);
  const shown = (await page.locator("#save-card-text").textContent()) ?? "";
  const text = shown.replaceAll("'", "''");
  psql(`INSERT INTO payments.card_authorization (business_id, version, text, accepted_at)
    SELECT '${seeded.business}', GREATEST(authorization_version, ${String(AUTHORIZATION_VERSION)}), '${text}', now() FROM payments.fee_terms`);
  return posted;
}
const PAY = { name: "Confirm and pay $25" };

test("TD-FEE-45 SEC-019: the owner and a Confirm and billing member reach the page; a Cards and quotes member and another business get the one not-found page; no session never sees it", async ({ page }) => {
  const seeded = seedPick();
  const other = seedPick();
  await serve(page, "pm_card_visa");
  const path = keyedUrl() + confirmPath(seeded.pick);
  for (const session of [seeded.owner, seedMember(seeded.business, "confirm_and_billing")]) {
    await signIn(page, session);
    expect((await page.goto(path))?.status()).toBe(200);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Confirm job \d+$/);
  }
  for (const session of [seedMember(seeded.business, "cards_and_quotes"), other.owner]) {
    await signIn(page, session);
    expect((await page.goto(path))?.status()).toBe(404);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("We can't find that page");
  }
  await signIn(page, null);
  expect((await page.goto(path))?.status()).toBe(401);
  await expect(page.locator("#confirm-form")).toHaveCount(0);
});

test("TD-PICK-43 TD-TERMS-11 A11Y-001: the served page carries Can't take it, reloading stores no terms row, and axe is clean at 320, 768 and 1280 px", async ({ page }) => {
  const seeded = seedPick();
  await openAsOwner(page, seeded, "pm_card_visa");
  await expect(page.getByRole("link", { name: "Can't take it" })).toHaveAttribute("href", `/b/picks/${seeded.pick}/cant-take`);
  await expect(page.getByRole("button", PAY)).toBeEnabled();
  await expect(page.locator("#save-card")).not.toBeChecked();
  for (let i = 0; i < 2; i += 1) await page.reload();
  expect(termsRows(seeded.pick), "a load stores no terms acceptance").toBe(0);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 900 });
    const { violations, incomplete } = await axeScan(page);
    expect(violations, `axe at ${String(width)} px`).toEqual([]);
    test.info().annotations.push({ type: `axe incomplete at ${String(width)} px`, description: incomplete.join("; ") || "none" });
  }
});

test("TD-FEE-45 FEE-016 TD-TERMS-11: a new card pressed twice charges once and stores one terms row", async ({ page, request }) => {
  const seeded = seedPick();
  const posted = await openAsOwner(page, seeded, "pm_card_visa");
  await page.getByRole("button", PAY).click();
  await expect(page.locator("#confirm-status")).toHaveText(WAITING);
  await page.getByRole("button", PAY).click();
  await expect.poll(() => posted.length).toBe(2);
  await expect(page.locator("#confirm-status")).toHaveText(WAITING);
  expect(posted[0]).not.toContain("save_card");
  expect(posted[1]).toContain(new URLSearchParams(posted[0]).get("press") ?? "no press ID");
  expect(posted.join("&")).not.toMatch(/4242|cvc|card_number|amount|currency/i);
  expect(await charges(request, seeded.pick), "one charge").toBe(1);
  expect(termsRows(seeded.pick), "one terms acceptance").toBe(1);
});

test("TD-FEE-45 FEE-014: a press with Save this card for next time ticked charges once", async ({ page, request }) => {
  const seeded = seedPick();
  const posted = await openAsOwner(page, seeded, "pm_card_visa");
  await page.getByLabel("Save this card for next time").check();
  await page.getByRole("button", PAY).click();
  await expect(page.locator("#confirm-status")).toHaveText(WAITING);
  expect(posted[0]).toContain("save_card=yes");
  expect(await charges(request, seeded.pick), "one charge").toBe(1);
});

/** Presses with the 3-D Secure test card and waits for Stripe's step; returns the intent's ID. */
async function pressToBank(page: Page): Promise<string> {
  await page.getByRole("button", PAY).click();
  await expect(page.locator("#confirm-status")).toHaveText(/^Your bank wants to check this payment\./);
  await expect.poll(async () => (await stripeCalls(page)).some((c) => c.startsWith("3ds pi_"))).toBe(true);
  const call = (await stripeCalls(page)).find((c) => c.startsWith("3ds ")) ?? "";
  return call.slice("3ds ".length).split("_secret_")[0] ?? "";
}

/** Ends the fake's 3-D Secure step: Stripe.js throws, the person cancels, or the step finishes. */
async function settle(page: Page, how: "error" | "cancel" | "done"): Promise<void> {
  await page.evaluate((h) => {
    (window as unknown as { settleBank: (h: string) => void }).settleBank(h);
  }, how);
}

for (const how of ["error", "cancel"] as const) {
  test(`TD-FEE-45 PICK-022: a 3-D Secure ${how} shows the try-again line and never success`, async ({ page, request }) => {
    const seeded = seedPick();
    await openAsOwner(page, seeded, "pm_card_authenticationRequired");
    await pressToBank(page);
    await settle(page, how);
    await expect(page.locator("#confirm-error")).toHaveText(TRY_AGAIN);
    await expect(page.locator("#confirm-status")).toHaveText("");
    expect(await charges(request, seeded.pick), "nothing charged").toBe(0);
  });
}

test("TD-FEE-45 PICK-022: after a passed 3-D Secure step the page shows the checking line until the server answers", async ({ page, request }) => {
  const seeded = seedPick();
  let release = (): void => undefined;
  const held = new Promise<void>((done) => {
    release = done;
  });
  await openAsOwner(page, seeded, "pm_card_authenticationRequired", held);
  const intent = await pressToBank(page);
  const passed = await request.post(`${STRIPE}/v1/payment_intents/${intent}/stand_in_authenticate`, { headers: STAND_IN_AUTH, form: { result: "pass" } });
  expect(passed.status()).toBe(200);
  const answer = page.waitForResponse((r) => r.request().method() === "GET" && r.url().endsWith(confirmPath(seeded.pick)) && r.request().headers().accept === "application/json");
  await settle(page, "done");
  await expect(page.locator("#confirm-status")).toHaveText(CHECKING);
  release();
  const said = (await (await answer).json()) as { state: string; message: string };
  expect(said.state, "the release waits on the signed webhook").toBe("waiting");
  await expect(page.locator("#confirm-status")).toHaveText(said.message);
  await expect(page.locator("#confirm-error")).toHaveText("");
  expect(await charges(request, seeded.pick), "one charge").toBe(1);
});

test("TD-FEE-45 CON-029: a homeowner-paused pick refuses on load and on press, and charges nothing", async ({ page, request }) => {
  const before = seedPick();
  pause(before.job);
  await signIn(page, before.owner);
  expect((await page.goto(keyedUrl() + confirmPath(before.pick)))?.status()).toBe(200);
  await expect(page.locator("#confirm-paused")).toHaveText(PAUSED);
  await expect(page.locator("#confirm-form")).toHaveCount(0);
  const during = seedPick();
  await openAsOwner(page, during, "pm_card_visa");
  pause(during.job);
  await page.getByRole("button", PAY).click();
  await expect(page.locator("#confirm-status")).toHaveText(PAUSED);
  expect(await charges(request, during.pick), "nothing charged").toBe(0);
  expect(termsRows(during.pick)).toBe(0);
});
