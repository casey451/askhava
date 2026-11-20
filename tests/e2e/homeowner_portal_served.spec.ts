// TD-PRIV-21's browser half: the served homeowner portal on T452's keyed web binary (T307, D-2128,
// D-3967; SEC-008, SEC-034, PRIV-021, CON-035).
//
// The spec seeds through psql on DATABASE_URL a fake homeowner contact (phone and street sealed with
// `seal` into privacy.contact, her number's blind index into privacy.contact_index, so the sign-in
// finds her). She signs in through the served pages with the code computed here from the Verify
// stand-in's rule (stand_ins/twilio.py `start_verification`: the sum of the E.164 number's character
// codes modulo 1,000,000, six digits), as TD-BIZ-42 does; her home shows her own record, she turns
// texts off, the page confirms it on screen and her number is on the suppression list. Every answer
// is `no-store`. The sign-in and home pages' axe checks are their `PAGE_STATES` entries in
// tests/a11y/page_states.ts. T561's case: she downloads her data file, then opens a delete request
// and its code screen (the answer to a POST, so no PAGE_STATES entry, D-4822) is axe-clean at 320,
// 768 and 1280 pixels. Fake data only; no key or value is printed or logged.
import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { test, expect } from "./fixtures";
import { blindIndex, keyedUrl, psql, seal } from "./keyed";

/** Her fake numbers (E.164, as the vault stores them), one per test, and street. The keyed vault keys
 * are made per run, so a number's index digest never meets an earlier run's. */
const HER = { phone: "+19285550176", wrongCodePhone: "+19285550178", street: "76 Example Way" };

/** T561's fake number for the download case. */
const DOWNLOAD_PHONE = "+19285550179";

/** The code the Verify stand-in issues for `phone` (stand_ins/twilio.py `start_verification`). */
function verifyCode(phone: string): string {
  let sum = 0;
  for (let i = 0; i < phone.length; i += 1) sum += phone.charCodeAt(i);
  return String(sum % 1_000_000).padStart(6, "0");
}

/** `value` as a SQL bytea literal. */
const bytea = (value: Buffer): string => `decode('${value.toString("hex")}', 'hex')`;

/** Her contact on `number`, sealed, and the number's blind index. */
function seedHer(number: string): string {
  const contact = randomUUID();
  const digest = bytea(blindIndex("homeowner-phone", number));
  const phone = bytea(seal("privacy.contact", "phone", contact, number));
  const street = bytea(seal("privacy.contact", "address", contact, HER.street));
  psql(`WITH c AS (INSERT INTO privacy.contact (id, phone, address) VALUES ('${contact}', ${phone}, ${street}) RETURNING id)
    INSERT INTO privacy.contact_index (purpose, key_version, digest, contact_id)
    SELECT 'homeowner-phone', 1, ${digest}, id FROM c`);
  return contact;
}

/** Whether her sign-in challenge is open: the route sends the code after it answers (D-4013). */
const challenged = (contact: string): boolean =>
  psql(`SELECT count(*) FROM portals.code_challenge WHERE subject_id = '${contact}'`) !== "0";

/** Whether her number is on the suppression list. */
function listed(): boolean {
  const digest = bytea(blindIndex("homeowner-phone", HER.phone));
  return psql(`SELECT count(*) FROM messaging.suppression WHERE index_version = 1 AND digest = ${digest}`) !== "0";
}

test("TD-PRIV-21: she signs in with the texted code, sees her own record and turns texts off", async ({ page }) => {
  const contact = seedHer(HER.phone);
  const answers: string[] = [];
  page.on("response", (response) => {
    if (new URL(response.url()).pathname.startsWith("/h")) answers.push(response.headers()["cache-control"] ?? "");
  });
  await page.goto(`${keyedUrl()}/h/sign-in`);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Sign in");
  await page.getByLabel("Your mobile number").fill(HER.phone);
  await page.getByRole("button", { name: "Text me a code" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Enter your code");
  await expect.poll(() => challenged(contact), { message: "her code was sent" }).toBe(true);
  await page.getByLabel("6-digit code").fill(verifyCode(HER.phone));
  await page.getByRole("button", { name: "Sign in" }).click();

  await expect(page).toHaveURL(`${keyedUrl()}/h`);
  const main = page.locator("main");
  await expect(main).toContainText("Mobile number ending in 0176");
  await expect(main).toContainText(HER.street);
  expect(listed(), "texts on before the switch").toBe(false);
  await page.getByRole("button", { name: "Turn texts off" }).click();
  await expect(main).toContainText("Texts are off.");
  await expect(page.getByRole("button", { name: "Turn texts off" })).toHaveCount(0);
  expect(listed(), "her number is on the suppression list at once (CON-035)").toBe(true);
  expect(answers.length).toBeGreaterThan(0);
  for (const cache of answers) expect(cache).toContain("no-store");
});

test("TD-PRIV-21: a wrong code does not sign her in", async ({ page }) => {
  seedHer(HER.wrongCodePhone);
  await page.goto(`${keyedUrl()}/h/sign-in`);
  await page.getByLabel("Your mobile number").fill(HER.wrongCodePhone);
  await page.getByRole("button", { name: "Text me a code" }).click();
  const wrong = String((Number(verifyCode(HER.wrongCodePhone)) + 1) % 1_000_000).padStart(6, "0");
  await page.getByLabel("6-digit code").fill(wrong);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("alert")).toContainText("That code didn't work");
  const home = await page.request.get(`${keyedUrl()}/h`, { maxRedirects: 0 });
  expect(home.status()).toBe(401);
  expect(await home.text()).not.toContain(HER.street);
});

test("TD-PRIV-21: she downloads her data file and the request screen is axe-clean", async ({ page }) => {
  const contact = seedHer(DOWNLOAD_PHONE);
  await page.goto(`${keyedUrl()}/h/sign-in`);
  await page.getByLabel("Your mobile number").fill(DOWNLOAD_PHONE);
  await page.getByRole("button", { name: "Text me a code" }).click();
  await expect.poll(() => challenged(contact), { message: "her code was sent" }).toBe(true);
  await page.getByLabel("6-digit code").fill(verifyCode(DOWNLOAD_PHONE));
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(`${keyedUrl()}/h`);

  const downloading = page.waitForEvent("download");
  await page.getByRole("link", { name: "Download my data" }).click();
  const download = await downloading;
  expect(download.suggestedFilename()).toBe("my-data.json");
  const file = JSON.parse(readFileSync(await download.path(), "utf8")) as Record<string, unknown>;
  expect(Object.keys(file).sort()).toEqual(["consent", "messages", "quotes", "requests"]);
  expect(JSON.stringify(file)).not.toContain(HER.street);

  await page.getByRole("button", { name: "Delete my data" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Check it's you");
  await expect(page.getByRole("button", { name: "Text me the code again" })).toBeVisible();
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 900 });
    const { violations } = await axeScan(page);
    expect(violations, `request screen at ${String(width)} px`).toEqual([]);
  }
});
