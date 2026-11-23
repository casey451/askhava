/// <reference types="node" />
// Journey J3 keyboard-only, with axe at three widths (T118; A11Y-005, A11Y-021; docs/test-design.md
// sections 5 and 6; D-3744, D-3844, D-3889, D-5282 to D-5285). tests/e2e/j3.ts's steps, each control
// reached with Tab and used with Enter, the arrow keys or typed text; every stop is the next control
// in source order, with a focus indicator, in the viewport and uncovered (A11Y-005). On each new page
// axe runs at 320, 768 and 1280 px and focus leaves and comes back with Shift+Tab (no trap). Sign-in
// is owner_session.ts's own (D-3744); the e2e `test` gets the control secret. Fake data only.
import type { Locator, Page } from "@playwright/test";
import { REVIEWED_INCOMPLETE, TARGETS, WIDTHS, axeScan } from "./fixtures";
import { expect, test } from "../e2e/fixtures";
import {
  before, begin, byText, closingChecks, restore, step1SignIn, step2Pause, step3Refund, step4FeeMode,
  step5SafeMode, step6AuditLog, webAndEmail, type Before, type Driver, type J3,
} from "../e2e/j3";
import { controlSecret } from "../e2e/webserver";

test.use({ controlSecret: controlSecret() });
test.setTimeout(1_500_000);

interface Focus { index: number; next: number; name: string; problems: string[] }

/** Where focus is: its place among the tabbable controls, where Tab must move it, and any A11Y-005 problem. Runs in the page. */
function focusReport(targets: string): Focus {
  // A radio group is one Tab stop: its checked button, so an unchecked one beside it is skipped.
  const groupStop = (t: HTMLElement): boolean =>
    !(t instanceof HTMLInputElement && t.type === "radio" && !t.checked && (t.form ?? document).querySelector(`input[type=radio][name="${t.name}"]:checked`) !== null);
  const tabbable = [...document.querySelectorAll<HTMLElement>(targets)].filter(
    (t) => t.tabIndex >= 0 && !t.hasAttribute("disabled") && t.getClientRects().length > 0 && groupStop(t),
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
  // A box counts as outside only past half a pixel (Chrome scrolls on fractional pixels).
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
  const was = await where(page);
  await page.keyboard.press("Tab");
  const at = await where(page);
  expect(at.index, `Tab from "${was.name}" reaches the next control in source order`).toBe(was.next);
  expect(at.problems).toEqual([]);
  return at;
}

/** Whether `target` (or, with `group`, any radio button of its name in its form) has focus. */
const focused = (target: Locator, group: boolean): Promise<boolean> =>
  target.evaluate((el, radios) => {
    const at = document.activeElement;
    return at === el || (radios && at instanceof HTMLInputElement && at.type === "radio" && at.name === (el as HTMLInputElement).name && at.form === (el as HTMLInputElement).form);
  }, group);

/** Tabs from where focus is until `target` (or its radio group) has focus, checking every stop. */
async function tabTo(target: Locator, group = false): Promise<void> {
  const page = target.page();
  for (let stop = 0; stop < 80; stop += 1) {
    if (await focused(target, group)) return;
    await tab(page);
  }
  throw new Error(`no focus on ${target.toString()} within 80 Tab presses`);
}

/** Tabs through every control and off the page; Shift+Tab then comes back to the last one (no trap). */
async function noTrap(page: Page): Promise<void> {
  let last = -1;
  for (let stop = 0; stop < 120; stop += 1) {
    const at = await tab(page);
    if (at.index === -1) break;
    last = at.index;
  }
  expect((await where(page)).index, "focus left the page after the last control").toBe(-1);
  await page.keyboard.press("Shift+Tab");
  expect((await where(page)).index, "Shift+Tab comes back to the last control").toBe(last);
}

/** The pages already checked with axe and for a trap, by route. */
const scanned = new Set<string>();

/** axe at three widths on the page's route once, then back to 1280 px; no violation, nothing unreviewed (TD-A11Y-01). */
async function axeOnce(page: Page): Promise<void> {
  const route = new URL(page.url()).pathname.replace(/[0-9a-f-]{36}/, "{pick}");
  if (scanned.has(route)) return;
  scanned.add(route);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 900 });
    const { violations, incomplete } = await axeScan(page);
    expect(violations, `axe on ${route} at ${String(width)} px`).toEqual([]);
    const ruleOf = (result: string): string => result.split(":")[0] ?? result;
    const unreviewed = incomplete.filter((result) => !REVIEWED_INCOMPLETE.includes(`${ruleOf(result)} on ${route}`));
    expect(unreviewed, `TD-A11Y-01: unreviewed axe incomplete results on ${route} at ${String(width)} px`).toEqual([]);
  }
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.evaluate(() => {
    (document.activeElement as HTMLElement | null)?.blur();
  });
  await noTrap(page);
  await page.keyboard.press("Tab");
}

/** The keyboard driver: Tab to the control, then Enter, the arrow keys or typed text (A11Y-005). */
const KEYBOARD: Driver = {
  async press(target) {
    await axeOnce(target.page());
    await tabTo(target);
    await target.page().keyboard.press("Enter");
  },
  async choose(target) {
    await axeOnce(target.page());
    await tabTo(target, true);
    for (let move = 0; move < 4 && !(await target.isChecked()); move += 1) await target.page().keyboard.press("ArrowDown");
    await expect(target).toBeChecked();
    await expect(target).toBeFocused();
  },
  async type(target, text) {
    await axeOnce(target.page());
    await tabTo(target);
    await target.page().keyboard.type(text);
  },
};

let was: Before | undefined;
let run: J3 | undefined;

test.afterAll(() => {
  if (was !== undefined) restore(was, run?.alert);
});

test("A11Y-005 A11Y-021 J3 keyboard-only: sign-in, pause, refund, fee mode, safe mode, audit log, closing checks; axe at three widths on each page", async ({ page, control }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  was = before();
  run = await begin(page, control, KEYBOARD);
  const j = run;
  await step1SignIn(j);
  await webAndEmail(j);
  await step2Pause(j);
  await step3Refund(j);
  await step4FeeMode(j);
  await step5SafeMode(j);
  await step6AuditLog(j);
  await closingChecks(j);
  await byText(j);
});
