/// <reference types="node" />
// Journey J3's shared steps (T118; SEC-005, SEC-006, SEC-043, OPS-001, OPS-003, OPS-010, OPS-024,
// FEE-007, FEE-016, FEE-017, FEE-026, FEE-028, INT-003, CON-009, TXT-021; D-3744, D-3844, D-3889,
// D-5282 to D-5285), driven by j3_operator.spec.ts (mouse) and tests/a11y/j3_keyboard.spec.ts
// (keyboard) through a `Driver`. All on the keyed binary (Stripe, D-3844), the owner signed in only
// through owner_session.ts (D-3744). Owner sessions run on the system clock, so their age is moved
// in the database (D-5282); the worker's app clock moves for safe mode, whose High alert is a seeded
// `ops.vendor_errors` one at the app clock (D-5283). `restore` puts the shared state back. Fake data only.
import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { expect, type APIRequestContext, type Locator, type Page } from "@playwright/test";
import type { Control } from "./fixtures";
import { deliverSuccess, standInCharges, STAND_IN } from "./j2";
import { confirmPath, keyedUrl, openAsOwner, psql, seedPick, serve, signIn, stripeCalls, type Seeded, WAITING } from "./keyed";
import { ownerCode, signInOwner } from "./owner_session";
import { keyedTextIn } from "./twilio_signature";

/** How a spec works the page: the mouse (`MOUSE`) or the keyboard alone (the a11y spec's). */
export interface Driver {
  press(target: Locator): Promise<void>; // activates a button or link
  choose(target: Locator): Promise<void>; // selects a radio button
  type(target: Locator, text: string): Promise<void>; // types into an empty field
}

/** The mouse driver of `j3_operator.spec.ts`. */
export const MOUSE: Driver = {
  press: (target) => target.click(),
  choose: (target) => target.check(),
  type: (target, text) => target.fill(text),
};

/** What one J3 run touches, for the steps and the closing checks. */
export interface J3 {
  page: Page;
  control: Control;
  drive: Driver;
  auditFrom: number; // the first audit row ID of this run
  textsFrom: number; emailsFrom: number; // the fake carrier's text and email counts when the run began
  picks: Seeded[]; // the picks this run paid or tried to pay
  alert?: string; // the seeded High alert, once raised
}

/** The brand file the app embeds (D-1932). */
const BRAND = JSON.parse(readFileSync("app/core/data/brand.json", "utf8")) as { display_name: string; short_name: string };
/** INT-004's 911 safety text as the English template holds it, brand tokens filled. */
const SAFETY_TEXT = (JSON.parse(readFileSync("app/messaging/templates/en.json", "utf8")) as { emergency_911: { text: string } })
  .emergency_911.text.replaceAll("{brand_short}", BRAND.short_name).replaceAll("{brand}", BRAND.display_name);
/** keyed.ts's seeded homeowner: the sentinel contact details that must stay in the vault. */
const SENTINELS = ["+19285550163", "928-555-0163", "Robin Fakename", "12 Sample Street"];
/** The fake number that texts STOP in step 5 (no other spec uses it). */
const STOP_PHONE = "+19285550157";
/** Any `sk_test_` key: the Stripe stand-in checks only the prefix. */
const STRIPE_AUTH = { authorization: `Bearer sk_test_${"e2e".repeat(4)}` };
const [MINUTE, HOUR] = [60, 3600];

/**
 * SEC-005, SEC-043 (D-5282): moves the owner sessions back by `minutes`: with `whole` their start
 * (the stored end then no later than 12 hours after it, as the server stores it), else their
 * last use and stored end. The web binaries time sessions by the system clock
 * (`app/server/src/lib.rs` passes `SystemClock` to the owner routes), so the app clock's advance
 * cannot age them.
 */
function ageOwnerSessions(minutes: number, whole: boolean): void {
  const by = `interval '${String(minutes)} minutes'`;
  if (whole) {
    psql(`UPDATE portals.session SET created_at = created_at - ${by},
      expires_at = LEAST(expires_at, created_at - ${by} + interval '12 hours') WHERE realm = 'owner'`);
  } else {
    psql(`UPDATE portals.owner_session SET last_seen_at = last_seen_at - ${by}`);
    psql(`UPDATE portals.session SET expires_at = expires_at - ${by} WHERE realm = 'owner'`);
  }
}

/** The status the owner door gives the kept session cookie in `page`'s context: 200 live, 401 gone. */
async function ownerDoor(page: Page): Promise<number> {
  return (await page.request.get(`${keyedUrl()}/owner/step-up`, { maxRedirects: 0 })).status();
}

/** Presses the owner page's button `name` and waits for its form's POST to answer and the page to load. */
async function submit(j: J3, name: string): Promise<void> {
  const answered = j.page.waitForResponse((r) => r.request().method() === "POST" && r.url().startsWith(`${keyedUrl()}/owner/`));
  await j.drive.press(j.page.getByRole("button", { name }));
  expect((await answered).status(), `the POST of "${name}"`).toBeLessThan(400);
  await j.page.waitForLoadState();
}

/** SEC-043: a fresh TOTP on the step-up page, typed and sent with the driver. */
export async function stepUp(j: J3): Promise<void> {
  await j.page.goto(`${keyedUrl()}/owner/step-up`);
  await j.drive.type(j.page.getByLabel("Code", { exact: true }), await ownerCode());
  await j.drive.press(j.page.getByRole("button", { name: "Check code" }));
  await expect(j.page.locator("#step-up-done")).toHaveText(/That worked/);
}

/** The newest homeowner mode-state row, as `texting_on|launch_channel|line_state|suspended_since`. */
const modeState = (): string =>
  psql(`SELECT texting_on || '|' || launch_channel || '|' || line_state || '|' || coalesce(suspended_since::text, '')
    FROM messaging.mode_state WHERE audience = 'homeowner' ORDER BY seq DESC LIMIT 1`);

/** What `restore` puts back: the homeowner mode state and the fee mode before the run. */
export interface Before { mode: string; feeMode: string }
/** Reads what `restore` puts back after the run. */
export function before(): Before {
  return { mode: modeState(), feeMode: psql("SELECT mode FROM payments.fee_terms") };
}

/**
 * TD-TXT-15, OPS-001: puts the homeowner mode state, the fee mode, the plumbing pause and safe mode
 * back as they were, so a failure leaves the shared database clean for the specs that follow.
 */
export function restore(was: Before, alert: string | undefined): void {
  const target = was.mode || "true|sms|working|"; // no row before the run: the app's default applies
  const [on = "true", channel = "sms", line = "working", since = ""] = target.split("|");
  if ((modeState() || "true|sms|working|") !== target) {
    psql(`INSERT INTO messaging.mode_state (audience, texting_on, line_state, suspended_since, launch_channel, actor_id, changed_at)
      VALUES ('homeowner', ${on === "true" ? "true" : "false"}, '${line}', ${since === "" ? "NULL" : `'${since}'`}, '${channel}',
        '00000000-0000-4000-8000-0000000000aa', now())`);
  }
  psql(`UPDATE payments.fee_terms SET mode = '${was.feeMode}' WHERE mode <> '${was.feeMode}'`);
  psql(`UPDATE ops.pause_switch SET paused = false WHERE name = 'trade' AND paused`);
  if (alert !== undefined) psql(`UPDATE ops.alert SET resolved_at = now() WHERE id = '${alert}' AND resolved_at IS NULL`);
  psql(`UPDATE ops.safe_mode SET "on" = false, since = NULL WHERE "on"`);
}

/** The run's starting marks. */
export async function begin(page: Page, control: Control, drive: Driver): Promise<J3> {
  const auditFrom = Number(psql("SELECT coalesce(max(id), 0) + 1 FROM ops.audit_log"));
  const [textsFrom, emailsFrom] = [(await control.sentTexts()).length, (await control.sentEmails()).length];
  page.setDefaultTimeout(30_000);
  return { page, control, drive, auditFrom, textsFrom, emailsFrom, picks: [] };
}

/** J3 step 1 (SEC-005): idle past 30 minutes, then password and TOTP; a session over 12 hours is refused. */
export async function step1SignIn(j: J3): Promise<void> {
  ageOwnerSessions(31, false);
  await signInOwner(j.page, keyedUrl());
  await expect(j.page).toHaveURL(/\/owner\/step-up$/);
  expect(psql(`SELECT count(*) FROM ops.audit_log WHERE id >= ${String(j.auditFrom)} AND action = 'portals.owner_signed_in'`)).toBe("1");
  ageOwnerSessions(11 * 60 + 58, true);
  expect(await ownerDoor(j.page), "an owner session 11 hours 58 minutes old").toBe(200);
  ageOwnerSessions(3, true);
  expect(await ownerDoor(j.page), "an owner session 12 hours 1 minute old").toBe(401);
  await signInOwner(j.page, keyedUrl());
  expect(await ownerDoor(j.page)).toBe(200);
}

/** TD-TXT-15 (TXT-021): the launch channel to web-and-email; homeowner texting goes off with it. */
export async function webAndEmail(j: J3): Promise<void> {
  await stepUp(j);
  await j.page.goto(`${keyedUrl()}/owner/home`);
  await j.drive.choose(j.page.getByLabel("Web and email only"));
  await submit(j, "Save launch channel");
  await expect(j.page.locator("#homeowner-texting")).toContainText("Texting to homeowners is off.");
  expect(modeState()).toMatch(/^false\|web_and_email\|/);
}

/** The plumbing trade's row on the owner home. */
const plumbing = (page: Page): Locator => page.locator("#pause-switches li").filter({ hasText: /plumb/i });
/** J3 step 2 (OPS-003): pause the plumbing trade switch, then unpause it, as stored and shown (D-5284). */
export async function step2Pause(j: J3): Promise<void> {
  await j.page.goto(`${keyedUrl()}/owner/home`);
  await j.drive.press(plumbing(j.page).getByRole("button", { name: /^Pause / }));
  await expect(plumbing(j.page)).toContainText("is paused.");
  expect(psql("SELECT count(*) FROM ops.pause_switch WHERE name = 'trade' AND paused")).toBe("1");
  await j.drive.press(plumbing(j.page).getByRole("button", { name: /^Resume / }));
  await expect(plumbing(j.page)).toContainText("is running.");
  expect(psql("SELECT count(*) FROM ops.pause_switch WHERE name = 'trade' AND paused")).toBe("0");
}

/** The stand-in's charge for `pick`: amount and amount refunded, in cents. */
async function standInCharge(request: APIRequestContext, pick: string): Promise<{ amount: number; refunded: number }[]> {
  const reply = await request.get(`${STAND_IN}/stripe/v1/charges?created[gte]=0&limit=100`, { headers: STRIPE_AUTH });
  const listed = (await reply.json()) as { data: { metadata?: { pick_id?: string }; status: string; amount: number; amount_refunded?: number }[] };
  return listed.data
    .filter((c) => c.metadata?.pick_id === pick && c.status === "succeeded")
    .map((c) => ({ amount: c.amount, refunded: c.amount_refunded ?? 0 }));
}

/** A page in its own browser context, so a business sign-in never clears the owner's cookie. */
async function businessPage(j: J3): Promise<Page> {
  const browser = j.page.context().browser();
  if (browser === null) throw new Error("no browser for a second context");
  const page = await (await browser.newContext()).newPage();
  page.setDefaultTimeout(30_000);
  return page;
}

/** Pays a new $25 pick through the Confirm page with a good card until the server waits for Stripe. */
async function paidPick(j: J3, page: Page): Promise<Seeded> {
  const seeded = seedPick();
  j.picks.push(seeded);
  await openAsOwner(page, seeded, "pm_card_visa");
  await page.getByRole("button", { name: "Confirm and pay $25" }).click();
  await expect(page.locator("#confirm-status")).toHaveText(WAITING);
  return seeded;
}

/** J3 step 3 (FEE-026, FEE-028, SEC-043): refund a paid charge on its review step: one Stripe refund, one ledger row. */
export async function step3Refund(j: J3): Promise<void> {
  const paid = await paidPick(j, await businessPage(j));
  await stepUp(j);
  await j.page.goto(`${keyedUrl()}/owner/refunds/${paid.pick}`);
  const form = await j.page.locator("form").filter({ has: j.page.getByRole("button", { name: "Refund this charge" }) })
    .evaluate((f) => ({ action: (f as HTMLFormElement).action, body: new URLSearchParams(new FormData(f as HTMLFormElement) as unknown as string[][]).toString() }));
  const refund = j.page.waitForResponse((r) => r.request().method() === "POST" && r.url() === form.action);
  await j.drive.press(j.page.getByRole("button", { name: "Refund this charge" }));
  expect((await refund).status()).toBe(200);
  await expect(j.page.getByRole("status")).toHaveText("Refunded in full.");
  const again = await j.page.evaluate(async ({ action, body }) => {
    const reply = await fetch(action, { method: "POST", body: new URLSearchParams(body) });
    return { status: reply.status, text: await reply.text() };
  }, form);
  expect(again.status, "the same refund form posted again").toBe(200);
  expect(again.text).toContain("Nothing new was sent. A refund was already done or is on its way.");
  expect(await standInCharge(j.page.request, paid.pick)).toEqual([{ amount: 2500, refunded: 2500 }]);
  expect(psql(`SELECT count(*) FROM payments.ledger_entry WHERE job_id = '${paid.job}' AND kind = 'refund'`)).toBe("1");
}

/** `payments::api::AUTHORIZATION_VERSION`, the least version a Confirm accepts (keyed.ts). */
const AUTHORIZATION_VERSION = 2;

/** The fee mode the database holds and its authorization version. */
const feeTerms = (): string => psql("SELECT mode || '|' || authorization_version FROM payments.fee_terms");

/** Switches the fee mode on the owner home to the radio labelled `label`, after a fresh code (SEC-043). */
async function feeMode(j: J3, label: string): Promise<void> {
  await stepUp(j);
  await j.page.goto(`${keyedUrl()}/owner/home`);
  await j.drive.choose(j.page.getByLabel(label));
  await submit(j, "Save fee mode");
}

/**
 * J3 step 4 (FEE-007, SEC-043, D-072): flat fee mode after a fresh code is logged, and a business
 * holding only the old authorization is refused at Confirm until it accepts the new one (D-3857:
 * the refusal only); then back to fees by quote size.
 */
export async function step4FeeMode(j: J3): Promise<void> {
  const [, stored = "1"] = feeTerms().split("|");
  const old = String(Math.max(Number(stored), AUTHORIZATION_VERSION));
  await feeMode(j, "One flat fee for each job type");
  const [mode, version = "0"] = feeTerms().split("|");
  expect(mode).toBe("flat");
  expect(Number(version), "FEE-007: the switch needs a new authorization").toBeGreaterThan(Number(old));
  expect(psql(`SELECT count(*) FROM ops.audit_log WHERE id >= ${String(j.auditFrom)} AND action = 'payments.fee_mode_switched'`)).toBe("1");
  const seeded = seedPick();
  j.picks.push(seeded);
  const page = await businessPage(j);
  await serve(page, "pm_card_visa");
  await signIn(page, seeded.owner);
  await page.goto(keyedUrl() + confirmPath(seeded.pick));
  await expect.poll(() => stripeCalls(page)).toEqual(["init test key", "mount #card-element"]);
  psql(`INSERT INTO payments.card_authorization (business_id, version, text, accepted_at)
    VALUES ('${seeded.business}', ${old}, 'Fake earlier card authorization.', now())`);
  await j.drive.press(page.getByRole("button", { name: "Confirm and pay $25" }));
  await expect(page.locator("#confirm-status")).toContainText("Accept the updated card authorization in your portal, then confirm. Nothing was charged.");
  expect(await standInCharges(j.page.request, seeded.pick)).toEqual([]);
  await page.context().close();
  await signInOwner(j.page, keyedUrl());
  await feeMode(j, "By the size of each quote");
  expect(feeTerms()).toMatch(/^tiers\|/);
}

/** OPS-004, D-5283: one pushed High `ops.vendor_errors` alert at the app clock. */
async function raiseHighAlert(j: J3): Promise<void> {
  const [at, id] = [await j.control.now(), randomUUID()];
  psql(`WITH a AS (INSERT INTO ops.alert (id, raised_at, kind, severity) VALUES ('${id}', '${at}', 'ops.vendor_errors', 'high') RETURNING id)
    INSERT INTO ops.alert_push (alert_id, pushed_at) SELECT id, '${at}' FROM a`);
  j.alert = id;
}

/** Polls `sql` until it returns `want`, moving the app clock a minute each try so the worker runs. */
async function untilTasks(j: J3, sql: string, want: string): Promise<void> {
  await expect.poll(async () => {
    await j.control.advanceClock(MINUTE);
    return psql(sql);
  }, { timeout: 60_000, intervals: [1_000] }).toBe(want);
}

/** OPS-001: safe mode on 24 hours after the push, with its audit row. */
async function safeModeStarts(j: J3): Promise<void> {
  await j.control.advanceClock(24 * HOUR);
  await untilTasks(j, `SELECT "on" FROM ops.safe_mode`, "t");
}

/**
 * INT-003, CON-009: in safe mode a gas smell gets the 911 safety text on screen and a STOP is taken
 * at once; with the line closed by web-and-email its confirmation is no text (D-5285).
 */
async function textsStillGo(j: J3): Promise<void> {
  const page = await businessPage(j);
  await page.goto(`${keyedUrl()}/request`);
  await j.drive.type(page.getByLabel("What job do you need done?"), "I smell gas in the kitchen near the stove.");
  await j.drive.press(page.getByRole("button", { name: "Send" }));
  await expect(page.locator("main ol")).toContainText(SAFETY_TEXT);
  await page.context().close();
  const mark = psql("SELECT clock_timestamp()");
  expect(await keyedTextIn(STOP_PHONE, "STOP")).toBe(200);
  expect(psql(`SELECT count(*) FROM messaging.optout_event WHERE action = 'opt_out' AND effective_at >= '${mark}'`)).toBe("1");
  // D-5285: under web-and-email the homeowner line is closed, so the CON-009 confirmation is not
  // queued as a text (CON-030, D-2629, `app/messaging/src/optout.rs` `confirm`).
  expect(psql(`SELECT count(*) FROM messaging.outbox WHERE template_id = 'opt_out_confirmation' AND created_at >= '${mark}'`)).toBe("0");
}

/** OPS-024: a Confirm and pay press in safe mode is refused with the "try again" line and charges nothing. */
async function pressRefused(j: J3, seeded: Seeded): Promise<Page> {
  const page = await businessPage(j);
  await openAsOwner(page, seeded, "pm_card_visa");
  await j.drive.press(page.getByRole("button", { name: "Confirm and pay $25" }));
  await expect(page.locator("#confirm-status")).toHaveText("We can't take payments right now. Nothing was charged. Try again in a few minutes.");
  expect(await standInCharges(j.page.request, seeded.pick)).toEqual([]);
  return page;
}

/** Acknowledges the alert on the owner home with the driver (OPS-001: no fresh code needed). */
async function acknowledge(j: J3): Promise<void> {
  await j.page.goto(`${keyedUrl()}/owner/home`);
  await submit(j, "I've seen this alert");
  await expect(j.page.getByRole("button", { name: "I've seen this alert" })).toHaveCount(0);
}

/**
 * J3 step 5 (OPS-001, OPS-024, INT-003, CON-009, SEC-043, FEE-016, FEE-017, D-133): a High alert
 * left 24 hours starts safe mode; a paid pick's release is held, texts still go, a Confirm press
 * is refused; the idle owner signs in again and acknowledges; the held release goes ahead and the
 * refused business's next press charges once.
 */
export async function step5SafeMode(j: J3): Promise<void> {
  const held = await paidPick(j, await businessPage(j));
  await raiseHighAlert(j);
  await safeModeStarts(j);
  expect(await deliverSuccess(j.page.request, held.pick)).toBe(200);
  expect(psql(`SELECT count(*) FROM picks.release WHERE pick_id = '${held.pick}'`)).toBe("0");
  expect(psql(`SELECT count(*) FROM core.task WHERE kind = 'payments.held_release' AND payload::text LIKE '%${held.pick}%'`)).not.toBe("0");
  await textsStillGo(j);
  const refused = seedPick();
  j.picks.push(refused);
  const confirm = await pressRefused(j, refused);
  ageOwnerSessions(31, false);
  expect(await ownerDoor(j.page), "the owner session after 31 minutes idle").toBe(401);
  await signInOwner(j.page, keyedUrl());
  await acknowledge(j);
  await untilTasks(j, `SELECT "on" FROM ops.safe_mode`, "f");
  await untilTasks(j, `SELECT count(*) FROM picks.release WHERE pick_id = '${held.pick}'`, "1");
  await confirm.reload();
  await j.drive.press(confirm.getByRole("button", { name: "Confirm and pay $25" }));
  await expect(confirm.locator("#confirm-status")).toHaveText(WAITING);
  expect(await standInCharges(j.page.request, refused.pick)).toEqual([2500]);
  await confirm.context().close();
}

/** The actions J3 makes as the owner, each shown in the viewer with its actor (SEC-006, OPS-010). */
const OWNER_ACTIONS = ["portals.owner_signed_in", "switch_off", "switch_on", "portals.owner_refund",
  "payments.fee_mode_switched", "messaging.mode_changed", "ops.alert_owner_acknowledged"];

/** J3 step 6 (SEC-006, OPS-010): the audit log viewer shows every action above with its actor. */
export async function step6AuditLog(j: J3): Promise<void> {
  for (const action of [...OWNER_ACTIONS, "ops.safe_mode_on", "ops.safe_mode_off"]) {
    const id = psql(`SELECT min(id) FROM ops.audit_log WHERE id >= ${String(j.auditFrom)} AND action = '${action}'`);
    expect(id, `an audit row for ${action}`).not.toBe("");
    await j.page.goto(`${keyedUrl()}/owner/audit-log?after=${String(Number(id) - 1)}`);
    const row = j.page.getByRole("region", { name: "Audit log rows" }).getByRole("row").nth(1);
    await expect(row).toContainText(action);
    await expect(row).toContainText(OWNER_ACTIONS.includes(action) ? "Owner" : "System");
  }
}

/**
 * The closing checks (docs/test-design.md section 5, TD-PRIV-02, TD-TXT-15): no outbound text and
 * no duplicate email; one stand-in charge at most per pick and the ledger equal to the stand-in to
 * the cent; every pause and safe-mode change has its audit row; no sentinel contact detail on the
 * owner pages or in any email.
 */
export async function closingChecks(j: J3): Promise<void> {
  expect((await j.control.sentTexts()).length - j.textsFrom, "TD-TXT-15: texts sent").toBe(0);
  const emails = await j.control.sentEmails();
  const keys = emails.slice(j.emailsFrom).map((e) => `${e.to.join(",")}|${e.subject}|${e.text}`);
  expect(new Set(keys).size, "no duplicate email").toBe(keys.length);
  for (const seeded of j.picks) {
    const charges = await standInCharge(j.page.request, seeded.pick);
    expect(charges.length, `charges for ${seeded.pick}`).toBeLessThanOrEqual(1);
    const net = charges.reduce((sum, c) => sum + c.amount - c.refunded, 0);
    const ledger = psql(`SELECT coalesce(sum(CASE kind WHEN 'charge' THEN amount_cents WHEN 'refund' THEN -abs(amount_cents) ELSE 0 END), 0)
      FROM payments.ledger_entry WHERE job_id = '${seeded.job}'`);
    expect(Number(ledger), `ledger against the stand-in for ${seeded.pick}`).toBe(net);
  }
  const from = String(j.auditFrom);
  expect(psql(`SELECT count(*) FROM ops.audit_log WHERE id >= ${from} AND subject_kind = 'ops.pause_switch'`)).toBe("2");
  expect(psql(`SELECT count(*) FROM ops.audit_log WHERE id >= ${from} AND action IN ('ops.safe_mode_on', 'ops.safe_mode_off')`)).toBe("2");
  const shown = [await j.page.content(), ...emails.map((e) => e.text + e.html)].join("\n");
  for (const sentinel of SENTINELS) expect(shown, "TD-PRIV-02").not.toContain(sentinel);
}

/** TD-TXT-15: the launch channel back to By text, and homeowner texting on again. */
export async function byText(j: J3): Promise<void> {
  await stepUp(j);
  await j.page.goto(`${keyedUrl()}/owner/home`);
  await j.drive.choose(j.page.getByLabel("By text"));
  await submit(j, "Save launch channel");
  await submit(j, "Turn homeowner texting on");
  await expect(j.page.locator("#homeowner-texting")).toContainText("Texting to homeowners is on.");
  expect(modeState()).toMatch(/^true\|sms\|/);
}
