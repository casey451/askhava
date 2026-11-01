// Page structure on every page and state (T053; A11Y-003, A11Y-004, A11Y-017, A11Y-018, A11Y-026;
// TD-A11Y-01's sticky-bar case; docs/test-design.md section 6).
//
// One `h1`, one `main`, all content in landmarks, a skip link that is the first Tab stop and
// points into `main`, `lang` of en or es on the page and on every inline passage, no ARIA role an
// HTML element already gives; status regions with `role="status"`, urgent ones with `role="alert"`;
// help in the same landmark and order on every page; tabbing never leaves the focused element fully
// under a sticky or fixed bar, whose height the page's scroll-padding covers. The planted case
// proves a second `h1` is reported. Fake data only.
import type { Page } from "@playwright/test";
import { STATES_TO_CHECK, WIDTHS, axeScan, expect, open, test } from "./fixtures";

/** axe's structure rules, best-practice ones included (A11Y-003, A11Y-004). */
const STRUCTURE_RULES = ["page-has-heading-one", "landmark-one-main", "region", "skip-link", "bypass", "landmark-unique",
  "landmark-no-duplicate-banner", "landmark-no-duplicate-contentinfo", "landmark-no-duplicate-main", "html-has-lang",
  "html-lang-valid", "valid-lang"];

/** A11Y-003, A11Y-004, A11Y-018: the DOM rules axe does not check, as a list of problems. */
function structureProblems(statusIds: string[]): string[] {
  const problems: string[] = [];
  const h1s = document.querySelectorAll("h1").length;
  if (h1s !== 1) problems.push(`${String(h1s)} h1 elements`);
  if (document.querySelectorAll("main, [role=main]").length !== 1) problems.push("not exactly one main");
  for (const el of [document.documentElement, ...document.querySelectorAll("body [lang]")]) {
    if (!["en", "es"].includes(el.getAttribute("lang") ?? "")) problems.push(`lang "${el.getAttribute("lang") ?? ""}" on <${el.tagName}>`);
  }
  const native = ["button", "link", "heading", "list", "listitem", "navigation", "main", "banner", "contentinfo", "checkbox", "radio", "textbox"];
  for (const el of document.querySelectorAll("[role]")) {
    if (native.includes(el.getAttribute("role") ?? "")) problems.push(`role="${el.getAttribute("role") ?? ""}" where HTML has an element`);
  }
  for (const id of statusIds) {
    if (document.getElementById(id)?.getAttribute("role") !== "status") problems.push(`#${id} is not role="status"`);
  }
  for (const el of document.querySelectorAll("[role=status], [role=alert], [aria-live]")) {
    const role = el.getAttribute("role");
    const live = el.getAttribute("aria-live");
    const want = role === "alert" ? "assertive" : "polite";
    if ((role !== "status" && role !== "alert") || (live !== null && live !== want)) {
      problems.push(`live region role="${role ?? ""}" aria-live="${live ?? ""}"`);
    }
  }
  return problems;
}

/** A11Y-003: the first Tab stop is a skip link to `main` or into it. */
async function skipLinkProblem(page: Page): Promise<string | null> {
  await page.keyboard.press("Tab");
  return page.evaluate(() => {
    const link = document.activeElement;
    const href = link instanceof HTMLAnchorElement ? link.getAttribute("href") ?? "" : "";
    const target = href.startsWith("#") ? document.getElementById(href.slice(1)) : null;
    return target?.closest("main") ? null : "the first Tab stop is not a skip link into main";
  });
}

/** A11Y-026: the focused element is fully under a fixed or sticky bar, or a bar's height is not padded. */
function coveredProblem(): string | null {
  const focused = document.activeElement;
  if (focused === null || focused === document.body) return null;
  const box = focused.getBoundingClientRect();
  const root = getComputedStyle(document.documentElement);
  for (const bar of document.querySelectorAll("body *")) {
    const position = getComputedStyle(bar).position;
    if ((position !== "fixed" && position !== "sticky") || bar.contains(focused)) continue;
    const r = bar.getBoundingClientRect();
    const atTop = r.top <= 0 && (parseFloat(root.scrollPaddingTop) || 0) < r.bottom;
    const atBottom = r.bottom >= innerHeight && (parseFloat(root.scrollPaddingBottom) || 0) < innerHeight - r.top;
    if (atTop || atBottom) return `scroll-padding does not cover the ${atTop ? "top" : "bottom"} bar`;
    if (r.left <= box.left && r.right >= box.right && r.top <= box.top && r.bottom >= box.bottom) {
      return `<${focused.tagName}> "${focused.textContent.trim()}" is under a ${position} bar`;
    }
  }
  return null;
}

/** A11Y-017: where the help link sits: its landmark and its place among that landmark's links. */
function helpPlace(): string {
  const links = [...document.querySelectorAll("a[href]")];
  const help = links.find((a) => /help|ayuda/i.test(a.textContent) || /\/help\b/.test(a.getAttribute("href") ?? ""));
  const landmark = help?.closest("header, footer, nav, main, aside");
  if (help === undefined || landmark === null || landmark === undefined) return help === undefined ? "none" : "outside landmarks";
  return `${landmark.tagName} link ${String([...landmark.querySelectorAll("a[href]")].indexOf(help) + 1)}`;
}

for (const state of STATES_TO_CHECK) {
  for (const width of WIDTHS) {
    test(`A11Y-003 A11Y-004 A11Y-018 A11Y-026 TD-A11Y-01: structure of ${state.route} (${state.name}) at ${String(width)}px`, async ({ page, pageUrl }) => {
      await open(page, pageUrl(state), width);
      expect((await axeScan(page, STRUCTURE_RULES)).violations).toEqual([]);
      expect(await page.evaluate(structureProblems, state.statusRegions)).toEqual([]);
      expect(await skipLinkProblem(page)).toBeNull();
      const stops = await page.locator("a[href], button, input, select, textarea, [tabindex='0']").count();
      for (let i = 0; i <= stops; i += 1) {
        expect(await page.evaluate(coveredProblem)).toBeNull();
        await page.keyboard.press("Tab");
      }
    });
  }
}

test("A11Y-017: help is in the same place on every page", async ({ page, pageUrl }) => {
  const places = new Set<string>();
  for (const state of STATES_TO_CHECK) {
    for (const width of WIDTHS) {
      await open(page, pageUrl(state), width);
      places.add(await page.evaluate(helpPlace));
    }
  }
  expect([...places]).toHaveLength(1);
  expect(places.has("outside landmarks")).toBe(false);
});

test("A11Y-003: a planted second h1 is reported", async ({ page, pageUrl }) => {
  const [state] = STATES_TO_CHECK;
  if (state === undefined) throw new Error("no page state to plant into");
  await open(page, pageUrl(state), 320);
  await page.evaluate(() => document.querySelector("main")?.append(document.createElement("h1")));
  expect(await page.evaluate(structureProblems, [])).toContain("2 h1 elements");
});
