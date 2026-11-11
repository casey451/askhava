/// <reference types="node" />
// Journey J1, the homeowner, stage-4 slice (T098; docs/test-design.md section 5, J1, with its
// closing checks; A11Y-021; D-491, D-493, D-1739; the slice and its gaps are D-2029 to D-2032).
//
// The suite's web binary (tests/e2e/webserver.ts) runs in `ci` with no public base URL and no
// vault keys, so it serves no Twilio route and no page pick (D-1311, D-1516). This file starts a
// second web binary from the same build on its own port, with every adapter on its stand-in and
// fake keys made at run time: the public base URL, the Twilio token, the Verify service, one masked
// number, the ID secret and both vault keys. Inbound texts are posted to it signed as Twilio signs
// them (SEC-016). Nothing yet sends the messaging outbox to the carrier (no sender task, D-2030),
// so the texts the app queued are read from `messaging.outbox` through psql on DATABASE_URL, not
// from the stand-in's sent log. The conversation stops after the consent text (the question order
// is D-999's open gap), so steps 2 to 4 are seeded: a job linked to her request, two
// diagnostic-visit quotes and a quote-page token (D-2029). Step 6 runs the real page pick with the
// Verify stand-in's code. The closing checks read the database. Fake data only: the 555-01xx
// numbers are fictional and every key is random per run.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { createHash, createHmac, randomBytes } from "node:crypto";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { webPort } from "./webserver";

const ROOT = join(import.meta.dirname, "..", "..");
const PORT = webPort() + 17;
const BASE = `http://127.0.0.1:${String(PORT)}`;
const PUBLIC_BASE = "https://j1.example.test";
const TWILIO_SID = `AC${"0".repeat(32)}`;
const TWILIO_TOKEN = randomBytes(20).toString("hex");
const HOMEOWNER = "+19285550142";
const CON_031_START = "Ask Hava: Got it. Next, a few quick questions.";

test.describe.configure({ mode: "serial" });

let web: ChildProcess | undefined;
/** Everything the J1 web binary logged, for the closing sentinel check (PRIV-016). */
const logs: string[] = [];

/** Rows of `sql` from psql on DATABASE_URL, each a list of column values. */
function rows(sql: string): string[][] {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-AtqF", "\t", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed: ${done.stderr}`);
  return done.stdout.split("\n").filter((line) => line !== "").map((line) => line.split("\t"));
}

function fullEnv(): NodeJS.ProcessEnv {
  const database = process.env.DATABASE_URL ?? "";
  const key = (): string => `1:${randomBytes(32).toString("base64")}`;
  const inherited = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith("ASKHAVA_")));
  const scratch = mkdtempSync(join(tmpdir(), "askhava-j1-"));
  return {
    ...inherited,
    PORT: String(PORT),
    ASKHAVA_APP_ENV: "ci",
    ASKHAVA_DATABASE_URL: database,
    ASKHAVA_MIGRATION_DATABASE_URL: database,
    ASKHAVA_PUBLIC_BASE_URL: PUBLIC_BASE,
    ASKHAVA_ADAPTER_MODE_TWILIO: "stand_in",
    ASKHAVA_TWILIO_BASE_URL: "http://127.0.0.1:4010/twilio",
    ASKHAVA_TWILIO_VERIFY_BASE_URL: "http://127.0.0.1:4010/twilio",
    ASKHAVA_TWILIO_ACCOUNT_SID: TWILIO_SID,
    ASKHAVA_TWILIO_AUTH_TOKEN: TWILIO_TOKEN,
    ASKHAVA_TWILIO_VERIFY_SERVICE_SID: `VA${"e".repeat(32)}`,
    ASKHAVA_MASKED_POOL_SIZE: "1",
    ASKHAVA_MASKED_NUMBERS: "+15005550100",
    ASKHAVA_ID_SECRET: randomBytes(24).toString("hex"),
    ASKHAVA_STORAGE_BASE_URL: `file://${join(scratch, "storage")}`,
    ASKHAVA_TEST_CONTROL_SECRET: randomBytes(24).toString("hex"),
    ASKHAVA_TEST_CONTROL_CLOCK_FILE: join(scratch, "clock"),
    VAULT_KEY: key(),
    VAULT_INDEX_KEY: key(),
  };
}

async function waitForWeb(): Promise<void> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const up = await fetch(`${BASE}/terms`).then(() => true, () => false);
    if (up) return;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("the J1 web binary did not answer within 30 seconds");
}

test.beforeAll(async () => {
  web = spawn(join(ROOT, "target", "debug", "server"), [], { cwd: ROOT, env: fullEnv(), stdio: ["ignore", "pipe", "pipe"] });
  web.stdout?.on("data", (chunk: Buffer) => logs.push(chunk.toString()));
  web.stderr?.on("data", (chunk: Buffer) => logs.push(chunk.toString()));
  await waitForWeb();
});

test.afterAll(() => {
  web?.kill("SIGTERM");
});

/** Posts one inbound text signed as Twilio signs it (SEC-016); returns the HTTP status. */
async function textIn(from: string, body: string, sid = `SM${randomBytes(16).toString("hex")}`): Promise<number> {
  const form: Record<string, string> = { AccountSid: TWILIO_SID, Body: body, From: from, MessageSid: sid, NumMedia: "0", To: "+15005550006" };
  const path = "/webhooks/twilio/sms";
  const signed = PUBLIC_BASE + path + Object.keys(form).sort().map((name) => name + (form[name] ?? "")).join("");
  const signature = createHmac("sha1", TWILIO_TOKEN).update(signed).digest("base64");
  const reply = await fetch(BASE + path, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", "x-twilio-signature": signature },
    body: new URLSearchParams(form).toString(),
  });
  return reply.status;
}

/** The homeowner's request (conversation) opened since `since`, by its first inbox row's time. */
function requestSince(since: string): { conversation: string; contact: string } {
  const found = rows(`SELECT c.id, h.contact_id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id
    WHERE c.opened_at >= '${since}' ORDER BY c.opened_at DESC LIMIT 1`);
  const [conversation = "", contact = ""] = found[0] ?? [];
  if (conversation === "") throw new Error("no request was opened");
  return { conversation, contact };
}

/** The template IDs queued for `contact`, oldest first. */
function textsTo(contact: string): string[] {
  return rows(`SELECT template_id FROM messaging.outbox WHERE recipient_id = '${contact}' ORDER BY created_at, template_id`).map((r) => r[0] ?? "");
}

function now(): string {
  return rows("SELECT clock_timestamp()")[0]?.[0] ?? "";
}

/** Steps 2 to 4 as they would leave the request (D-2029): an hvac job of hers in Quotes ready, two diagnostic-visit quotes, a link token. */
function seedQuotes(conversation: string): { path: string; job: string } {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(token).digest("hex");
  const job = rows(`WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    a AS (INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at)
      SELECT '${conversation}', 'job.hvac.home', replace(j.id::text, '-', ''), now() FROM j),
    t AS (INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode('${hash}', 'hex'), now() FROM j),
    q AS (INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes,
      original, ai_read, confirmed, received_at)
    SELECT j.id, gen_random_uuid(), 'diagnostic_visit', 8900, false, '',
      'Diagnostic visit, sample wording ' || g.n, false, true, now() + g.n * interval '1 minute'
    FROM j, generate_series(1, 2) AS g(n))
    SELECT id FROM j`)[0]?.[0] ?? "";
  return { path: `/q/${token}`, job };
}

/** Re-sends every POST with a same-origin https Origin (D-1250). */
async function httpsOrigin(page: Page): Promise<void> {
  await page.route("**/q/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const headers = { ...request.headers(), origin: `https://${new URL(request.url()).host}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

/** The code the Verify stand-in sent to `phone`, from its test hook standing in for her phone. */
async function verifyCode(phone: string): Promise<string> {
  const reply = await fetch(`http://127.0.0.1:4010/_stand_in/twilio/code?to=${encodeURIComponent(phone)}`);
  return ((await reply.json()) as { code: string }).code;
}

const state = { since: "", conversation: "", contact: "", path: "", job: "" };

test("J1 step 1 CON-005 CON-031 INT-002 D-419: the first text is the consent; the consent text is queued once, no 911 text, no reply asked", async () => {
  state.since = now();
  const sid = `SM${randomBytes(16).toString("hex")}`;
  expect(await textIn(HOMEOWNER, "AC blowing warm air", sid)).toBe(200);
  expect(await textIn(HOMEOWNER, "AC blowing warm air", sid), "the same delivery again").toBe(200);
  Object.assign(state, requestSince(state.since));
  expect(textsTo(state.contact)).toEqual(["consent"]);
  const body = rows(`SELECT body FROM messaging.outbox WHERE recipient_id = '${state.contact}'`)[0]?.[0] ?? "";
  expect(body.startsWith(CON_031_START)).toBe(true);
  expect(body).not.toMatch(/Reply (OK|YES)/i);
  expect(rows(`SELECT channel FROM intake.consent_record WHERE conversation_id = '${state.conversation}'`)).toEqual([["sms"]]);
  expect(rows(`SELECT count(*) FROM intake.emergency_event WHERE conversation_id = '${state.conversation}'`)).toEqual([["0"]]);
});

test("J1 step 5 WORD-005 A11Y-014: both diagnostic-visit quotes in arrival order, the WORD-005 line, a status region present at load", async ({ page }) => {
  Object.assign(state, seedQuotes(state.conversation));
  await page.goto(BASE + state.path);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Your quotes for request \d+$/);
  await expect(page.locator("main")).toContainText("Every eligible quote shown, in the order received.");
  await expect(page.locator("#quote-status")).toHaveAttribute("role", "status");
  await expect(page.locator("#quotes > li h2")).toHaveText(["Quote #1", "Quote #2"]);
  await expect(page.locator("#quote-1")).toContainText("Diagnostic visit, sample wording 1");
  await expect(page.locator("#quote-2")).toContainText("Diagnostic visit, sample wording 2");
});

test("J1 step 6 PICK-016 PICK-013 PICK-017 PICK-018: review, Go back, pick again, a wrong code then the right one, masked call, focus on the result", async ({ page }) => {
  await httpsOrigin(page);
  await page.goto(BASE + state.path);
  await page.getByRole("button", { name: "Pick #2" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Check your pick: quote #2");
  await expect(page.locator("#final-pick + #terms-line")).toHaveText(/^By picking, you agree to the Terms/);
  await page.getByRole("link", { name: "Go back" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Your quotes for request \d+$/);
  await page.getByRole("button", { name: "Pick #2" }).click();
  await page.locator("#share").check();
  await page.locator("#final-pick").click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Finish your pick for request \d+$/);
  await page.getByLabel(/^A phone call/).check();
  const right = await verifyCode(HOMEOWNER);
  await page.getByLabel("6-digit code").fill(right === "000000" ? "111111" : "000000");
  await page.getByRole("button", { name: "Finish my pick" }).click();
  await expect(page.locator("#code-error")).toBeVisible();
  await page.getByLabel(/^A phone call/).check();
  await page.getByLabel("6-digit code").fill(right);
  await page.getByRole("button", { name: "Finish my pick" }).click();
  await expect(page.locator("#pick-result")).toBeFocused();
  await expect(page.locator("#pick-result")).toHaveText("Done. The business has your pick.");
  expect(rows(`SELECT channel FROM intake.terms_acceptance WHERE conversation_id = '${state.conversation}'`)).toEqual([["quote_page"]]);
});

test("J1 variant CON-009 CON-012 CON-028: STOP after quotes arrive; the one confirmation, then no further text; the job and its quote page go on", async ({ page }) => {
  const from = "+19285550146";
  const since = now();
  expect(await textIn(from, "Water heater leaking")).toBe(200);
  const request = requestSince(since);
  const seeded = seedQuotes(request.conversation);
  expect(await textIn(from, "STOP")).toBe(200);
  expect(await textIn(from, "Is anyone coming today?")).toBe(200);
  expect(textsTo(request.contact)).toEqual(["consent", "opt_out_confirmation"]);
  expect(rows(`SELECT status FROM jobs.job WHERE id = '${seeded.job}'`)).toEqual([["quotes_ready"]]);
  await page.goto(BASE + seeded.path);
  await expect(page.locator("#quotes > li h2")).toHaveText(["Quote #1", "Quote #2"]);
});

test("J1 variant INT-004 INT-005 D-436: 911-list words first get the 911 text and the consent text, once each; SAFE sends nothing more", async () => {
  const since = now();
  expect(await textIn("+19285550143", "I smell gas in the kitchen")).toBe(200);
  const gas = requestSince(since);
  // Both texts are queued in one transaction with one `created_at` and a random `id`, so the
  // outbox cannot show which came first; the set is checked here, the order is D-2032's gap.
  expect([...textsTo(gas.contact)].sort()).toEqual(["consent", "emergency_911"]);
  expect(rows(`SELECT count(*) FROM intake.emergency_event WHERE conversation_id = '${gas.conversation}' AND safe_at IS NULL`)).toEqual([["1"]]);
  expect(await textIn("+19285550143", "we're safe")).toBe(200);
  expect(textsTo(gas.contact)).toHaveLength(2);
  expect(rows(`SELECT count(*) FROM intake.emergency_event WHERE conversation_id = '${gas.conversation}' AND safe_at IS NOT NULL`)).toEqual([["1"]]);
});

for (const [from, words] of [["+19285550144", "pipe burst under the sink"], ["+19285550145", "AC died, 92 inside"]] as const) {
  test(`J1 variant INT-002 D-436: "${words}" gets no 911 text, only the consent text`, async () => {
    const since = now();
    expect(await textIn(from, words)).toBe(200);
    const request = requestSince(since);
    expect(textsTo(request.contact)).toEqual(["consent"]);
    expect(rows(`SELECT count(*) FROM intake.emergency_event WHERE conversation_id = '${request.conversation}'`)).toEqual([["0"]]);
  });
}

test("J1 closing checks TD-PRIV-02 OPS-017 JOB-005: no duplicate text or charge, the ledger untouched, an audit row per status change, no sentinel outside the vault", async ({ control }) => {
  const twice = rows(`SELECT template_id, recipient_id FROM messaging.outbox WHERE created_at >= '${state.since}'
    GROUP BY template_id, recipient_id, subject_ids HAVING count(*) > 1`);
  expect(twice, "no text queued twice").toEqual([]);
  const sent = (await control.sentTexts()).map((t) => `${t.to} ${t.body}`);
  expect(new Set(sent).size, "no text sent twice").toBe(sent.length);
  expect(rows(`SELECT count(*) FROM payments.pick_intent WHERE job_id = '${state.job}'`), "no charge without Confirm").toEqual([["0"]]);
  expect(rows(`SELECT count(*) FROM payments.ledger_entry WHERE job_id = '${state.job}'`), "nothing in the ledger").toEqual([["0"]]);
  const moves = rows(`SELECT from_status, to_status FROM jobs.status_history WHERE job_id = '${state.job}' ORDER BY id`);
  const audited = rows(`SELECT from_status, to_status FROM ops.audit_log WHERE subject_kind = 'jobs.job'
    AND subject_id = '${state.job}' AND action = 'status_changed' ORDER BY id`);
  expect(moves).toEqual([["quotes_ready", "awaiting_confirm"]]);
  expect(audited, "one audit row per status change").toEqual(moves);
  expect(rows(`SELECT count(*) FROM ops.audit_log WHERE action = 'picks.picked' AND subject_id IN
    (SELECT id FROM picks.pick WHERE job_id = '${state.job}')`)).toEqual([["1"]]);
  const dump = spawnSync("pg_dump", [process.env.DATABASE_URL ?? "", "--data-only"], { encoding: "utf8", maxBuffer: 1 << 28 });
  expect(dump.status).toBe(0);
  expect(logs.join("")).toContain("/webhooks/twilio/sms");
  for (const sentinel of ["9285550142", "928-555-0142", "(928) 555-0142", "928 555 0142"]) {
    expect(dump.stdout.includes(sentinel), `the database outside the sealed vault holds ${sentinel}`).toBe(false);
    expect(logs.join("").includes(sentinel), `the web log holds ${sentinel}`).toBe(false);
  }
});
