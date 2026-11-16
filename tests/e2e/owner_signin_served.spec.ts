/// <reference types="node" />
// The served owner sign-in, step-up and sign-out in a browser (T303; the browser half of TD-SEC-22
// and TD-SEC-46; SEC-005, SEC-034, SEC-043, SEC-044; D-2128, D-3744, D-3878).
//
// The one owner is made by the server binary's `bootstrap-owner` command through the shared
// tests/e2e/owner_session.ts helper (its fake token in the environment, its fake password on
// standard input); the TOTP codes are computed here from the authenticator secret it printed. The
// owner signs in with the password and a code (the TOTP path is the browser path; the passkey is
// checked in app/server/tests/owner_signin_route.rs), steps up with a fresh code, and signs out,
// after which the step-up page is the owner door's refusal. The wrong-password and the
// password-limit cases are the route test's, so this spec uses one password try of the hour's few
// per address (`OWNER_TRIES_PER_IP`), leaving the rest for the other owner specs. The pages'
// axe checks at 320, 768 and 1280 are the Ally suite's (tests/a11y/page_states.ts). Fake data only.
import { test, expect } from "./fixtures";
import { ownerCode, signInOwner } from "./owner_session";

// Two TOTP steps of waiting at most, plus the pages (D-3878).
test.setTimeout(150_000);

test("TD-SEC-22/TD-SEC-46: the bootstrapped owner signs in with a code, steps up and signs out", async ({ page }) => {
  await signInOwner(page);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Confirm it's you");
  await page.getByLabel("Code", { exact: true }).fill(await ownerCode());
  await page.getByRole("button", { name: "Check code" }).click();
  await expect(page.locator("#step-up-done")).toHaveText(/That worked/);
  await page.getByRole("button", { name: "Sign out", exact: true }).click();
  await expect(page).toHaveURL(/\/owner\/sign-in$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Owner sign-in");
  const after = await page.goto("/owner/step-up");
  expect(after?.status(), "the signed-out browser is refused").toBe(401);
  expect(after?.headers()["cache-control"]).toContain("no-store");
  await expect(page.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/owner/sign-in");
});

test("TD-SEC-46: the sign-in page is no-store and has no email path", async ({ page }) => {
  const reply = await page.goto("/owner/sign-in");
  expect(reply?.status()).toBe(200);
  expect(reply?.headers()["cache-control"]).toContain("no-store");
  await expect(page.getByLabel("Password")).toBeVisible();
  await expect(page.locator("input[type=email]")).toHaveCount(0);
});
