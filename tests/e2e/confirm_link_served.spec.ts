/// <reference types="node" />
// TD-SEC-68: the Confirm email link served, in a browser, on T452's keyed web binary (T457,
// D-3675, D-3844, D-3889; SEC-004, SEC-027, BIZ-038, PICK-021).
//
// `seedPick` (keyed.ts) seeds a pick waiting for Confirm; this spec links a fake texting phone
// to its owner (sealed as the vault stores it) and seeds a Confirm link for the owner: a
// portals.login_link row holding the SHA-256 of `action:confirm:<pick>:<token>` (D-1955). A
// browser with no session opens the link, presses its button, gets the code step and uses
// nothing; a wrong code uses nothing; the Verify stand-in's code (computed as
// action_links_served.spec.ts does) uses the link, signs the browser in and lands on the Confirm
// page's review step. Stripe.js is answered by keyed.ts's fake (`serve`), so no third-party
// request is made (D-3844). Fake data only; no token is printed.
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { test, expect } from "./fixtures";
import { confirmPath, keyedUrl, psql, seal, seedPick, serve, signIn } from "./keyed";

/** The owner's fake texting phone, as the vault stores it (`+1` and ten digits). */
const OWNER_PHONE = "+19285550147";

/** The code the Verify stand-in issues for `phone` (stand_ins/twilio.py `start_verification`). */
function verifyCode(phone: string): string {
  let sum = 0;
  for (let i = 0; i < phone.length; i += 1) sum += phone.charCodeAt(i);
  return String(sum % 1_000_000).padStart(6, "0");
}

/** A fake texting phone linked by `business`'s owner, and a live Confirm link on `pick` for that owner. */
function seedConfirmLink(business: string, pick: string): { path: string; hash: string } {
  const line = randomUUID();
  const token = randomBytes(32).toString("hex");
  const hash = createHash("sha256").update(`action:confirm:${pick}:${token}`).digest("hex");
  psql(`WITH m AS (SELECT id, business_id FROM businesses.team_member WHERE business_id = '${business}' AND is_owner),
    p AS (INSERT INTO businesses.staff_phone (id, business_id, linked_by, linked_at)
      SELECT '${line}', business_id, id, now() FROM m RETURNING id),
    c AS (INSERT INTO businesses.esign_consent (business_id, member_id, channel, wording_version,
      consented_at, line_id) SELECT m.business_id, m.id, 'text', 'con019-v1', now(), p.id FROM m, p),
    v AS (INSERT INTO privacy.staff_phone (line_id, number)
      VALUES ('${line}', decode('${seal("privacy.staff_phone", "number", line, OWNER_PHONE).toString("hex")}', 'hex')))
    INSERT INTO portals.login_link (token_sha256, member_id, business_id, created_at, expires_at)
    SELECT decode('${hash}', 'hex'), id, business_id, now() - interval '1 day', now() + interval '2 hours' FROM m`);
  return { path: `/b/a/confirm/${pick}/${token}`, hash };
}

/** Whether the link whose stored hash is `hash` is still unused. */
function unused(hash: string): boolean {
  return psql(`SELECT used_at IS NULL FROM portals.login_link WHERE token_sha256 = decode('${hash}', 'hex')`) === "t";
}

test("TD-SEC-68: the Confirm link with no session asks for a code, then signs the browser in and opens the Confirm review step", async ({ page }) => {
  const seeded = seedPick();
  const link = seedConfirmLink(seeded.business, seeded.pick);
  await serve(page, "pm_card_visa");
  await signIn(page, null);

  const signedOut = await page.goto(keyedUrl() + confirmPath(seeded.pick));
  expect(signedOut?.status(), "the Confirm page answers no one signed out").toBe(401);

  await page.goto(keyedUrl() + link.path);
  await page.getByRole("button", { name: /^Confirm/ }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Type your code to see the job");
  await expect(page.locator("#code-sent")).toHaveText("We texted a code to your phone. Type it below.");
  expect(unused(link.hash), "sending the code uses nothing (SEC-026)").toBe(true);

  await page.getByLabel("6-digit code").fill(verifyCode(OWNER_PHONE) === "000000" ? "000001" : "000000");
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page).toHaveTitle(/^Error: /);
  await expect(page.locator("#error-summary")).toBeFocused();
  await expect(page.locator("#code-error")).toHaveText(/^Error: /);
  expect(unused(link.hash), "a wrong code uses nothing").toBe(true);

  await page.getByLabel("6-digit code").fill(verifyCode(OWNER_PHONE));
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page).toHaveURL(keyedUrl() + confirmPath(seeded.pick));
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Confirm job \d+$/);
  await expect(page.getByRole("heading", { level: 2, name: "Before you confirm" })).toBeVisible();
  await expect(page.locator("#fee-line")).toContainText("Introduction fee by job size: $25.");
  expect(unused(link.hash), "the right code used the link").toBe(false);
});
