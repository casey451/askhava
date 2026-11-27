// Forced colors (T612; WEB-029, A11Y-002; DESIGN_SPEC `:153`, WCAG 1.4.11; D-4252).
//
// With `forcedColors: "active"`, on the home page, the quote page with three quotes, the pick review
// step (that page's Pick #1) and the help form, at 390 and 1440 px: every card (`.card` and each
// quote `article`), button, input, textarea and select with a box keeps a side with a computed border
// width over 0 or an outline; a checkbox or radio never computes `appearance: none` (the native
// control draws its own edge there, so it is held to that rule in place of the border one, D-5838);
// and axe-core reports no violation. Planted pages whose card has `border: 0`, and whose checkbox is
// `appearance: none`, fail. Fake data only.
import type { Page } from "@playwright/test";
import { PAGE_STATES, axeScan, expect, open, test, type PageState } from "./fixtures";

/** Runs in the page: each edged control or card that lost its edge, each native box drawn away. */
function forcedProblems(): string[] {
  const out: string[] = [];
  for (const el of document.querySelectorAll(".card, article, button, input, textarea, select")) {
    const r = el.getBoundingClientRect();
    if (r.width <= 1 || r.height <= 1) continue;
    const s = getComputedStyle(el);
    const what = `<${el.tagName}${el.id ? `#${el.id}` : ""}>`;
    if (el.matches('input[type="checkbox"], input[type="radio"]')) {
      if (s.appearance === "none") out.push(`${what} computes appearance: none`);
      continue;
    }
    const border = ["top", "right", "bottom", "left"].some((side) => parseFloat(s.getPropertyValue(`border-${side}-width`)) > 0);
    const outline = s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0;
    if (!border && !outline) out.push(`${what} has no border and no outline`);
  }
  return out;
}

/** The page state named `name`; none fails the run. */
const byName = (name: string): PageState => PAGE_STATES.find((s) => s.name === name) ?? (() => { throw new Error(`no page state "${name}"`); })();

/** The screens D-4252 names: [label, state, the step taken after opening it, if any]. */
const SCREENS: [string, PageState, ((page: Page) => Promise<void>) | null][] = [
  ["home page", byName("home page"), null],
  ["quote page with three quotes", byName("three quotes, #2 gone"), null],
  ["pick review step", byName("three quotes, #2 gone"), async (page) => {
    await page.getByRole("button", { name: "Pick #1" }).click();
    await expect(page.locator("#final-pick")).toBeVisible();
  }],
  ["help form", byName("help page"), null],
];

for (const [label, state, step] of SCREENS) {
  for (const width of [390, 1440]) {
    test(`D-4252 A11Y-002: the ${label} keeps every edge in forced colors at ${String(width)}px`, async ({ page, pageUrl }) => {
      await page.emulateMedia({ forcedColors: "active" });
      await open(page, pageUrl(state), width);
      if (step !== null) await step(page);
      expect(await page.evaluate(forcedProblems)).toEqual([]);
      expect((await axeScan(page)).violations).toEqual([]);
    });
  }
}

test("D-4252: a planted card with border 0 and a drawn-away checkbox fail in forced colors", async ({ page }) => {
  await page.emulateMedia({ forcedColors: "active" });
  await page.setContent(`<main><div class="card" style="border: 0">Planted card</div>
    <label><input id="box" type="checkbox" style="appearance: none; border: 2px solid">Planted box</label></main>`);
  expect(await page.evaluate(forcedProblems)).toEqual(["<DIV> has no border and no outline", "<INPUT#box> computes appearance: none"]);
});
