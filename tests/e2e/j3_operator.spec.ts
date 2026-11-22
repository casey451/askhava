/// <reference types="node" />
// Journey J3, the operator (Casey), end to end with the mouse (T118; docs/test-design.md section 5
// J3 and its closing checks, TD-TXT-15; SEC-005, SEC-006, SEC-043, OPS-001, OPS-003, OPS-004,
// OPS-010, OPS-024, FEE-007, FEE-016, FEE-017, FEE-026, FEE-028, INT-003, CON-009,
// TXT-021; D-3744, D-3844, D-3889, D-5282 to D-5285).
// One test on the keyed binary: web-and-email on (TD-TXT-15), the six steps (tests/e2e/j3.ts), the
// closing checks, then By text with texting on; late project (D-3889); `afterAll` restores. Fake data only.
import { test } from "./fixtures";
import {
  MOUSE, before, begin, byText, closingChecks, restore, step1SignIn, step2Pause, step3Refund,
  step4FeeMode, step5SafeMode, step6AuditLog, webAndEmail, type Before, type J3,
} from "./j3";

// Two TOTP waits per step-up at most, a 12-hour session walk and the worker's safe-mode rounds.
test.setTimeout(900_000);

let was: Before | undefined;
let run: J3 | undefined;

test.afterAll(() => {
  if (was !== undefined) restore(was, run?.alert);
});

test("J3 TD-TXT-15: sign-in, pause, refund, fee mode, safe mode, audit log, with the closing checks, launch channel web-and-email", async ({ page, control }) => {
  was = before();
  run = await begin(page, control, MOUSE);
  const j = run;
  await test.step("1. owner sign-in with password and TOTP; a session over 12 hours is refused (SEC-005)", () => step1SignIn(j));
  await test.step("TD-TXT-15: launch channel web-and-email, homeowner texting off (TXT-021)", () => webAndEmail(j));
  await test.step("2. pause and unpause the plumbing trade switch as stored (OPS-003; D-5284)", () => step2Pause(j));
  await test.step("3. refund through the review step after a fresh TOTP (FEE-026, FEE-028, SEC-043)", () => step3Refund(j));
  await test.step("4. flat fee mode after a fresh TOTP; old authorization refused (FEE-007, SEC-043)", () => step4FeeMode(j));
  await test.step("5. High alert, safe mode, safety text, STOP, refused press, sign-in again, acknowledge, held release, one charge (OPS-001, OPS-024, INT-003, CON-009, FEE-016, FEE-017)", () => step5SafeMode(j));
  await test.step("6. the audit log viewer shows every action with its actor (SEC-006, OPS-010)", () => step6AuditLog(j));
  await test.step("closing checks (TD-PRIV-02, TD-TXT-15)", () => closingChecks(j));
  await test.step("TD-TXT-15: launch channel back to By text, texting on", () => byText(j));
});
