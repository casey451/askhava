/// <reference types="node" />
// Journey J2 (business), its stage-4 slice, with its closing checks (T099; docs/test-design.md
// section 5; TD-BIZ-22, TD-BIZ-25; D-1739, D-3144, D-3862, D-3863).
//
// On the keyed web binary (tests/e2e/keyed.ts, T452) a fake R-37 plumber signs up on the fixture ROC
// record through the served sign-up (tests/e2e/j2.ts), then gets a picked job waiting for Confirm
// (keyed.ts `seedPick` with the business: the homeowner's sealed contact, her `intake.homeowner` and
// an open conversation, which the release needs) and confirms it on the served Confirm route,
// `GET` and `POST /b/picks/{pick}/confirm` (T261). The $75 test is FEE-001's tier rule on a sign-up
// joining after day 90 (FEE-010): the keyed launch date is 2026-01-15 and the plumbing category reads
// as switched on before it (D-3857), so a sign-up today has no free win. Its stand-in Stripe success
// comes from `GET /stripe/v1/events` and is posted to `/webhooks/stripe` signed with the run's secret
// (`stripeDelivery`, T515), never with the stand-in's own signature. The free-first-win test moves
// the sign-up's `submitted_at` to before 2026-04-15 (D-3863) and stores the card authorization the
// page shows (j2.ts `acceptAuthorization`, D-3857), so its Confirm is $0 with the $75 waived and
// releases at once, with no charge and no webhook (FEE-012). Each ends with the closing checks: the
// portal shows the released details, again after a reload; one release row, one business notice,
// one open masked line, no second fee; a second delivery changes nothing; the ledger matches the stand-in's charges to the cent; and the
// Twilio stand-in records no call to the business's phone (the J2 half of TD-BIZ-22's and
// TD-BIZ-25's sweeps: placed calls only, since the Verify stand-in does not record the channel).
// POSTs go with the browser's own headers (D-4292). Fake data only.
import { test, expect } from "./fixtures";
import { type Seeded, WAITING, keyedUrl, confirmPath, openAsOwner, psql, seedPick, serve, signIn } from "./keyed";
import { RECORD_PHONE, acceptAuthorization, callsTo, closing, deliverSuccess, joinedAt, plumbingOnBeforeLaunch, signUp, standInCharges } from "./j2";
import type { Page } from "@playwright/test";

/** The seeded homeowner's first name (keyed.ts `HOMEOWNER`), shown once released (PICK-024). */
const HER_FIRST_NAME = "Robin";
/** The press's answer on a $0 Confirm (app/portals/src/confirm_pay.rs `CONFIRMED_LINE`, then its portal link). */
const CONFIRMED = /^Confirmed\. No fee is due\. The homeowner's details are in your portal\. Go to your portal$/;
/** FEE-012: the fee a free first win waives on a $75 job. */
const MEDIUM_CENTS = 7500;

/** The released job's portal page shows her details, and again after a reload (PICK-024, D-1739). */
async function portalShowsRelease(page: Page, seeded: Seeded): Promise<void> {
  await page.goto(`${keyedUrl()}/b/picks/${seeded.pick}`);
  await expect(page.locator("main")).toContainText(HER_FIRST_NAME);
  await page.reload();
  await expect(page.locator("main")).toContainText(HER_FIRST_NAME);
}

test("J2 TD-BIZ-22 TD-BIZ-25 FEE-001 FEE-010 FEE-016 PICK-021 PICK-022 D-1739 D-3862: a sign-up after day 90 confirms a $75 pick, pays once, is released once by the signed stand-in success and sees the details in the portal; no call to the business", async ({ page, request }) => {
  plumbingOnBeforeLaunch();
  const business = await signUp(page);
  const seeded = seedPick({ business, feeCents: MEDIUM_CENTS });
  await openAsOwner(page, seeded, "pm_card_visa");
  await expect(page.locator("#fee-line"), "FEE-010: no free win after day 90").toHaveText(/^Introduction fee by job size: \$75\. /);
  await expect(page.getByRole("link", { name: "Go back" })).toBeVisible();
  await page.getByRole("button", { name: "Confirm and pay $75" }).click();
  await expect(page.locator("#confirm-status")).toHaveText(WAITING);
  expect(closing(business, seeded.pick, seeded.job).releases, "nothing released before Stripe confirms").toBe(0);

  expect(await deliverSuccess(request, seeded.pick), "the signed stand-in success").toBe(200);
  await expect.poll(() => closing(business, seeded.pick, seeded.job).releases).toBe(1);
  await portalShowsRelease(page, seeded);
  expect(await deliverSuccess(request, seeded.pick), "the same event again").toBe(200);
  expect(closing(business, seeded.pick, seeded.job)).toEqual({
    releases: 1, basis: "paid", notices: 1, lines: 1, charges: 1, ledger: `charge:${String(MEDIUM_CENTS)}`,
  });
  expect(await standInCharges(request, seeded.pick), "one charge at the stand-in, as in the ledger to the cent").toEqual([MEDIUM_CENTS]);
  expect(await callsTo(request, [RECORD_PHONE]), "TD-BIZ-22 TD-BIZ-25: no call to the business").toEqual([]);
});

test("J2 TD-BIZ-22 TD-BIZ-25 FEE-010 FEE-012 W24 PICK-022 D-1739 D-3863: a free first win confirms $0 with the $75 waived and is released at once, with no charge, no webhook and no call to the business", async ({ page, request }) => {
  plumbingOnBeforeLaunch();
  const business = await signUp(page);
  joinedAt(business, "2026-02-01T17:00:00Z");
  const seeded = seedPick({ business, feeCents: MEDIUM_CENTS });
  await serve(page, "pm_card_visa");
  await signIn(page, seeded.owner);
  expect((await page.goto(keyedUrl() + confirmPath(seeded.pick)))?.status()).toBe(200);
  await acceptAuthorization(page, business);
  await expect(page.locator("#fee-line")).toHaveText("No fee is due for this job.");
  await page.getByRole("button", { name: "Confirm", exact: true }).click();
  await expect(page.locator("#confirm-status")).toHaveText(CONFIRMED);

  await portalShowsRelease(page, seeded);
  expect(closing(business, seeded.pick, seeded.job)).toEqual({
    releases: 1, basis: "no_fee", notices: 1, lines: 1, charges: 0, ledger: "free-first-win:0",
  });
  expect(psql(`SELECT waived_cents FROM payments.free_win WHERE business_id = '${business}'`), "W24: the $75 waived").toBe(String(MEDIUM_CENTS));
  expect(await standInCharges(request, seeded.pick), "FEE-012: nothing charged").toEqual([]);
  expect(await callsTo(request, [RECORD_PHONE]), "TD-BIZ-22 TD-BIZ-25: no call to the business").toEqual([]);
});
