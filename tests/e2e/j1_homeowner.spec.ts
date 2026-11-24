/// <reference types="node" />
// Journey J1, the homeowner, stage-5 slice (T098, T676; docs/test-design.md section 5, J1, with its
// closing checks; A11Y-021; TD-INT-50; D-491, D-493, D-1739, D-4360, D-5550, D-5657).
//
// The conversation runs through the served Twilio route of the keyed web binary (`keyedUrl()` in
// tests/e2e/keyed.ts, with `keyedSecrets()` for the token and the public base URL), not a binary of
// this file's own: the suite's worker, which writes the cards and the first-quote text, shares its ID
// secret and vault keys (D-5657). Inbound texts are posted signed as Twilio signs them (SEC-016).
// This harness sets no homeowner number (`ASKHAVA_HOMEOWNER_NUMBER`), so no homeowner text is ever
// sent: each reply's texts are the rows the app queued in `messaging.outbox`, body and all, read
// per reply and compared as a set, never ordered by `created_at` or `template_id` (one reply queues
// its texts in one transaction). Steps 2 to 4 are the real conversation of INT-043's "Intake order"
// table: her ZIP, the coverage line counting two fake active HVAC businesses seeded in her town as
// fixture rows, "How soon?", the trade questions, "No photo", her fake name and address, the card
// and SEND; the job's cards are then recorded. The two quotes come in through each business's
// served Quote link (a `portals.login_link` row for the real job and business, as
// tests/e2e/action_links_served.spec.ts seeds one) and the quote form, so the job reaches Quotes
// ready by the real path. Step 6 runs the real page pick with the Verify stand-in's code. The
// closing checks read the database; the keyed binary's log is not readable from here, so the
// sentinel is checked in the database only (D-5657). Fake data only: the 555-01xx numbers are
// fictional and every key is random per run.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test, expect, type Control } from "./fixtures";
import type { Page } from "@playwright/test";
import { keyedUrl } from "./keyed";
import { TWILIO_SMS_PATH, twilioSignature } from "./twilio_signature";

const ROOT = join(import.meta.dirname, "..", "..");
const BASE = keyedUrl();
const TWILIO_SID = `AC${"0".repeat(32)}`;
const HOMEOWNER = "+19285550142";
/** The brand file the app embeds (app/core/data/brand.json, D-1932): expected brand words come from it. */
const BRAND = JSON.parse(readFileSync(join(ROOT, "app", "core", "data", "brand.json"), "utf8")) as {
  short_name: string;
};
const CON_031_START = `${BRAND.short_name}: Got it. Next, a few quick questions.`;
/** Her replies of a regular HVAC repair request from her ZIP to SEND (INT-043 steps 4 to 12). */
const REPLIES = ["86403", "2", "1", "1", "1", "2", "Nothing else", "No photo", "Pat Sample, 12 Sample Street", "SEND"];

test.describe.configure({ mode: "serial", timeout: 120_000 });

/** Rows of `sql` from psql on DATABASE_URL, each a list of column values. */
function rows(sql: string): string[][] {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-AtqF", "\t", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed: ${done.stderr}`);
  return done.stdout.split("\n").filter((line) => line !== "").map((line) => line.split("\t"));
}

/** Posts one inbound text signed as Twilio signs it (SEC-016); returns the HTTP status. */
async function textIn(from: string, body: string, sid = `SM${randomBytes(16).toString("hex")}`): Promise<number> {
  const form: Record<string, string> = { AccountSid: TWILIO_SID, Body: body, From: from, MessageSid: sid, NumMedia: "0", To: "+15005550006" };
  const reply = await fetch(BASE + TWILIO_SMS_PATH, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", "x-twilio-signature": twilioSignature(TWILIO_SMS_PATH, form) },
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

/** One text the app queued for her; this harness sets no homeowner number, so none is sent and the outbox keeps its body. */
interface Queued { id: string; template: string; body: string }

/** A request's texts read per reply: `seen` holds the outbox rows an earlier reply already showed. */
interface Chat { from: string; contact: string; seen: Set<string> }

function queuedFor(contact: string): Queued[] {
  const json = rows(`SELECT coalesce(json_agg(json_build_object('id', id, 'template', template_id, 'body', body) ORDER BY created_at), '[]')
    FROM messaging.outbox WHERE recipient_id = '${contact}'`)[0]?.[0] ?? "[]";
  return JSON.parse(json) as Queued[];
}

/** The texts one inbound text of hers made the app queue; one reply queues them in one transaction, so their order is not read. */
async function say(chat: Chat, body: string): Promise<Queued[]> {
  expect(await textIn(chat.from, body)).toBe(200);
  const fresh = queuedFor(chat.contact).filter((t) => !chat.seen.has(t.id));
  fresh.forEach((t) => chat.seen.add(t.id));
  return fresh;
}

const templates = (got: Queued[]): string[] => got.map((t) => t.template).sort();

/** Starts a request of `from` with `first`; its first texts count as seen. */
async function open(from: string, first: string): Promise<{ chat: Chat; conversation: string }> {
  const since = now();
  expect(await textIn(from, first)).toBe(200);
  const { conversation, contact } = requestSince(since);
  const chat: Chat = { from, contact, seen: new Set() };
  queuedFor(contact).forEach((t) => chat.seen.add(t.id));
  return { chat, conversation };
}

/** The app clock's date in Arizona (UTC-7 all year): the date of a ROC list made when none exists (the licence classes come from the latest list, D-2743). */
async function arizonaDay(control: Control): Promise<string> {
  return new Date(new Date(await control.now()).getTime() - 7 * 3_600_000).toISOString().slice(0, 10);
}

/** Retires the earlier fixture businesses, then makes two active HVAC businesses in her town, licensed R-39 on the latest ROC list (INT-016, INT-017). */
function seedHvacBusinesses(day: string): void {
  rows(`UPDATE businesses.business SET status = 'removed', status_changed_at = now()
    WHERE status <> 'removed' AND id IN (SELECT business_id FROM businesses.business_name WHERE business_name LIKE 'Sample E2E Cooling %')`);
  rows(`INSERT INTO licensing.list_import (source, list_date, rows_recorded, rows_skipped)
    SELECT 'roc', '${day}'::date, 0, 0 WHERE NOT EXISTS (SELECT 1 FROM licensing.list_import WHERE source = 'roc')`);
  for (const n of [1, 2]) {
    const license = `ROC${String(100_000 + Math.floor(Math.random() * 899_999))}`;
    rows(`WITH b AS (INSERT INTO businesses.business (id, status, created_at, status_changed_at)
        VALUES ('${randomUUID()}', 'active', now(), now()) RETURNING id),
      m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes, confirm_and_billing, added_at)
        SELECT id, gen_random_uuid(), true, true, true, now() FROM b),
      w AS (INSERT INTO businesses.welcome (business_id, requested_at, sent_at) SELECT id, now(), now() FROM b),
      nm AS (INSERT INTO businesses.business_name (business_id, business_name, changed_at)
        SELECT id, 'Sample E2E Cooling ${String(n)} ${randomBytes(3).toString("hex")} LLC', now() FROM b),
      t AS (INSERT INTO businesses.town (business_id, town) SELECT id, 'lake_havasu_city' FROM b),
      c AS (INSERT INTO businesses.job_type_choice (business_id, category, job_type) SELECT id, 'hvac_repair', 'AC not cooling' FROM b),
      li AS (SELECT id, list_date FROM licensing.list_import WHERE source = 'roc' ORDER BY list_date DESC LIMIT 1),
      ar AS (INSERT INTO licensing.agency_record (import_id, source, list_date, license_number, class_code, status)
        SELECT li.id, 'roc', li.list_date, '${license}', 'R-39', 'active' FROM li)
      INSERT INTO licensing.license_check (business_id, source, license_number, list_date, status, checked_at)
        SELECT b.id, 'roc', '${license}', li.list_date, 'active', now() FROM b, li`);
  }
}

/** Waits until the job's two cards are recorded (the worker's task writes them, INT-041); returns their businesses. */
async function cardsRecorded(job: string): Promise<string[]> {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    const businesses = rows(`SELECT business_id FROM quotes.job_card WHERE job_id = '${job}' ORDER BY business_id`).map((r) => r[0] ?? "");
    if (businesses.length === 2) return businesses;
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error("the job's cards were not recorded");
}

/** One business's quote through its served Quote link (a login link row for the real job and business) and the quote form. */
async function quoteFrom(page: Page, job: string, business: string, n: number): Promise<void> {
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(`action:quote:${job}:${token}`).digest("hex");
  rows(`INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
    SELECT decode('${hash}', 'hex'), id, business_id, now() - interval '1 day', now() + interval '2 hours'
    FROM businesses.team_member WHERE business_id = '${business}' AND is_owner`);
  await page.goto(`${BASE}/b/a/quote/${job}/${token}`);
  await page.getByLabel("Visit fee ($)").fill("89");
  await page.getByLabel(/^Note for the homeowner/).fill(`Diagnostic visit, sample wording ${String(n)}`);
  for (const [name, value] of [["trip_fee", "none"], ["parts", "on_site"], ["permit", "not_needed"], ["may_change", "no"], ["visit_part", "morning"], ["spanish", "no"]] as const) {
    await page.locator(`input[name="${name}"][value="${value}"]`).check();
  }
  await page.getByLabel("Date", { exact: true }).fill(new Date(Date.now() + 86_400_000).toISOString().slice(0, 10));
  await page.getByRole("button", { name: /^Send quote for job \d+$/ }).click();
  await expect(page.getByRole("heading", { level: 1 })).not.toHaveText("Your quote wasn't sent");
}

/**
 * Her live quote page path: the link of the newest text that carries one, since each text that
 * names her page mints a token and revokes the last (first quote, then an update for the second).
 * `""` when none is queued yet.
 */
function quotePath(chat: Chat): string {
  const linked = queuedFor(chat.contact).map((t) => /\/q\/[0-9a-f]+/.exec(t.body)?.[0] ?? "").filter((path) => path !== "");
  return linked.at(-1) ?? "";
}

/**
 * The first-quote text is no reply, so quiet hours (8 p.m. to 8 a.m. in Arizona, QH-001) hold it:
 * outside them the worker queues it within seconds; inside them the shared app clock moves once to
 * 8:30 a.m., the one forward move this file makes (a later spec that seeds a short-lived link
 * after it at night is the hazard; D-5717). The path is read once no newer text names her page.
 */
async function quoteTextQueued(control: Control, chat: Chat): Promise<string> {
  for (let tries = 0; tries < 2; tries += 1) {
    const deadline = Date.now() + 8_000;
    while (Date.now() < deadline && quotePath(chat) === "") await new Promise((r) => setTimeout(r, 300));
    if (quotePath(chat) !== "") {
      let path = "";
      while (path !== quotePath(chat)) {
        path = quotePath(chat);
        await new Promise((r) => setTimeout(r, 2_000));
      }
      return path;
    }
    const at = new Date(await control.now());
    const arizona = ((at.getUTCHours() + 17) % 24) * 60 + at.getUTCMinutes();
    if (arizona >= 8 * 60 && arizona < 20 * 60) break;
    await control.advanceClock(((8 * 60 + 30 - arizona + 1440) % 1440) * 60 + 5);
  }
  throw new Error("no text carried her quote page link");
}

/** Both carded businesses quote in turn; the job reaches Quotes ready by the real path (JOB-005); returns her quote page path, read from the queued first-quote text. */
async function quotesIn(page: Page, control: Control, [chat, job]: [Chat, string]): Promise<string> {
  const businesses = await cardsRecorded(job);
  for (const [i, business] of businesses.entries()) await quoteFrom(page, job, business, i + 1);
  expect(rows(`SELECT status FROM jobs.job WHERE id = '${job}'`)).toEqual([["quotes_ready"]]);
  return quoteTextQueued(control, chat);
}

/** The code the Verify stand-in sent to `phone`, from its test hook standing in for her phone. */
async function verifyCode(phone: string): Promise<string> {
  const reply = await fetch(`http://127.0.0.1:4010/_stand_in/twilio/code?to=${encodeURIComponent(phone)}`);
  return ((await reply.json()) as { code: string }).code;
}

const state = { since: "", conversation: "", contact: "", path: "", job: "", chat: { from: HOMEOWNER, contact: "", seen: new Set<string>() } satisfies Chat };

test("J1 step 1 CON-005 CON-031 INT-002 D-419: the first text is the consent; the consent text is queued once, no 911 text, no reply asked", async ({ control }) => {
  seedHvacBusinesses(await arizonaDay(control));
  state.since = now();
  const sid = `SM${randomBytes(16).toString("hex")}`;
  expect(await textIn(HOMEOWNER, "AC blowing warm air", sid)).toBe(200);
  expect(await textIn(HOMEOWNER, "AC blowing warm air", sid), "the same delivery again").toBe(200);
  Object.assign(state, requestSince(state.since));
  state.chat.contact = state.contact;
  queuedFor(state.contact).forEach((t) => state.chat.seen.add(t.id));
  // CON-031, INT-043 (D-4360): the ZIP question follows the consent text as its own text.
  expect(textsTo(state.contact)).toEqual(["consent", "zip_question"]);
  const body = rows(`SELECT body FROM messaging.outbox WHERE recipient_id = '${state.contact}' AND template_id = 'consent'`)[0]?.[0] ?? "";
  expect(body.startsWith(CON_031_START)).toBe(true);
  expect(body).not.toMatch(/Reply (OK|YES)/i);
  expect(rows(`SELECT channel FROM intake.consent_record WHERE conversation_id = '${state.conversation}'`)).toEqual([["sms"]]);
  expect(rows(`SELECT count(*) FROM intake.emergency_event WHERE conversation_id = '${state.conversation}'`)).toEqual([["0"]]);
});

test("J1 step 2 TD-INT-50 INT-043 INT-049 INT-016 INT-017 D-460: her ZIP gets the coverage line naming the 2 businesses in her town, then \"How soon?\"", async () => {
  const got = await say(state.chat, "86403");
  expect(templates(got), "steps 4 and 5, then step 6").toEqual(["coverage_count_many", "how_soon"]);
  expect(got.find((t) => t.template === "coverage_count_many")?.body).toMatch(new RegExp(`^${BRAND.short_name}: Got it\\. We'll send this to the 2 `));
  expect(got.find((t) => t.template === "how_soon")?.body).toContain("How soon do you need this?");
});

test("J1 step 3 TD-INT-50 INT-043 INT-030 INT-039 INT-044: \"In the next few days\" gets the trade questions one at a time, then the photo ask, then the name-and-address question", async () => {
  const asks = [/What kind of system is it\?/, /What's happening\?/, /About how old is the system\?/, /Is an error code or flashing light showing\?/, /Anything else\?/];
  for (const [i, answer] of ["2", "1", "1", "1", "2"].entries()) {
    const got = await say(state.chat, answer);
    expect(templates(got), "step 8").toEqual(["trade_question"]);
    expect(got[0]?.body).toMatch(asks[i] ?? /^$/);
  }
  expect(templates(await say(state.chat, "Nothing else")), "step 9").toEqual(["photo_ask"]);
  const last = await say(state.chat, "No photo");
  expect(templates(last), "step 11").toEqual(["name_address"]);
  expect(last[0]?.body).toContain("What is your first name and street address?");
});

test("J1 step 4 TD-INT-50 INT-041 INT-043: her fake name and address get the card; SEND sends it at once and the job's cards are recorded", async ({ page, control }) => {
  const card = await say(state.chat, "Pat Sample, 12 Sample Street");
  expect(templates(card), "step 12").toHaveLength(1);
  expect(["card_shown", "card_link"]).toContain(card[0]?.template);
  expect(card[0]?.body).toMatch(/It goes out in 30 minutes\. Reply EDIT to change it, or SEND to send it now\.$/);
  expect(await say(state.chat, "SEND"), "SEND sends it at once with no text of its own").toEqual([]);
  state.job = rows(`SELECT job_id FROM intake.conversation WHERE id = '${state.conversation}'`)[0]?.[0] ?? "";
  expect(state.job).not.toBe("");
  state.path = await quotesIn(page, control, [state.chat, state.job]);
  expect(rows(`SELECT count(*) FROM quotes.quote WHERE job_id = '${state.job}'`)).toEqual([["2"]]);
});

test("J1 step 5 WORD-005 A11Y-014: both diagnostic-visit quotes in arrival order, the WORD-005 line, a status region present at load", async ({ page }) => {
  await page.goto(BASE + state.path);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Your quotes for request \d+$/);
  await expect(page.locator("main")).toContainText("Every eligible quote shown, in the order received.");
  await expect(page.locator("#quote-status")).toHaveAttribute("role", "status");
  await expect(page.locator("#quotes > li h2")).toHaveText(["Quote #1", "Quote #2"]);
  await expect(page.locator("#quote-1")).toContainText("Diagnostic visit, sample wording 1");
  await expect(page.locator("#quote-2")).toContainText("Diagnostic visit, sample wording 2");
});

test("J1 step 6 PICK-016 PICK-013 PICK-017 PICK-018: review, Go back, pick again, a wrong code then the right one, masked call, focus on the result", async ({ page }) => {
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

test("J1 variant CON-009 CON-012 CON-028: STOP after quotes arrive; the one confirmation, then no further text; the job and its quote page go on", async ({ page, control }) => {
  seedHvacBusinesses(await arizonaDay(control));
  const { chat, conversation } = await open("+19285550146", "AC blowing warm air");
  for (const reply of REPLIES) await say(chat, reply);
  const job = rows(`SELECT job_id FROM intake.conversation WHERE id = '${conversation}'`)[0]?.[0] ?? "";
  const path = await quotesIn(page, control, [chat, job]);
  queuedFor(chat.contact).forEach((t) => chat.seen.add(t.id));
  expect(templates(await say(chat, "STOP")), "the one confirmation").toEqual(["opt_out_confirmation"]);
  expect(await say(chat, "Is anyone coming today?"), "no further text").toEqual([]);
  expect(rows(`SELECT status FROM jobs.job WHERE id = '${job}'`)).toEqual([["quotes_ready"]]);
  await page.goto(BASE + path);
  await expect(page.locator("#quotes > li h2")).toHaveText(["Quote #1", "Quote #2"]);
});

test("J1 variant INT-004 INT-005 D-436: 911-list words first get the 911 text and the consent text, once each; SAFE then gets only the ZIP question", async () => {
  const since = now();
  expect(await textIn("+19285550143", "I smell gas in the kitchen")).toBe(200);
  const gas = requestSince(since);
  // Both texts are queued in one transaction with one `created_at` and a random `id`, so the
  // outbox cannot show which came first; the set is checked here, the order is D-2032's gap.
  expect([...textsTo(gas.contact)].sort()).toEqual(["consent", "emergency_911"]);
  expect(rows(`SELECT count(*) FROM intake.emergency_event WHERE conversation_id = '${gas.conversation}' AND safe_at IS NULL`)).toEqual([["1"]]);
  expect(await textIn("+19285550143", "we're safe")).toBe(200);
  // INT-025, INT-043 (D-4360): on the 911 path the ZIP is asked after SAFE, and nothing else.
  expect(textsTo(gas.contact)).toHaveLength(3);
  expect(textsTo(gas.contact)[2]).toBe("zip_question");
  expect(rows(`SELECT count(*) FROM intake.emergency_event WHERE conversation_id = '${gas.conversation}' AND safe_at IS NOT NULL`)).toEqual([["1"]]);
});

for (const [from, words] of [["+19285550144", "pipe burst under the sink"], ["+19285550145", "AC died, 92 inside"]] as const) {
  test(`J1 variant INT-002 D-436: "${words}" gets no 911 text, only the consent text`, async () => {
    const since = now();
    expect(await textIn(from, words)).toBe(200);
    const request = requestSince(since);
    // CON-031, INT-043 (D-4360): the ZIP question follows the consent text as its own text.
    expect(textsTo(request.contact)).toEqual(["consent", "zip_question"]);
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
  // D-5550: the real path records every move: Intake to Open (cards out), Open to Quotes ready (first quote), then the pick.
  expect(moves).toEqual([["intake", "open"], ["open", "quotes_ready"], ["quotes_ready", "awaiting_confirm"]]);
  expect(audited, "one audit row per status change").toEqual(moves);
  expect(rows(`SELECT count(*) FROM ops.audit_log WHERE action = 'picks.picked' AND subject_id IN
    (SELECT id FROM picks.pick WHERE job_id = '${state.job}')`)).toEqual([["1"]]);
  const dump = spawnSync("pg_dump", [process.env.DATABASE_URL ?? "", "--data-only"], { encoding: "utf8", maxBuffer: 1 << 28 });
  expect(dump.status).toBe(0);
  for (const sentinel of ["9285550142", "928-555-0142", "(928) 555-0142", "928 555 0142"]) {
    expect(dump.stdout.includes(sentinel), `the database outside the sealed vault holds ${sentinel}`).toBe(false);
  }
});
