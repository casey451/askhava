/// <reference types="node" />
// The served owner refund review, audit log and health page in a browser (T304; TD-SEC-47's
// browser half, J3's refund and audit-log steps; SEC-006, SEC-034, SEC-043, FEE-026, FEE-050,
// OPS-008, OPS-010; D-2128, D-2204, D-3844).
//
// On the keyed web binary, the only one with Stripe settings: two fake picks are paid through the
// Confirm page against the Stripe stand-in (keyed.ts), one of them then disputed through psql.
// The owner signs in there only through the shared tests/e2e/owner_session.ts helper and steps
// up with a fresh code, refunds the first charge on its review step (one ledger row), reads the
// audit log row for it, sees the disputed charge's refund blocked, and presses "I submitted this
// dispute's evidence in Stripe myself": the countering fee is booked once and a replayed press
// changes nothing. The wrong-session, CSRF and stale-factor refusals are
// app/server/tests/owner_tools_route.rs; the axe checks are the Ally suite's
// (tests/a11y/page_states.ts). Fake data only.
import { randomBytes } from "node:crypto";
import { test, expect } from "./fixtures";
import { confirmToWaiting, keyedUrl, psql, seedPick, signIn } from "./keyed";
import { ownerCode, signInOwner } from "./owner_session";

// Two paid picks, then up to two TOTP steps of waiting, plus the pages (D-3878).
test.setTimeout(240_000);

/** How many ledger rows of `kind` the business has. */
const ledger = (business: string, kind: string): string =>
  psql(`SELECT count(*) FROM payments.ledger_entry WHERE business_id = '${business}' AND kind = '${kind}'`);

test("TD-SEC-47: the owner refunds a charge once, reads its audit row and marks a dispute's evidence as sent", async ({ page, context }) => {
  const refunded = seedPick();
  await confirmToWaiting(page, refunded);
  const disputed = seedPick();
  await confirmToWaiting(await context.newPage(), disputed);
  const dispute = `dp_FakeT304${randomBytes(6).toString("hex")}`;
  psql(`INSERT INTO payments.dispute (stripe_dispute_id, pick_id, amount_cents, due_at, opened_at)
    VALUES ('${dispute}', '${disputed.pick}', 2500, now() + interval '10 days', now())`);
  const before = psql("SELECT coalesce(max(id), 0) FROM ops.audit_log");

  await signIn(page, null);
  await signInOwner(page, keyedUrl());
  await page.getByLabel("Code", { exact: true }).fill(await ownerCode());
  await page.getByRole("button", { name: "Check code" }).click();
  await expect(page.locator("#step-up-done")).toHaveText(/That worked/);

  const step = await page.goto(`${keyedUrl()}/owner/refunds/${refunded.pick}`);
  expect(step?.status()).toBe(200);
  expect(step?.headers()["cache-control"]).toContain("no-store");
  await page.getByRole("button", { name: "Refund this charge" }).click();
  await expect(page.getByRole("status")).toHaveText("Refunded in full.");
  await expect(page.getByRole("button", { name: "Refund this charge" })).toHaveCount(0);
  expect(ledger(refunded.business, "refund")).toBe("1");

  await page.goto(`${keyedUrl()}/owner/audit-log?after=${before}`);
  const row = page.getByRole("region", { name: "Audit log rows" }).getByRole("row").filter({ hasText: "portals.owner_refund" });
  await expect(row).toHaveCount(1);
  await expect(row).toContainText(`picks.pick ${refunded.pick}`);
  await expect(row).toContainText("Owner");

  await page.goto(`${keyedUrl()}/owner/refunds/${disputed.pick}`);
  await expect(page.getByRole("main")).toContainText("This charge is disputed, so it can't be refunded.");
  await expect(page.getByRole("button", { name: "Refund this charge" })).toHaveCount(0);
  const csrf = await page.locator("#disputes input[name=csrf]").inputValue();
  await page.getByRole("button", { name: "I submitted this dispute's evidence in Stripe myself" }).click();
  await expect(page.getByRole("status")).toHaveText("Marked as sent by you.");
  const again = await page.evaluate(async ([token, id]) => {
    const body = new URLSearchParams({ csrf: token ?? "", dispute: id ?? "" });
    const reply = await fetch(location.pathname, { method: "POST", body });
    return { status: reply.status, text: await reply.text() };
  }, [csrf, dispute]);
  expect(again.status).toBe(200);
  expect(again.text).toContain("That evidence is already in.");
  expect(ledger(disputed.business, "countering-fee")).toBe("1");
  expect(ledger(disputed.business, "refund")).toBe("0");

  const health = await page.goto(`${keyedUrl()}/owner/health`);
  expect(health?.status()).toBe(200);
  const secrets = page.getByRole("region", { name: "Required secrets" });
  await expect(secrets.getByRole("row").filter({ hasText: "ASKHAVA_STRIPE_SECRET_KEY" })).toContainText("Yes");
  expect(await page.content()).not.toMatch(/sk_test_|whsec_/);
});
