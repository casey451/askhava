// Reflow, zoom, text spacing and reduced motion on every page and state (T053; A11Y-008, A11Y-009;
// docs/test-design.md section 6).
//
// At 320 px wide, and at 400% zoom (a 1280 by 1024 window at 400% is a 320 by 256 CSS px viewport),
// nothing scrolls or runs off sideways except inside a data table's focusable, labelled region,
// and every table sits in one. With WCAG 1.4.12's spacing applied, at every width, no text is
// clipped, no two text boxes overlap and no control is hidden. With reduced motion asked for,
// at every width, every animation and transition lasts 0 s and nothing scrolls smoothly. Only the
// text-spacing tests turn on `bypassCSP`, to add their stylesheet. The planted case proves
// sideways scrolling at 320 px is reported. Fake data only.
import { STATES_TO_CHECK, TARGETS, TEXT_SPACING, WIDTHS, expect, open, test } from "./fixtures";

/** A11Y-008: sideways scrolling and content off the side, outside a table's scroll region. */
function reflowProblems(): string[] {
  const width = document.documentElement.clientWidth;
  const region = '[role="region"][tabindex="0"][aria-label]';
  const problems: string[] = [];
  if (document.documentElement.scrollWidth > width) problems.push(`the page scrolls sideways at ${String(width)}px`);
  for (const el of document.querySelectorAll("body *")) {
    const r = el.getBoundingClientRect();
    if (r.width <= 1 || r.height <= 1 || el.closest(region)) continue;
    if (r.right > width + 1 || r.left < -1) problems.push(`<${el.tagName}> runs off the side`);
  }
  for (const table of document.querySelectorAll("table")) {
    if (!table.parentElement?.matches(region)) problems.push("a table outside a labelled, focusable region");
  }
  return problems;
}

/** A11Y-009: clipped text, overlapping text boxes and hidden controls. */
function spacingProblems(targets: string): string[] {
  const problems: string[] = [];
  const texty = [...document.querySelectorAll("body *")].filter((el) =>
    [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? "").trim() !== ""));
  for (const el of texty) {
    const style = getComputedStyle(el);
    const clips = /hidden|clip/.test(style.overflowX + style.overflowY) || style.textOverflow === "ellipsis";
    if (clips && (el.scrollHeight > el.clientHeight + 1 || el.scrollWidth > el.clientWidth + 1)) problems.push(`<${el.tagName}> text is clipped`);
  }
  texty.forEach((a, i) => {
    for (const b of texty.slice(i + 1)) {
      if (a.contains(b) || b.contains(a)) continue;
      const [ra, rb] = [a.getBoundingClientRect(), b.getBoundingClientRect()];
      if (ra.left < rb.right - 1 && rb.left < ra.right - 1 && ra.top < rb.bottom - 1 && rb.top < ra.bottom - 1) {
        problems.push(`<${a.tagName}> overlaps <${b.tagName}>`);
      }
    }
  });
  const fails = (c: Element): boolean => {
    c.scrollIntoView({ block: "center" });
    const r = c.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return r.width < 1 || r.height < 1 || hit === null || !(c.contains(hit) || hit.contains(c));
  };
  // D-4970 (1): inside a closed <details>, behind its own summary, when that summary passes.
  const folded = (c: Element): boolean => {
    const summary = c.closest("details:not([open])")?.querySelector(":scope > summary") ?? null;
    return summary !== null && summary !== c && !fails(summary);
  };
  // D-4970 (2), D-4991: a boxless link, or links' own summary, whose outermost `display: none`
  // container holds no other control and only links that have a passing same-href link outside it.
  const duplicated = (c: Element): boolean => {
    let box: Element | null = null;
    for (let e: Element | null = c; e !== null; e = e.parentElement) if (getComputedStyle(e).display === "none") box = e;
    if (box === null || c.getClientRects().length > 0) return false;
    const inside = [box, ...box.querySelectorAll("*")];
    const allowed = (e: Element): boolean => e.matches("a[href]") ||
      (e !== box && e.matches("details > summary:first-of-type") && e.parentElement?.querySelector("a[href]") != null);
    const twin = (a: Element): boolean => [...document.querySelectorAll("a[href]")].some((o) =>
      !inside.includes(o) && o.getAttribute("href") === a.getAttribute("href") && (folded(o) || !fails(o)));
    return allowed(c) && inside.every((e) => !e.matches(targets) || allowed(e)) && inside.every((e) => !e.matches("a[href]") || twin(e));
  };
  for (const control of document.querySelectorAll(targets)) {
    if (!folded(control) && !duplicated(control) && fails(control)) problems.push(`<${control.tagName}> "${control.textContent.trim()}" is hidden`);
  }
  return problems;
}

/** A11Y-009: anything that still moves with `prefers-reduced-motion: reduce`. */
function motionProblems(): string[] {
  const moving = (list: string): boolean => list.split(",").some((value) => parseFloat(value) !== 0);
  return [document.documentElement, ...document.querySelectorAll("body, body *")].flatMap((el) =>
    [null, "::before", "::after"].flatMap((pseudo) => {
      const style = getComputedStyle(el, pseudo);
      const what = `<${el.tagName}>${pseudo ?? ""}`;
      return [
        ...(moving(style.animationDuration) && style.animationName !== "none" ? [`${what} animates`] : []),
        ...(moving(style.transitionDuration) ? [`${what} has a transition`] : []),
        ...(style.scrollBehavior === "smooth" ? [`${what} scrolls smoothly`] : []),
      ];
    }));
}

for (const state of STATES_TO_CHECK) {
  const where = `${state.route} (${state.name})`;

  test(`A11Y-008: ${where} reflows at 320px and at 400% zoom`, async ({ page, pageUrl }) => {
    await open(page, pageUrl(state), 320);
    expect(await page.evaluate(reflowProblems)).toEqual([]);
    await open(page, pageUrl(state), 320, 256);
    expect(await page.evaluate(reflowProblems)).toEqual([]);
  });

  for (const width of WIDTHS) {
    test(`A11Y-009: ${where} stops all motion for reduced motion at ${String(width)}px`, async ({ page, pageUrl }) => {
      await page.emulateMedia({ reducedMotion: "reduce" });
      await open(page, pageUrl(state), width);
      expect(await page.evaluate(motionProblems)).toEqual([]);
    });
  }
}

// Only these tests bypass the page's CSP, which allows no inline style (SEC-013), so they can add
// the text-spacing stylesheet; every other test here and in the suite runs under the real CSP.
test.describe("text spacing", () => {
  test.use({ bypassCSP: true });

  for (const state of STATES_TO_CHECK) {
    for (const width of WIDTHS) {
      test(`A11Y-009: ${state.route} (${state.name}) takes the text-spacing override at ${String(width)}px`, async ({ page, pageUrl }) => {
        await open(page, pageUrl(state), width);
        await page.addStyleTag({ content: TEXT_SPACING });
        expect(await page.evaluate(spacingProblems, TARGETS)).toEqual([]);
        expect(await page.evaluate(reflowProblems)).toEqual([]);
      });
    }
  }
});

test("A11Y-008: planted sideways scrolling at 320px is reported", async ({ page, pageUrl }) => {
  const [state] = STATES_TO_CHECK;
  if (state === undefined) throw new Error("no page state to plant into");
  await open(page, pageUrl(state), 320);
  await page.evaluate(() => {
    const wide = document.createElement("p");
    wide.style.width = "400px";
    wide.textContent = "A planted line wider than the screen";
    document.querySelector("main")?.append(wide);
  });
  expect(await page.evaluate(reflowProblems)).toContain("the page scrolls sideways at 320px");
});

// Planted pages for the hidden-control loop (D-4970, D-4991), set as page content so they can carry
// their own styles: [what, markup, width, the controls reported as hidden]. Fake links only.
const LINKS = '<a href="/how">How</a><a href="/help">Help</a>';
const HEADER = `<style>.site-nav { display: none } @media (min-width: 1024px) { .site-nav { display: block } .menu { display: none } }</style>
  <header><a href="/">Home</a><details class="menu"><summary>Menu</summary>${LINKS}</details><nav class="site-nav">${LINKS}</nav></header>`;
const PLANTED: [string, string, number, string[]][] = [
  ["a display: none link with no counterpart", '<a href="/a" hidden>Alone</a>', 390, ['<A> "Alone"']],
  ["a hidden container whose counterpart is itself hidden", '<p hidden><a href="/a">One</a></p><p hidden><a href="/a">Two</a></p>', 390, ['<A> "One"', '<A> "Two"']],
  ["a closed details whose summary alone is display: none", '<details><summary hidden>Menu</summary><a href="/a">In</a></details>', 390, ['<SUMMARY> "Menu"']],
  ["a hidden container holding a duplicated link and a button", '<a href="/a">A</a><p hidden><a href="/a">Copy</a><button>Go</button></p>', 390, ['<A> "Copy"', '<BUTTON> "Go"']],
  ["a hidden details whose links have no counterpart outside it", `<details hidden><summary>Menu</summary>${LINKS}</details>`, 390, ['<SUMMARY> "Menu"', '<A> "How"', '<A> "Help"']],
  ["a hidden details holding duplicated links and a button", `${LINKS}<details hidden><summary>Menu</summary>${LINKS}<button>Go</button></details>`, 390, ['<SUMMARY> "Menu"', '<A> "How"', '<A> "Help"', '<BUTTON> "Go"']],
  ["a hidden details holding only its summary", "<details hidden><summary>Menu</summary></details>", 390, ['<SUMMARY> "Menu"']],
  ["a rendered control covered by another element", '<a href="/a">Under</a><p style="position: fixed; inset: 0"></p>', 390, ['<A> "Under"']],
  ["the specified header on a phone (docs/design/DESIGN_SPEC.md)", HEADER, 390, []],
  ["the specified header on a desktop (docs/design/DESIGN_SPEC.md)", HEADER, 1280, []],
];

for (const [what, markup, width, reported] of PLANTED) {
  test(`A11Y-009: planted ${what} at ${String(width)}px reports ${String(reported.length)} hidden controls`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.setContent(markup);
    expect(await page.evaluate(spacingProblems, TARGETS)).toEqual(reported.map((control) => `${control} is hidden`));
  });
}
