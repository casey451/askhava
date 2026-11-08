/// <reference types="node" />
// Journey J1 keyboard-only, with axe at three widths (T098; A11Y-005, A11Y-021; docs/test-design.md
// sections 5 and 6; D-2029 to D-2031).
//
// The web steps of J1's stage-4 slice, driven with Tab, Shift+Tab, Enter and Space only: the quote
// page with two diagnostic-visit quotes (step 5), Pick #2, the review step, "Go back", Pick #2
// again, the final Pick, and the code-and-contact form (step 6). The suite's web binary has no page
// pick (no vault keys in `ci`, D-1516), so its final Pick shows "Nothing was picked", and the code
// step is reached by switching the review form's address to /code, as pick_forms.spec.ts does; the
// real code check runs in tests/e2e/j1_homeowner.spec.ts. At every stop the focused element must be
// the next control in source order (after a "#quote-2" address, the first one from that card), show
// a focus indicator, lie inside the viewport and not under any other element (A11Y-005); a radio
// group is one stop (its checked radio, else its first), as browsers tab; tabbing
// past the last control leaves the page and Shift+Tab comes back, so nothing traps focus. The 3 px
// `focus` token ring is not checked yet: no stylesheet defines it until T101 and T166 (D-2031).
// axe runs on every page at 320, 768 and 1280 px, with focus already on the control about to be
// used, since a scan moves the browser's Tab starting point. The quote page is seeded through
// fixtures.ts's psql seeding. Fake data only.
import type { Page } from "@playwright/test";
import { TARGETS, WIDTHS, axeScan, expect, open, test, type PageState } from "./fixtures";

/** A job in Quotes ready with two diagnostic-visit quotes ($89 visit each), J1 step 5. */
const TWO_VISITS: PageState = {
  route: "/q/{token}",
  name: "J1 two diagnostic-visit quotes",
  statusRegions: ["quote-status"],
  shows: "Pick #2",
  hides: "No quotes yet.",
  sql: `WITH j AS (INSERT INTO jobs.job (status, emergency, created_at, status_at, timer_from)
      VALUES ('quotes_ready', false, now(), now(), now()) RETURNING id),
    n AS (UPDATE jobs.job_number_next SET next = next + 1 RETURNING next - 1 AS n),
    num AS (INSERT INTO jobs.job_number (number, job_id, issued_at) SELECT n.n, j.id, now() FROM n, j),
    t AS (INSERT INTO jobs.quote_token (job_id, salt, key_version, token_hash, issued_at)
      SELECT id, decode(md5(random()::text), 'hex'), 1, decode(':hash', 'hex'), now() FROM j)
    INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, notes,
      original, ai_read, confirmed, received_at)
    SELECT j.id, gen_random_uuid(), 'diagnostic_visit', 8900, false, '',
      'Diagnostic visit, sample wording ' || g.n, false, true, now() + g.n * interval '1 minute'
    FROM j, generate_series(1, 2) AS g(n)`,
};

/** Re-sends every POST with a same-origin https Origin (D-1250). */
async function httpsOrigin(page: Page): Promise<void> {
  await page.route("**/q/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const headers = { ...request.headers(), origin: `https://${new URL(request.url()).host}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

interface Focus {
  index: number;
  next: number;
  name: string;
  problems: string[];
}

/** Where focus is: its place among the tabbable controls, the place Tab must move it to, and any A11Y-005 problem. Runs in the page. */
function focusReport(targets: string): Focus {
  const groupStop = (t: HTMLElement): boolean => {
    if (!(t instanceof HTMLInputElement) || t.type !== "radio" || t.name === "") return true;
    const group = [...document.querySelectorAll<HTMLInputElement>(`input[type="radio"][name="${t.name}"]`)];
    return t.checked || (!group.some((r) => r.checked) && group[0] === t);
  };
  const tabbable = [...document.querySelectorAll<HTMLElement>(targets)].filter(
    (t) => t.tabIndex >= 0 && !t.hasAttribute("disabled") && t.getClientRects().length > 0 && groupStop(t),
  );
  const el = document.activeElement as HTMLElement | null;
  if (el === null || el === document.body) {
    const target = location.hash === "" ? null : document.getElementById(location.hash.slice(1));
    const reaches = Node.DOCUMENT_POSITION_FOLLOWING | Node.DOCUMENT_POSITION_CONTAINED_BY;
    const from = target === null ? 0 : tabbable.findIndex((t) => t === target || (target.compareDocumentPosition(t) & reaches) !== 0);
    return { index: -1, next: tabbable.length > 0 ? from : -1, name: "", problems: [] };
  }
  const index = tabbable.indexOf(el);
  const next = tabbable.findIndex((t) => (el.compareDocumentPosition(t) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 && !el.contains(t));
  const name = (el.getAttribute("aria-label") ?? el.textContent).trim() || el.id;
  const problems: string[] = [];
  const style = getComputedStyle(el);
  if (style.outlineStyle === "none" && style.boxShadow === "none") problems.push(`${name}: no focus indicator`);
  const box = el.getBoundingClientRect();
  if (box.top < 0 || box.left < 0 || box.bottom > innerHeight || box.right > innerWidth) problems.push(`${name}: outside the viewport`);
  const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  const ownLabel = el instanceof HTMLInputElement && [...(el.labels ?? [])].some((label) => hit !== null && label.contains(hit));
  if (hit !== null && hit !== el && !el.contains(hit) && !ownLabel) problems.push(`${name}: covered by <${hit.tagName}>`);
  return { index, next, name, problems };
}

async function where(page: Page): Promise<Focus> {
  return page.evaluate(focusReport, TARGETS);
}

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
  for (let stop = 0; stop < 40; stop += 1) {
    if (name.test((await tab(page)).name)) return;
  }
  throw new Error(`no control named ${String(name)} within 40 Tab presses`);
}

/** Tabs through every control and off the page; Shift+Tab then comes back to the last one, and once more to the one before (no trap). */
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
  await page.keyboard.press("Shift+Tab");
  const back = await where(page);
  expect(back.index, "Shift+Tab goes back one more").toBe(last - 1);
  expect(back.problems).toEqual([]);
}

/** axe on the page as it stands, run with focus already on the control about to be used; focus must stay there. */
async function axeClean(page: Page, step: string): Promise<void> {
  const before = await where(page);
  const { violations } = await axeScan(page);
  expect(violations, `axe on ${step}`).toEqual([]);
  expect((await where(page)).index, "the scan left focus where it was").toBe(before.index);
}

/** From the review step's "Nothing was picked" answer to the code step, its form's address switched to /code (D-1264). */
async function toCodeStep(page: Page): Promise<void> {
  await page.locator("#final-pick").evaluate((button) => {
    const form = button.closest("form");
    if (form === null) throw new Error("no final form");
    form.action = form.action.replace(/\/pick$/, "/code");
  });
  await tabTo(page, /^Pick #2$/);
  await axeClean(page, "the final Pick's answer");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Finish your pick for request \d+$/);
}

for (const width of WIDTHS) {
  test(`A11Y-005 A11Y-021 J1 steps 5 and 6 keyboard-only at ${String(width)} px: quote page, Pick #2, Go back, Pick #2 again, final Pick, code step; axe on each`, async ({ page, pageUrl }) => {
    await httpsOrigin(page);
    await open(page, pageUrl(TWO_VISITS), width);
    await noTrap(page);
    await open(page, pageUrl(TWO_VISITS), width);
    await tabTo(page, /^Pick #2$/);
    await axeClean(page, "the quote page");
    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Check your pick: quote #2");
    await tabTo(page, /^Go back$/);
    await axeClean(page, "the review step");
    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Your quotes for request \d+$/);
    await tabTo(page, /^Pick #2$/);
    await page.keyboard.press("Space");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Check your pick: quote #2");
    await tabTo(page, /^Pick #2$/);
    await page.keyboard.press("Enter");
    await expect(page.locator("#pick-not-open")).toBeFocused();
    await toCodeStep(page);
    await tabTo(page, /^reach-call$/);
    await page.keyboard.press("Space");
    await expect(page.getByLabel(/^A phone call/)).toBeChecked();
    await tabTo(page, /^code$/);
    await page.keyboard.type("123456");
    await axeClean(page, "the code step");
    await page.keyboard.press("Enter");
    await expect(page.locator("#pick-not-open")).toBeFocused();
    await axeClean(page, "the code step's answer");
  });
}
