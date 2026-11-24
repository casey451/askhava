/// <reference types="node" />
// TD-INT-50 (T676, D-4360, D-5657): the urgent Emergency path and the 911 path of INT-043's
// "Intake order" table, run through the served Twilio route of the keyed web binary
// (`keyedUrl()`), with the stand-ins and fake data only. Each text is posted signed as Twilio
// signs it (SEC-016); the reply texts are read from the `messaging.outbox` rows in the order the
// suite's worker queued them (it shares the keyed binary's ID secret and vault keys), because
// no homeowner line is set there, so nothing reaches the stand-in carrier (D-5717). Two fake active HVAC businesses are fixture rows in her town, so the
// coverage line counts them and the job's cards go to them. Each path runs to its cards recorded
// (`quotes.job_card`): on the urgent Emergency path the cards go at once on her choice (INT-029),
// before the trade questions; the 911 path asks the ZIP only after SAFE (INT-025, INT-004).
import { spawnSync } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test, expect, type Control } from "./fixtures";
import { keyedTextIn } from "./twilio_signature";

/** The brand file the app embeds (app/core/data/brand.json, D-1932): expected brand words come from it. */
const BRAND = JSON.parse(readFileSync(join(import.meta.dirname, "..", "..", "app", "core", "data", "brand.json"), "utf8")) as { short_name: string };

test.describe.configure({ mode: "serial", timeout: 240_000 });

/** Rows of `sql` from psql on DATABASE_URL, each a list of column values. */
function rows(sql: string): string[][] {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-AtqF", "\t", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed: ${done.stderr}`);
  return done.stdout.split("\n").filter((line) => line !== "").map((line) => line.split("\t"));
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
    rows(`WITH b AS (INSERT INTO businesses.business (id, status, after_hours, created_at, status_changed_at)
        VALUES ('${randomUUID()}', 'active', true, now(), now()) RETURNING id),
      m AS (INSERT INTO businesses.team_member (business_id, mailbox_id, is_owner, cards_and_quotes, confirm_and_billing, added_at)
        SELECT id, gen_random_uuid(), true, true, true, now() FROM b RETURNING id, business_id),
      es AS (INSERT INTO businesses.emergency_switch (member_id, business_id, switch_on, test_sent_at, yes_at)
        SELECT id, business_id, true, now(), now() FROM m),
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

/** One text the app queued for her: its template ID and body (the outbox keeps the body of a queued text; this harness has no homeowner number, so none is sent). */
interface Queued { id: string; template: string; body: string }

/** A request's texts, read per reply from the outbox: `seen` holds the rows an earlier reply already showed. */
interface Chat { from: string; seen: Set<string>; contact: string }

/** Every text queued for the contact of the request `from` opened, as the outbox holds it. */
function queuedFor(contact: string): Queued[] {
  const json = rows(`SELECT coalesce(json_agg(json_build_object('id', id, 'template', template_id, 'body', body) ORDER BY created_at), '[]')
    FROM messaging.outbox WHERE recipient_id = '${contact}'`)[0]?.[0] ?? "[]";
  return JSON.parse(json) as Queued[];
}

/** The texts one inbound text of hers made the app queue, in no order (one reply queues its texts in one transaction). */
async function say(chat: Chat, body: string): Promise<Queued[]> {
  const since = rows("SELECT clock_timestamp()")[0]?.[0] ?? "";
  expect(await keyedTextIn(chat.from, body, "+15005550006")).toBe(200);
  if (chat.contact === "") {
    chat.contact = rows(`SELECT h.contact_id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id
      WHERE c.opened_at >= '${since}' ORDER BY c.opened_at DESC LIMIT 1`)[0]?.[0] ?? "";
  }
  const fresh = queuedFor(chat.contact).filter((t) => !chat.seen.has(t.id));
  fresh.forEach((t) => chat.seen.add(t.id));
  return fresh;
}

/** The job her request opened, from her conversation. */
function jobOf(contact: string): string {
  return rows(`SELECT c.job_id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id
    WHERE h.contact_id = '${contact}' AND c.job_id IS NOT NULL LIMIT 1`)[0]?.[0] ?? "";
}

const CARDS_WAIT_MS = 20_000;

/** Waits until the job has both cards recorded (the worker's task writes them), then returns when the last was sent. */
async function cardsRecorded(job: string): Promise<string> {
  const deadline = Date.now() + CARDS_WAIT_MS;
  while (Date.now() < deadline) {
    const [count = "0", sentAt = ""] = rows(`SELECT count(*), max(sent_at) FROM quotes.job_card WHERE job_id = '${job}'`)[0] ?? [];
    if (count === "2") return sentAt;
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error("the job's cards were not recorded");
}

const templates = (got: Queued[]): string[] => got.map((t) => t.template).sort();

/** Her ZIP, the coverage line, "How soon?" and "Today, it's urgent", up to the Emergency choice of step 7 (at night the INT-033 count first). */
async function fromZip(chat: Chat): Promise<void> {
  const zip = await say(chat, "86403");
  expect(templates(zip), "steps 4 and 5, then step 6").toEqual(["coverage_count_many", "how_soon"]);
  expect(zip.find((t) => t.template === "coverage_count_many")?.body).toMatch(new RegExp(`^${BRAND.short_name}: Got it\\. We'll send this to the 2 `));
  expect(zip.find((t) => t.template === "how_soon")?.body).toContain("How soon do you need this?");
  const choice = templates(await say(chat, "1")).join(",");
  expect(["emergency_choice", "emergency_choice,night_count_many"], "step 7 (INT-006, INT-033)").toContain(choice);
}

/** Her name and address, the last step of the Emergency path: the cards are already out, so no card is shown (INT-029, INT-041). */
async function finishEmergency(chat: Chat, job: string): Promise<void> {
  expect(templates(await say(chat, "No photo")), "step 11").toEqual(["name_address"]);
  expect(templates(await say(chat, "Pat Sample, 12 Sample Street")), "step 12 is skipped").toEqual([]);
  expect(rows(`SELECT count(*) FROM messaging.outbox WHERE recipient_id = '${chat.contact}' AND template_id IN ('card_shown', 'card_link')`)).toEqual([["0"]]);
  expect(rows(`SELECT count(*) FROM quotes.job_card WHERE job_id = '${job}'`)).toEqual([["2"]]);
}

/** Her Emergency choice: the emergency cards are recorded (INT-029) while the one question of step 8 is still unanswered. */
async function chooseEmergency(chat: Chat): Promise<string> {
  const emergency = await say(chat, "1");
  expect(templates(emergency), "step 8 follows the cards (INT-029)").toEqual(["trade_question"]);
  const job = jobOf(chat.contact);
  await cardsRecorded(job);
  expect(rows(`SELECT emergency, status FROM jobs.job WHERE id = '${job}'`)).toEqual([["t", "open"]]);
  expect(rows(`SELECT emergency FROM quotes.shown_card WHERE job_id = '${job}'`), "the emergency card stored at the choice").toEqual([["t"]]);
  expect(templates(await say(chat, "1")), "step 8's answer, then cards-out (TXT-032) and step 9").toEqual(["photo_ask", "request_out"]);
  return job;
}

test("TD-INT-50 INT-043 INT-029 INT-006: the urgent path sends the emergency cards on her choice, before the trade questions are answered, with no card shown to her", async ({ control }) => {
  seedHvacBusinesses(await arizonaDay(control));
  const chat: Chat = { from: "+19285550151", seen: new Set(), contact: "" };
  expect(templates(await say(chat, "AC blowing warm air")), "steps 3 and 4 (CON-031)").toEqual(["consent", "zip_question"]);
  await fromZip(chat);
  await finishEmergency(chat, await chooseEmergency(chat));
});

test("TD-INT-50 INT-043 INT-004 INT-005 INT-025 INT-029: the 911 path sends the 911 text, waits for SAFE, then the ZIP, and reaches its emergency cards", async ({ control }) => {
  seedHvacBusinesses(await arizonaDay(control));
  const chat: Chat = { from: "+19285550152", seen: new Set(), contact: "" };
  const first = await say(chat, "AC blowing warm air and I smell gas");
  expect(templates(first), "steps 2 and 3: the 911 text and the consent text, no ZIP question yet").toEqual(["consent", "emergency_911"]);
  expect(first.find((t) => t.template === "emergency_911")?.body).toMatch(/isn't an emergency service.*Reply SAFE when you're safe\.$/);
  expect(rows(`SELECT count(*) FROM intake.emergency_event WHERE safe_at IS NULL AND conversation_id IN
    (SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id WHERE h.contact_id = '${chat.contact}')`)).toEqual([["1"]]);
  expect(templates(await say(chat, "SAFE")), "step 4 after SAFE (INT-025)").toEqual(["zip_question"]);
  await fromZip(chat);
  await finishEmergency(chat, await chooseEmergency(chat));
});
