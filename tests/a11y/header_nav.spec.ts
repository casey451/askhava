// The shared header of app/site/templates/base.html on every page state (T614; WEB-027, WEB-030,
// A11Y-003, A11Y-004, A11Y-017, A11Y-026, BIZ-047; DESIGN_SPEC section 6, D-4252, D-4970, D-5059).
//
// Fails when: the header's link list (leaving out the business pages' `[data-get-help]` link)
// differs between two states; on a `lang="es"` page the skip link or a header link other than
// "Español" resolves to a language other than `en`, or "Español" to one other than `es`; a
// `[data-get-help]` link is not visible at 390 px; a preloaded font is not loaded by an
// `@font-face` the state uses; after the Menu is opened from the keyboard at 390 by 844, one of its
// four links or the ten stops after them is fully covered, or the page scrolls sideways (WCAG
// 2.4.11); below 1024 px the Menu pill or an open Menu link is not visible; at 320 and 390 px the
// closed header is taller than `--header-h` plus its border, opening the Menu moves the row, or the
// open list starts above the pill's bottom; from 1024 px a `.site-nav` link or "Español" is not
// visible. Fake data only.
import type { Page } from "@playwright/test";
import { STATES_TO_CHECK, expect, open, test } from "./fixtures";
import type { PageState } from "./page_states";

/** The pages whose open Menu is tabbed through (the home page, a form page, a story page, a long legal page). */
const FOCUS_ROUTES = ["/", "/help", "/how-it-works", "/terms"];
/** Tab presses after the Menu opens: its four links, then ten stops into `main`. */
const STOPS_AFTER_OPEN = 14;
const MENU_LINKS = ["How it works", "What we cover", "For businesses", "Help"];

/** The first seeded state of `route`. */
function stateOf(route: string): PageState {
  const state = STATES_TO_CHECK.find((s) => s.route === route);
  if (state === undefined) throw new Error(`no page state for ${route}`);
  return state;
}

/** The header's link list and its language, "Get help" and font-preload problems. Runs in the page. */
async function headerReport(): Promise<{ links: string; problems: string[] }> {
  const problems: string[] = [];
  const links = [...document.querySelectorAll("body > header a[href]:not([data-get-help])")];
  if (document.documentElement.lang === "es") {
    const skip = document.querySelector(".skip-link");
    if (skip === null) problems.push("no skip link");
    for (const el of skip === null ? links : [skip, ...links]) {
      const name = el.textContent.trim();
      const lang = el.closest("[lang]")?.getAttribute("lang") ?? "";
      if (lang !== (name === "Español" ? "es" : "en")) problems.push(`"${name}" resolves to lang="${lang}"`);
    }
  }
  const getHelp = document.querySelector("[data-get-help]")?.getBoundingClientRect();
  if (getHelp !== undefined && (getHelp.width === 0 || getHelp.left < 0 || getHelp.right > innerWidth)) {
    problems.push("the Get help link is not visible");
  }
  await document.fonts.ready;
  const loaded: string[] = [];
  const key = (family: string, weight: string, style: string): string => `${family.replace(/["']/g, "")} ${weight} ${style}`;
  document.fonts.forEach((face) => {
    if (face.status === "loaded") loaded.push(key(face.family, face.weight, face.style));
  });
  const faces = [...document.styleSheets].flatMap((sheet) => [...sheet.cssRules]).filter((rule) => rule instanceof CSSFontFaceRule);
  for (const preload of document.querySelectorAll<HTMLLinkElement>('link[rel="preload"][as="font"]')) {
    const file = preload.href.split("/").pop() ?? "";
    const used = faces
      .filter((face) => face.style.getPropertyValue("src").includes(file))
      .some((face) => loaded.includes(key(face.style.getPropertyValue("font-family"), face.style.getPropertyValue("font-weight"), face.style.getPropertyValue("font-style"))));
    if (!used) problems.push(`the preloaded ${file} is used by no @font-face of this page`);
  }
  return { links: links.map((a) => `${a.textContent.trim()} ${a.getAttribute("href") ?? ""}`).join(", "), problems };
}

/** WCAG 2.4.11: the focused element is fully under another element, or the page scrolls sideways. Runs in the page. */
function focusProblem(): string | null {
  if (document.documentElement.scrollWidth > innerWidth) return "the page scrolls sideways";
  const el = document.activeElement;
  if (!(el instanceof HTMLElement) || el === document.body) return null;
  const seen = [...el.getClientRects()].some((box) => {
    const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    const ownLabel = el instanceof HTMLInputElement && [...(el.labels ?? [])].some((label) => label.contains(hit));
    return hit === null || hit === el || el.contains(hit) || ownLabel;
  });
  return seen ? null : `<${el.tagName}> "${el.textContent.trim()}" is fully covered`;
}

interface Row { tops: number[]; height: number; limit: number; pillBottom: number; listTop: number; hidden: string[]; sideways: boolean }

/** Where the header's row sits and which of `selector`'s elements are not visible. Runs in the page. */
function rowReport(selector: string): Row {
  const box = (at: string): DOMRect => document.querySelector(at)?.getBoundingClientRect() ?? new DOMRect();
  const hidden = [...document.querySelectorAll<HTMLElement>(selector)].filter((el) => {
    const r = el.getBoundingClientRect();
    return r.width === 0 || r.height === 0 || r.left < 0 || r.right > innerWidth || getComputedStyle(el).visibility !== "visible";
  });
  return {
    tops: [box(".wordmark").top, box('body > header a[href="/es"]').top, box(".menu > summary").top],
    height: box("body > header").height,
    limit: parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--header-h")) + 1,
    pillBottom: box(".menu > summary").bottom,
    listTop: box(".menu-list").top,
    hidden: hidden.map((el) => el.textContent.trim()),
    sideways: document.documentElement.scrollWidth > innerWidth,
  };
}

const PHONE_CONTROLS = "body > header .menu > summary, body > header .menu-list a";

/** Tabs to the Menu pill and opens it with Enter. */
async function openMenuFromKeyboard(page: Page): Promise<void> {
  const onPill = (): boolean => document.activeElement?.matches("details.menu > summary") ?? false;
  for (let stop = 0; stop < 10 && !(await page.evaluate(onPill)); stop += 1) await page.keyboard.press("Tab");
  expect(await page.evaluate(onPill), "Tab reaches the Menu pill").toBe(true);
  await page.keyboard.press("Enter");
  await expect(page.locator("details.menu")).toHaveAttribute("open", "");
}

test("A11Y-017 A11Y-004 WEB-030 BIZ-047: one header link list, English tagged on Spanish pages, Get help shown, preloads used, on every state", async ({ page, pageUrl }) => {
  const lists = new Set<string>();
  const problems: string[] = [];
  for (const state of STATES_TO_CHECK) {
    await open(page, pageUrl(state), 390, 844);
    const report = await page.evaluate(headerReport);
    lists.add(report.links);
    problems.push(...report.problems.map((problem) => `${state.route} (${state.name}): ${problem}`));
  }
  expect(problems).toEqual([]);
  expect([...lists]).toHaveLength(1);
});

for (const route of FOCUS_ROUTES) {
  test(`A11Y-026 WEB-027: the open Menu covers no focused element on ${route} at 390 by 844`, async ({ page, pageUrl }) => {
    await open(page, pageUrl(stateOf(route)), 390, 844);
    await openMenuFromKeyboard(page);
    const names: string[] = [];
    for (let stop = 0; stop < STOPS_AFTER_OPEN; stop += 1) {
      await page.keyboard.press("Tab");
      expect(await page.evaluate(focusProblem), `stop ${String(stop + 1)} after the Menu opens`).toBeNull();
      names.push(await page.evaluate(() => document.activeElement?.textContent.trim() ?? ""));
    }
    expect(names.slice(0, MENU_LINKS.length)).toEqual(MENU_LINKS);
  });
}

for (const width of [320, 390, 768]) {
  test(`WEB-027: the Menu pill and its open links show and the row holds at ${String(width)}px`, async ({ page, pageUrl }) => {
    await open(page, pageUrl(stateOf("/")), width, 844);
    const closed = await page.evaluate(rowReport, PHONE_CONTROLS);
    expect(closed.hidden).not.toContain("Menu");
    await page.locator("details.menu > summary").click();
    const opened = await page.evaluate(rowReport, PHONE_CONTROLS);
    expect(opened.hidden).toEqual([]);
    expect(opened.sideways, "the page scrolls sideways with the Menu open").toBe(false);
    if (width >= 768) return;
    expect(closed.height, "the closed header is one row").toBeLessThanOrEqual(closed.limit);
    expect(opened.tops, "opening the Menu moves the wordmark, Español or the pill").toEqual(closed.tops);
    expect(opened.listTop, "the open list starts under the pill").toBeGreaterThanOrEqual(opened.pillBottom);
  });
}

for (const width of [1024, 1280]) {
  test(`WEB-027: the four plain links and Español show at ${String(width)}px`, async ({ page, pageUrl }) => {
    await open(page, pageUrl(stateOf("/")), width);
    const row = await page.evaluate(rowReport, 'body > header .site-nav a, body > header a[href="/es"]');
    expect(row.hidden).toEqual([]);
    expect(await page.locator('body > header .site-nav a, body > header a[href="/es"]').count()).toBe(MENU_LINKS.length + 1);
  });
}
