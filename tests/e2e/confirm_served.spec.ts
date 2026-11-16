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
// The seed, the fake Stripe.js and the Confirm steps live in keyed.ts (T515, D-3862), whose seed
// also gives the job its homeowner, as the release needs.
import type { APIRequestContext, Page } from "@playwright/test";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { test, expect } from "./fixtures";
import { confirmPath, keyedUrl, openAsOwner, psql, seedPick, serve, sessionSql, signIn, stripeCalls, token, WAITING } from "./keyed";

/** The Stripe stand-in on the stand-in server (tests/e2e/webserver.ts `STAND_IN_PORT`). */
const STRIPE = "http://127.0.0.1:4010/stripe";
/** Any `sk_test_` key: the stand-in checks only the prefix. */
const STAND_IN_AUTH = { authorization: `Bearer sk_test_${"e2e".repeat(4)}` };
/** The page script's lines (app/portals/static/confirm_pay.ts). */
const TRY_AGAIN = "We couldn't get an answer. Check your connection and press the button again.";
const CHECKING = "Checking with your bank. Stay on this page for the answer.";
const PAUSED = /^The homeowner has paused this job, so it can't be confirmed now\. Nothing was charged\.$/;

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

/** The homeowner's sole-word CANCEL pause on `job` (CON-029), on the conversation the seed opened. */
function pause(job: string): void {
  psql(`INSERT INTO intake.homeowner_pause (conversation_id, job_id, inbox_id, paused_at)
      SELECT id, job_id, gen_random_uuid(), now() FROM intake.conversation WHERE job_id = '${job}'`);
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
  const second = page.waitForResponse((r) => r.request().method() === "POST" && r.url().endsWith(confirmPath(seeded.pick)));
  await page.getByRole("button", PAY).click();
  await second;
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
