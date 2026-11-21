// The web chat and request page at /request, end to end on the keyed binary (T594; TD-CON-10,
// TD-A11Y-02, TD-WEB-01, TD-INT-09, TD-INT-30; CON-025, AI-026, A11Y-035, WEB-006, WEB-014;
// D-3744, D-3878, D-4120, D-4174).
//
// The chat's code step needs the chat-codes part, which only the keyed binary has (D-3744), so every
// case runs against `keyedUrl()`, with JavaScript off (A11Y-035: one question per server-rendered
// page). The Verify code is read from the stand-in's `GET /_stand_in/twilio/code`, standing in for
// her phone. The keyed binary runs no worker, so no text reaches the stand-in's sent log: the consent
// record and the queued texts are read through psql from `intake.consent_record` and
// `messaging.outbox`. Her contact is found by the vault's blind index of her number. AI-026's line
// is checked as shown only: outside Live the AI mode is always on, and the off case is T489's unit
// test (D-4120). The texting number's suspension is a `messaging.mode_state` row, restored after.
// The browser's chat POSTs go with its own headers (`Sec-Fetch-Site: same-origin`), never rewritten
// through `page.route`, and the server takes them (TD-SEC-73; SEC-042, A11Y-031; D-4292).
// Fake data only: 928-555-01xx numbers and made-up jobs.
import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";
import { blindIndex, keyedUrl, psql } from "./keyed";

test.use({ javaScriptEnabled: false });

/** The brand file the app embeds (app/core/data/brand.json, D-1932): expected brand words come from it. */
const BRAND = JSON.parse(readFileSync("app/core/data/brand.json", "utf8")) as { display_name: string; short_name: string };

/** INT-004's text as the English template file holds it, with the brand tokens filled. */
const SAFETY_TEXT = (JSON.parse(readFileSync("app/messaging/templates/en.json", "utf8")) as { emergency_911: { text: string } })
  .emergency_911.text.replaceAll("{brand_short}", BRAND.short_name).replaceAll("{brand}", BRAND.display_name);

/** A made-up job with no 911-list words. */
const JOB = "The kitchen faucet drips all night and the cabinet under it is damp.";

/**
 * A fresh fake 928-555-01xx number in the E.164 form the vault and Verify use. The chat hands Verify
 * the number as typed, so a dashed one gets no code (D-4162, a known failure of the served chat).
 * It draws only 0101 to 0141, which no other spec fixes: j1_homeowner opts 0146 out with STOP, and an
 * opted-out number keeps the chat asking for a number, so the code label never shows (INT-001, D-4823).
 */
function fakeNumber(): string {
  return `+1928555${String(101 + Math.floor(Math.random() * 41)).padStart(4, "0")}`;
}

/** The database's clock now, so later reads see only this case's rows. */
function since(): string {
  return psql("SELECT clock_timestamp()");
}

/** The vault contact of `e164`, or "" when the number was never stored. */
function contactOf(e164: string): string {
  const digest = blindIndex("homeowner-phone", e164).toString("hex");
  return psql(`SELECT contact_id FROM privacy.contact_index
    WHERE purpose = 'homeowner-phone' AND key_version = 1 AND digest = decode('${digest}', 'hex')`);
}

/** Template IDs queued to `contact` since `from`, oldest first. */
function textsTo(contact: string, from: string): string[] {
  if (contact === "") return [];
  const out = psql(`SELECT template_id FROM messaging.outbox WHERE recipient_id = '${contact}'
    AND created_at >= '${from}' ORDER BY created_at, template_id`);
  return out === "" ? [] : out.split("\n");
}

/** Channels of the consent records made since `from`, for `contact`, or for anyone when "". */
function consentsSince(from: string, contact = ""): string[] {
  const who = contact === "" ? "" : `AND h.contact_id = '${contact}'`;
  const out = psql(`SELECT r.channel FROM intake.consent_record r JOIN intake.homeowner h ON h.id = r.homeowner_id
    WHERE r.consented_at >= '${from}' ${who}`);
  return out === "" ? [] : out.split("\n");
}

/** The Verify code the stand-in sent to `e164`. */
async function codeFor(e164: string): Promise<string> {
  const reply = await fetch(`http://127.0.0.1:4010/_stand_in/twilio/code?to=${encodeURIComponent(e164)}`);
  return ((await reply.json()) as { code: string }).code;
}

/** A11Y-035: the page asks exactly one question: one visible field in the chat form. */
async function oneQuestion(page: Page, label: string): Promise<void> {
  const form = page.locator("main form");
  await expect(form.locator("textarea, input:not([type=hidden])")).toHaveCount(1);
  await expect(form.getByLabel(label)).toBeVisible();
}

/** Opens the request page on the keyed binary. */
async function openChat(page: Page): Promise<void> {
  await page.goto(`${keyedUrl()}/request`);
}

/** Opens the request page, sends the job and lands on the number question. */
async function toNumber(page: Page): Promise<void> {
  await openChat(page);
  await oneQuestion(page, "What job do you need done?");
  await page.getByLabel("What job do you need done?").fill(JOB);
  await page.getByRole("button", { name: "Send" }).click();
  await oneQuestion(page, "Your mobile number");
}

/** Types `typed` as her number and lands on the code question. */
async function toCode(page: Page, typed: string): Promise<void> {
  await page.getByLabel("Your mobile number").fill(typed);
  await page.getByRole("button", { name: "Text me a code" }).click();
  await oneQuestion(page, "The code we texted you");
}

/** Types `code` (maybe empty) into the code question and sends it. */
async function sendCode(page: Page, code: string): Promise<void> {
  await page.getByLabel("The code we texted you").fill(code);
  await page.getByRole("button", { name: "Check the code" }).click();
}

test("TD-CON-10 TD-WEB-01 CON-025 WEB-006 WEB-014 AI-026: the Spanish draft, the English consent text, the 18-or-older line and AI-026's line sit above the chat box; no agree button", async ({ page }) => {
  const from = since();
  await page.goto(`${keyedUrl()}/request`);
  const main = page.locator("main");
  const html = await main.innerHTML();
  const spanish = html.indexOf('lang="es"');
  const english = html.indexOf('data-fixed="WEB-014"');
  const adult = html.search(/18 or older/i);
  const ai = html.indexOf("An AI service helps word our questions.");
  const form = html.indexOf("<form");
  expect(spanish).toBeGreaterThan(-1);
  expect([spanish < english, english < adult, adult < form, ai > -1 && ai < form]).toEqual([true, true, true, true]);
  await expect(main.locator('[lang="es"]')).toContainText(/revisión por un hablante nativo/);
  await expect(main.locator('[data-fixed="WEB-014"]')).toContainText(/STOP/);
  await expect(main).toContainText(`You must be 18 or older to use ${BRAND.display_name}.`);
  await expect(main.getByRole("button", { name: /agree|^ok$|acepto/i })).toHaveCount(0);
  await expect(main.getByRole("checkbox")).toHaveCount(0);
  expect(consentsSince(from)).toEqual([]);
});

test("TD-CON-10 TD-INT-09 TD-INT-30 TD-A11Y-02 TD-SEC-73 CON-025 A11Y-035 SEC-042: one question per page; only the Verify code until the right code, which records web-chat consent and queues the consent text first", async ({ page }) => {
  const from = since();
  const her = fakeNumber();
  await toNumber(page);
  // Starting the chat and sending the job records no consent and queues nothing (D-470).
  await expect(page.locator("main ol")).toContainText(JOB);
  expect(consentsSince(from)).toEqual([]);
  await toCode(page, her);
  await expect(page.locator("main ol")).toContainText(JOB);
  const contact = contactOf(her);
  const code = await codeFor(her);
  expect(code).toMatch(/^\d{4,10}$/);
  expect([textsTo(contact, from), consentsSince(from, contact)]).toEqual([[], []]);

  await sendCode(page, code === "000000" ? "111111" : "000000");
  await expect(page.locator("#chat-notice")).toContainText("That code doesn't match.");
  await expect(page).toHaveTitle(/^Error: /);
  await oneQuestion(page, "The code we texted you");
  await sendCode(page, "");
  await oneQuestion(page, "The code we texted you");
  expect([textsTo(contact, from), consentsSince(from, contact)]).toEqual([[], []]);

  await sendCode(page, code);
  await expect(page.locator("#chat-notice")).toContainText("we've got your request");
  expect(consentsSince(from, contact)).toEqual(["web_chat"]);
  const texts = textsTo(contact, from);
  expect(texts).toHaveLength(1);
  expect(texts[0]).toMatch(/^consent/);
  const body = psql(`SELECT body FROM messaging.outbox WHERE recipient_id = '${contact}' AND created_at >= '${from}'`);
  const shown = (await page.locator('main [data-fixed="WEB-014"]').innerText()).trim();
  expect(body.trim()).toBe(shown);
  // TD-INT-09: the web chat started a request: the consent record's conversation is open on a job.
  const opened = psql(`SELECT count(*) FROM intake.consent_record r JOIN intake.homeowner h ON h.id = r.homeowner_id
    JOIN intake.conversation c ON c.id = r.conversation_id WHERE h.contact_id = '${contact}' AND r.consented_at >= '${from}'
    AND r.web_chat_session IS NOT NULL`);
  expect(opened).toBe("1");
});

test("TD-CON-10 TD-INT-30 CON-025: a wrong code alone, then no code, sends nothing more and records no consent", async ({ page }) => {
  const from = since();
  const her = fakeNumber();
  await toNumber(page);
  await toCode(page, her);
  const contact = contactOf(her);
  const code = await codeFor(her);
  await sendCode(page, code === "222222" ? "333333" : "222222");
  await sendCode(page, "");
  await oneQuestion(page, "The code we texted you");
  expect([textsTo(contact, from), consentsSince(from, contact)]).toEqual([[], []]);
  expect(await codeFor(her)).toBe(code);
});

test("TD-CON-10 TD-SEC-73 SEC-042: a valid chat form posted cross-site is refused and records nothing; the same form same-origin is taken", async ({ page }) => {
  const from = since();
  await page.goto(`${keyedUrl()}/request`);
  const form: Record<string, string> = { step: "message", words: JOB };
  for (const name of ["csrf", "chat", "message", "g", "said"]) {
    form[name] = (await page.locator(`main form input[name="${name}"]`).getAttribute("value")) ?? "";
  }
  expect(form.csrf).not.toBe("");
  const url = `${keyedUrl()}/request`;
  const post = (origin: string, site: string) =>
    page.request.post(url, { form, headers: { origin, "sec-fetch-site": site }, maxRedirects: 0 });
  // The pre-session cookie and token are valid, so only the cross-site check can refuse this one.
  const refused = await post("https://elsewhere.example", "cross-site");
  expect(refused.status()).toBe(403);
  expect(await refused.text()).not.toContain(JOB);
  expect(consentsSince(from)).toEqual([]);
  // The same form from the page's own origin passes both checks: the token was valid (D-4292).
  const taken = await post(`https://${new URL(url).host}`, "same-origin");
  expect(taken.status()).toBe(200);
  expect(await taken.text()).toContain(JOB);
  expect(consentsSince(from)).toEqual([]);
});

test("TD-CON-10 TD-INT-30 INT-004: with the texting number suspended, a 911-list message gets the INT-004 text in the chat", async ({ page }) => {
  const actor = "00000000-0000-4000-8000-0000000000aa";
  const last = psql(`SELECT texting_on, line_state, coalesce(suspended_since::text, ''), launch_channel
    FROM messaging.mode_state WHERE audience = 'homeowner' ORDER BY seq DESC LIMIT 1`);
  const [on = "t", state = "working", sinceAt = "", channel = "sms"] = last === "" ? [] : last.split("|");
  psql(`INSERT INTO messaging.mode_state (audience, texting_on, line_state, suspended_since, launch_channel, actor_id, changed_at)
    VALUES ('homeowner', false, 'suspended', now(), 'sms', '${actor}', now())`);
  try {
    const from = since();
    await openChat(page);
    await page.getByLabel("What job do you need done?").fill("I smell gas in the kitchen near the stove.");
    await page.getByRole("button", { name: "Send" }).click();
    await expect(page.locator("main ol")).toContainText(SAFETY_TEXT);
    await oneQuestion(page, "What job do you need done?");
    expect(consentsSince(from)).toEqual([]);
  } finally {
    const at = sinceAt === "" ? "NULL" : `'${sinceAt}'`;
    psql(`INSERT INTO messaging.mode_state (audience, texting_on, line_state, suspended_since, launch_channel, actor_id, changed_at)
      VALUES ('homeowner', ${on === "t" ? "true" : "false"}, '${state}', ${at}, '${channel}', '${actor}', now())`);
  }
});
