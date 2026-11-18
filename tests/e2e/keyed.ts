/// <reference types="node" />
// The keyed web binary's handles for the specs (T452, D-3674; SEC-004, PRIV-002).
//
// `webserver.ts` starts a second web binary beside the suite's, with fake Twilio Verify on the
// stand-in, its own ID secret and both vault keys, all made per run. This file gives a spec its base
// URL and ID secret, and writes values in the vault's own formats (app/privacy/src/vault.rs), so a
// spec can seed through psql what that binary opens: `seal` is AES-256-GCM under `VAULT_KEY`
// version 1 with a random 96-bit nonce, stored as the 2-byte big-endian version, the nonce, then the
// ciphertext and its tag, with associated data `<table>|<column>|<id>|v1`; `blindIndex` is
// HMAC-SHA256 under the key HMAC(`VAULT_INDEX_KEY`, `<purpose>|v1`). Node's `crypto` only.
//
// Since T515 (D-3862, D-3869): `stripeDelivery` signs a payload with the keyed binary's per-run
// webhook secret in Stripe's scheme (stand_ins/stripe.py `sign_payload`), and the Confirm journey's
// seed and steps live here (moved from confirm_served.spec.ts): `seedPick` seeds a business (or uses
// a given one), its owner's session, a job waiting for Confirm with its homeowner (a sealed
// `privacy.contact`, an `intake.homeowner` and an open `intake.conversation` on the job, which the
// release needs), the pick and its fee; `openAsOwner` opens the Confirm page with a fake Stripe.js;
// `confirmToWaiting` presses Confirm and pay with a good card until the server says "waiting". Fake
// data only; no key or value is printed or logged.
import { spawnSync } from "node:child_process";
import { createCipheriv, createHash, createHmac, randomBytes, randomUUID } from "node:crypto";
import { expect, type Page } from "@playwright/test";
import { keyedPort, keyedSecrets } from "./webserver";

/** The vault key version the keyed binary seals and indexes under. */
const VERSION = 1;

/** The keyed binary's base URL, e.g. for `page.goto(keyedUrl() + path)`. */
export function keyedUrl(): string {
  return `http://127.0.0.1:${String(keyedPort())}`;
}

/** The keyed binary's `ASKHAVA_ID_SECRET`, for a spec that makes an ID or link it checks. */
export function keyedIdSecret(): string {
  return keyedSecrets().idSecret;
}

/** The 32 key bytes of a `1:<base64>` vault key. */
function keyBytes(pair: string): Buffer {
  const [version, encoded = ""] = pair.split(":");
  const bytes = Buffer.from(encoded, "base64");
  if (version !== String(VERSION) || bytes.length !== 32) throw new Error("the keyed vault key is not version 1 with 32 bytes");
  return bytes;
}

/** `plain` sealed for `column` of row `id` of `table`, as the vault stores it (PRIV-002). */
export function seal(table: string, column: string, id: string, plain: string): Buffer {
  const nonce = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", keyBytes(keyedSecrets().vaultKey), nonce);
  cipher.setAAD(Buffer.from(`${table}|${column}|${id}|v${String(VERSION)}`, "utf8"));
  const sealed = Buffer.concat([cipher.update(plain, "utf8"), cipher.final(), cipher.getAuthTag()]);
  const version = Buffer.alloc(2);
  version.writeUInt16BE(VERSION);
  return Buffer.concat([version, nonce, sealed]);
}

/** The vault's blind-index digest of `value` for `purpose` (e.g. `email`) at key version 1. */
export function blindIndex(purpose: string, value: string): Buffer {
  const master = keyBytes(keyedSecrets().vaultIndexKey);
  const derived = createHmac("sha256", master).update(`${purpose}|v${String(VERSION)}`, "utf8").digest();
  return createHmac("sha256", derived).update(value, "utf8").digest();
}

/**
 * The headers of a Stripe delivery of `payload` at `timestamp` (Unix seconds) that the keyed binary
 * accepts: `Stripe-Signature: t=<timestamp>,v1=<hex HMAC-SHA256(secret, "<t>.<payload>")>` under its
 * per-run webhook secret (SEC-016, D-3862). Send `payload` exactly as signed.
 */
export function stripeDelivery(payload: string, timestamp: number): Record<string, string> {
  const t = String(timestamp);
  const v1 = createHmac("sha256", keyedSecrets().stripeWebhookSecret).update(`${t}.${payload}`, "utf8").digest("hex");
  return { "content-type": "application/json", "stripe-signature": `t=${t},v1=${v1}` };
}

/** Runs `sql` through psql on DATABASE_URL and returns its unaligned output. */
export function psql(sql: string): string {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-qtA", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`seeding failed with status ${String(done.status)}`);
  return done.stdout.trim();
}

/** A business session for member `m` (a CTE name) of the token whose SHA256 is `hash`. */
export function sessionSql(hash: string, m: string): string {
  return `s AS (INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at)
      SELECT decode('${hash}', 'hex'), 'business', id, now(), now() + interval '1 day' FROM ${m} RETURNING id),
    bs AS (INSERT INTO portals.business_session (session_id, business_id) SELECT s.id, ${m}.business_id FROM s, ${m})`;
}

/** A fresh session token and its SHA256. */
export function token(): { value: string; hash: string } {
  const value = randomBytes(32).toString("hex");
  return { value, hash: createHash("sha256").update(value).digest("hex") };
}

/** A seeded pick waiting for Confirm: its business, job, owner session token and fee in cents. */
export interface Seeded { business: string; pick: string; job: string; owner: string; feeCents: number }

/** The seeded homeowner: fake name, 555-01xx phone and street (PRIV-002). */
const HOMEOWNER = { name: "Robin Fakename", phone: "+19285550163", address: "12 Sample Street" };

/** The job's homeowner: her sealed contact, her `intake.homeowner` and an open conversation on `job`. */
function homeownerSql(job: string): string {
  const contact = randomUUID();
  const field = (column: "name" | "phone" | "address"): string =>
    `decode('${seal("privacy.contact", column, contact, HOMEOWNER[column]).toString("hex")}', 'hex')`;
  return `pc AS (INSERT INTO privacy.contact (id, name, phone, address)
      VALUES ('${contact}', ${field("name")}, ${field("phone")}, ${field("address")}) RETURNING id),
    h AS (INSERT INTO intake.homeowner (contact_id, created_at) SELECT id, now() FROM pc RETURNING id),
    c AS (INSERT INTO intake.conversation (homeowner_id, job_id, language, first_inbox_id, consent_template,
      consent_sent_at, opened_at) SELECT id, '${job}', 'en', gen_random_uuid(), 'consent_first', now(), now() FROM h)`;
}

/** A new active business with its owner, or the owner of `business`, as CTE `m`. */
function ownerSql(business: string, existing: boolean): string {
  if (existing) return `m AS (SELECT id, business_id FROM businesses.team_member WHERE business_id = '${business}' AND is_owner LIMIT 1)`;
  return `b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
      VALUES ('${business}', 'active', now(), now()) RETURNING id),
    m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes,
      confirm_and_billing, added_at) SELECT id, gen_random_uuid(), true, true, true, now() FROM b
      RETURNING id, business_id)`;
}

/**
 * A business (new and active, or the given one), its owner's session, a job waiting for Confirm
 * with its homeowner, and a page pick on it with a `feeCents` fee due ($25 unless given).
 */
export function seedPick({ business, feeCents = 2500 }: { business?: string; feeCents?: number } = {}): Seeded {
  const [id, pick, job] = [business ?? randomUUID(), randomUUID(), randomUUID()];
  const owner = token();
  psql(`WITH ${ownerSql(id, business !== undefined)},
    ${sessionSql(owner.hash, "m")},
    j AS (INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from)
      VALUES ('${job}', 'awaiting_confirm', false, now(), now(), now()) RETURNING id),
    ${homeownerSql(job)},
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    p AS (INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at)
      SELECT '${pick}', j.id, m.business_id, 1, 'page', 'awaiting_confirm', now() FROM j, m RETURNING id)
    INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents, terms_version, fixed_at)
      SELECT p.id, '${id}', '${job}', t.mode, 'charged', ${String(feeCents)}, t.authorization_version, now()
      FROM p, payments.fee_terms t`);
  return { business: id, pick, job, owner: owner.value, feeCents };
}

const SESSION = "__Host-askhava-session";
/** `payments::api::AUTHORIZATION_VERSION`: a Confirm needs an acceptance of at least it (`required_version`). */
const AUTHORIZATION_VERSION = 2;
/** The server's waiting line once a press is paid but not yet released. */
export const WAITING = /^Payment sent\. You'll get the homeowner's details as soon as Stripe confirms it\.$/;

/** Stripe.js's fake: a test PaymentMethod from `window.fakeCard`, a 3-D Secure step the test settles. */
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

/** The pick's Confirm page path. */
export const confirmPath = (pick: string): string => `/b/picks/${pick}/confirm`;

/** Signs `page` in with `session` (none clears it), the cookie set as tests/a11y/fixtures.ts sets it. */
export async function signIn(page: Page, session: string | null): Promise<void> {
  await page.context().clearCookies();
  if (session === null) return;
  const cookie = { name: SESSION, value: session, domain: "127.0.0.1", path: "/", secure: true, httpOnly: true };
  await page.context().addCookies([{ ...cookie, sameSite: "Lax" }]);
}

/**
 * Answers Stripe.js with the fake, records the bodies of the page's POSTs (sent with the
 * browser's own headers, D-4292) and holds its JSON state read until `held` settles; returns them.
 */
export async function serve(page: Page, card: string, held: Promise<void> = Promise.resolve()): Promise<string[]> {
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
    return route.continue();
  });
  return posted;
}

/** What the fake Stripe.js was asked, in order. */
export const stripeCalls = (page: Page): Promise<string[]> => page.evaluate(() => (window as unknown as { stripeCalls: string[] }).stripeCalls);

/** Opens the owner's Confirm page with the fake card `card`, then stores the authorization it shows. */
export async function openAsOwner(page: Page, seeded: Seeded, card: string, held?: Promise<void>): Promise<string[]> {
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

/** Confirms and pays `seeded` with a good test card until the server says "waiting" (FEE-016). */
export async function confirmToWaiting(page: Page, seeded: Seeded): Promise<void> {
  await openAsOwner(page, seeded, "pm_card_visa");
  await page.getByRole("button", { name: `Confirm and pay $${String(seeded.feeCents / 100)}` }).click();
  await expect(page.locator("#confirm-status")).toHaveText(WAITING);
}
