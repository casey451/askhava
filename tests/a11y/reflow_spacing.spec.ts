// Reflow, zoom, text spacing and reduced motion on every page and state (T053; A11Y-008, A11Y-009;
// docs/test-design.md section 6).
//
// At 320 px wide, and at 400% zoom (a 1280 by 1024 window at 400% is a 320 by 256 CSS px viewport),
// nothing scrolls or runs off sideways except inside a data table's focusable, labelled region,
// and every table sits in one. With WCAG 1.4.12's spacing applied, at every width, no text is
// clipped, no two text boxes overlap and no control is hidden. With reduced motion asked for,
// every animation and transition lasts 0 s and nothing scrolls smoothly. The planted case proves
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
  for (const control of document.querySelectorAll(targets)) {
    control.scrollIntoView({ block: "center" });
    const r = control.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    if (r.width < 1 || r.height < 1 || hit === null || !(control.contains(hit) || hit.contains(control))) {
      problems.push(`<${control.tagName}> "${control.textContent.trim()}" is hidden`);
    }
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
    test(`A11Y-009: ${where} takes the text-spacing override at ${String(width)}px`, async ({ page, pageUrl }) => {
      await open(page, pageUrl(state), width);
      await page.addStyleTag({ content: TEXT_SPACING });
      expect(await page.evaluate(spacingProblems, TARGETS)).toEqual([]);
      expect(await page.evaluate(reflowProblems)).toEqual([]);
    });
  }

  test(`A11Y-009: ${where} stops all motion for reduced motion`, async ({ page, pageUrl }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await open(page, pageUrl(state), 1280);
    expect(await page.evaluate(motionProblems)).toEqual([]);
  });
}

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
