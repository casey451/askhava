/// <reference types="node" />
// Journey J2 keyboard-only, with axe at three widths (T099; A11Y-005, A11Y-021; docs/test-design.md
// sections 5 and 6).
//
// The web steps of J2's stage-4 slice on the keyed web binary (tests/e2e/keyed.ts), driven with Tab,
// Shift+Tab, Enter and Space and typed text only: the business sign-up form (its E-SIGN box ticked
// with Space), the identity code step, the service area and the emailed code, until "You're signed
// up" (codes computed as tests/e2e/j2.ts computes them); then a picked $75 job is seeded for the new
// business (keyed.ts `seedPick`, its category read as on before launch so no free win applies,
// D-3857) and its Confirm page is used: "Confirm and pay $75" pressed with Enter against the fake
// Stripe.js until the server says it is waiting for Stripe. At every stop the focused element must be
// the next control in source order, show a focus indicator, lie inside the viewport and not under
// any other element (A11Y-005); on the Confirm page, tabbing past the last control leaves the page
// and Shift+Tab comes back, so nothing traps focus. axe runs on every page at 320, 768 and 1280 px
// with focus on the control about to be used. Each width signs up once (the sign-up limit is five a
// day per address, `portals::limits::SIGNUPS_PER_IP`). Fake data only.
import type { Page } from "@playwright/test";
import { REVIEWED_INCOMPLETE, TARGETS, WIDTHS, axeScan, expect, test } from "./fixtures";
import { RECORD_PHONE, boundBusiness, emailedCode, plumbingOnBeforeLaunch, signUpEmail, standInCode, storeRocList } from "../e2e/j2";
import { WAITING, keyedUrl, openAsOwner, seedPick } from "../e2e/keyed";

interface Focus { index: number; next: number; name: string; problems: string[] }

/** Where focus is: its place among the tabbable controls, where Tab must move it, and any A11Y-005 problem. Runs in the page. */
function focusReport(targets: string): Focus {
  const tabbable = [...document.querySelectorAll<HTMLElement>(targets)].filter(
    (t) => t.tabIndex >= 0 && !t.hasAttribute("disabled") && t.getClientRects().length > 0,
  );
  const el = document.activeElement as HTMLElement | null;
  if (el === null || el === document.body) return { index: -1, next: tabbable.length > 0 ? 0 : -1, name: "", problems: [] };
  const index = tabbable.indexOf(el);
  const next = tabbable.findIndex((t) => (el.compareDocumentPosition(t) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 && !el.contains(t));
  const name = (el.getAttribute("aria-label") ?? el.textContent).trim() || el.id;
  const problems: string[] = [];
  const style = getComputedStyle(el);
  if (style.outlineStyle === "none" && style.boxShadow === "none") problems.push(`${name}: no focus indicator`);
  const box = el.getBoundingClientRect();
  // Chrome scrolls a focused control into view on fractional pixels (e.g. a bottom of 900.25 in a
  // 900 px viewport), so a box counts as outside only past half a pixel.
  const out = box.top < -0.5 || box.left < -0.5 || box.bottom > innerHeight + 0.5 || box.right > innerWidth + 0.5;
  if (out) problems.push(`${name}: outside the viewport`);
  const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  const ownLabel = el instanceof HTMLInputElement && [...(el.labels ?? [])].some((label) => hit !== null && label.contains(hit));
  if (hit !== null && hit !== el && !el.contains(hit) && !ownLabel) problems.push(`${name}: covered by <${hit.tagName}>`);
  return { index, next, name, problems };
}

const where = (page: Page): Promise<Focus> => page.evaluate(focusReport, TARGETS);

/** One Tab: focus moves to the next control in source order, visible and uncovered (A11Y-005). */
async function tab(page: Page): Promise<Focus> {
  const before = await where(page);
  await page.keyboard.press("Tab");
  const at = await where(page);
  expect(at.index, `Tab from "${before.name}" reaches the next control in source order`).toBe(before.next);
  expect(at.problems).toEqual([]);
  return at;
}

/** Tabs on until the control named `name` has focus, checking every stop. */
async function tabTo(page: Page, name: RegExp): Promise<void> {
  for (let stop = 0; stop < 60; stop += 1) {
    if (name.test((await tab(page)).name)) return;
  }
  throw new Error(`no control named ${String(name)} within 60 Tab presses`);
}

/** Tabs to the field `id` and types `text` into it. */
async function typeInto(page: Page, id: string, text: string): Promise<void> {
  await tabTo(page, new RegExp(`^${id}$`));
  await page.keyboard.type(text);
}

/** Tabs through every control and off the page; Shift+Tab then comes back to the last one (no trap). */
async function noTrap(page: Page): Promise<void> {
  let last = -1;
  for (let stop = 0; stop < 60; stop += 1) {
    const at = await tab(page);
    if (at.index === -1) break;
    last = at.index;
  }
  expect((await where(page)).index, "focus left the page after the last control").toBe(-1);
  await page.keyboard.press("Shift+Tab");
  expect((await where(page)).index, "Shift+Tab comes back to the last control").toBe(last);
}

/**
 * axe on the page at 320, 768 and 1280 px, then back to `width`, with focus already on the control
 * about to be used; no violation and no unreviewed "incomplete" result (TD-A11Y-01, A11Y-025).
 */
async function axeClean(page: Page, step: string, width: number): Promise<void> {
  const before = await where(page);
  const route = new URL(page.url()).pathname.replace(/[0-9a-f-]{36}/, "{pick}");
  for (const scanned of WIDTHS) {
    await page.setViewportSize({ width: scanned, height: 900 });
    const { violations, incomplete } = await axeScan(page);
    expect(violations, `axe on ${step} at ${String(scanned)} px`).toEqual([]);
    const ruleOf = (result: string): string => result.split(":")[0] ?? result;
    const unreviewed = incomplete.filter((result) => !REVIEWED_INCOMPLETE.includes(`${ruleOf(result)} on ${route}`));
    expect(unreviewed, `TD-A11Y-01: unreviewed axe incomplete results on ${step} at ${String(scanned)} px`).toEqual([]);
  }
  await page.setViewportSize({ width, height: 900 });
  expect((await where(page)).index, "the scan left focus where it was").toBe(before.index);
}

/** Presses Enter on the focused button and waits for the next step's heading. */
async function next(page: Page, heading: string): Promise<void> {
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(heading);
}

/** J2 steps 1 and 2 by keyboard: the form, the identity code, the service area, the emailed code. */
async function signUpByKeyboard(page: Page, width: number): Promise<string> {
  storeRocList();
  await page.setViewportSize({ width, height: 900 });
  await page.goto(`${keyedUrl()}/b/signup`);
  await typeInto(page, "business-name", "Example Plumbing Co");
  await typeInto(page, "roc-number", "ROC100001");
  await typeInto(page, "email", signUpEmail());
  await tabTo(page, /^cat-plumbing_repair$/);
  await page.keyboard.press("Space");
  await tabTo(page, /^consent$/);
  await page.keyboard.press("Space");
  await tabTo(page, /^Next: get your code$/);
  await axeClean(page, "the sign-up form", width);
  await next(page, "Where your code goes");
  await tabTo(page, /^Send the code$/);
  await axeClean(page, "the code channel step", width);
  await next(page, "Show this business is yours");
  await typeInto(page, "code", standInCode(RECORD_PHONE));
  await tabTo(page, /^Next$/);
  await axeClean(page, "the identity code step", width);
  await next(page, "How far do you travel for jobs?");
  await typeInto(page, "base-zip", "86403");
  await typeInto(page, "radius", "25");
  await tabTo(page, /^Next$/);
  await axeClean(page, "the service area step", width);
  await next(page, "Check your email for a code");
  const business = await boundBusiness(page);
  await typeInto(page, "code", emailedCode(business));
  await tabTo(page, /^Next$/);
  await axeClean(page, "the email code step", width);
  await next(page, "You're signed up");
  await axeClean(page, "the done screen", width);
  return business;
}

for (const width of WIDTHS) {
  test(`A11Y-005 A11Y-021 J2 keyboard-only at ${String(width)} px: sign-up, identity code, service area, email code, then Confirm and pay $75; axe at three widths on each`, async ({ page }) => {
    plumbingOnBeforeLaunch();
    const business = await signUpByKeyboard(page, width);
    const seeded = seedPick({ business, feeCents: 7500 });
    await openAsOwner(page, seeded, "pm_card_visa");
    await noTrap(page);
    await page.keyboard.press("Tab");
    await tabTo(page, /^Confirm and pay \$75$/);
    await axeClean(page, "the Confirm page", width);
    await page.keyboard.press("Enter");
    await expect(page.locator("#confirm-status")).toHaveText(WAITING);
    await axeClean(page, "the Confirm page's waiting answer", width);
  });
}
