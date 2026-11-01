// Target size, dragging and contrast on every page and state at 320, 768 and 1280 px (T053;
// A11Y-006, A11Y-007; docs/test-design.md section 6; docs/brains/accessibility-forms.md section 2).
//
// WCAG 2.5.8: a target under 24 by 24 CSS px passes only if it is inline in a sentence or if a
// 24 px circle on its centre touches no other target and no other small target's circle. Nothing
// is draggable and no range slider stands alone. Text contrast is axe's colour-contrast rules (4.5:1,
// 3:1 for large text); a button's or field's edge (its border or its own background against the
// page) is at least 3:1. The planted case proves a 20 px target beside another is reported.
// Fake data only.
import { STATES_TO_CHECK, TARGETS, WIDTHS, axeScan, expect, open, test } from "./fixtures";

/** A11Y-006: targets under 24 px that no WCAG 2.5.8 exception covers, and drag-only controls. */
function targetProblems(selector: string): string[] {
  const boxes = [...document.querySelectorAll(selector)]
    .map((el) => ({ el, r: el.getBoundingClientRect() }))
    .filter(({ r }) => r.width > 0 && r.height > 0);
  const small = (r: DOMRect): boolean => r.width < 24 || r.height < 24;
  const centre = (r: DOMRect): [number, number] => [r.left + r.width / 2, r.top + r.height / 2];
  const inSentence = (el: Element): boolean => {
    let block = el.parentElement;
    while (block !== null && getComputedStyle(block).display.startsWith("inline")) block = block.parentElement;
    const inline = (n: ChildNode): boolean => n.nodeType === Node.TEXT_NODE
      || (n instanceof Element && !n.matches(selector) && getComputedStyle(n).display.startsWith("inline"));
    const own = [...(block?.childNodes ?? [])].filter((n) => !n.contains(el) && inline(n));
    return getComputedStyle(el).display === "inline" && own.some((n) => (n.textContent ?? "").trim() !== "");
  };
  const problems = boxes.flatMap(({ el, r }) => {
    if (!small(r) || inSentence(el)) return [];
    const [x, y] = centre(r);
    const clash = boxes.some((other) => {
      if (other.el === el) return false;
      const nx = Math.max(other.r.left, Math.min(x, other.r.right));
      const ny = Math.max(other.r.top, Math.min(y, other.r.bottom));
      const [ox, oy] = centre(other.r);
      return Math.hypot(nx - x, ny - y) < 12 || (small(other.r) && Math.hypot(ox - x, oy - y) < 24);
    });
    const size = `${String(Math.round(r.width))}x${String(Math.round(r.height))}`;
    return clash ? [`<${el.tagName}> "${el.textContent.trim()}" is ${size} px and too close to another target`] : [];
  });
  const dragged = document.querySelectorAll('[draggable="true"], input[type="range"]');
  return [...problems, ...[...dragged].map((el) => `<${el.tagName}> needs dragging`)];
}

/** A11Y-007: a button or field whose edge is under 3:1 against the page behind it. */
function edgeProblems(): string[] {
  const luminance = (color: string): number => {
    const [r = 0, g = 0, b = 0] = (color.match(/[\d.]+/g) ?? []).map(Number).map((v) => {
      const c = v / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const ratio = (a: string, b: string): number => {
    const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
    return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
  };
  const behind = (el: Element | null): string => {
    for (let at = el; at !== null; at = at.parentElement) {
      const color = getComputedStyle(at).backgroundColor;
      if (!/rgba\(.*, 0\)$|transparent/.test(color)) return color;
    }
    return "rgb(255, 255, 255)";
  };
  const fields = document.querySelectorAll('button, select, textarea, input:not([type="hidden"])');
  return [...fields].flatMap((el) => {
    const style = getComputedStyle(el);
    const page = behind(el.parentElement);
    const border = parseFloat(style.borderTopWidth) > 0 ? ratio(style.borderTopColor, page) : 1;
    const fill = /rgba\(.*, 0\)$|transparent/.test(style.backgroundColor) ? 1 : ratio(style.backgroundColor, page);
    return Math.max(border, fill) >= 3 ? [] : [`<${el.tagName}> "${el.textContent.trim()}" edge under 3:1`];
  });
}

for (const state of STATES_TO_CHECK) {
  for (const width of WIDTHS) {
    test(`A11Y-006 A11Y-007: targets and contrast on ${state.route} (${state.name}) at ${String(width)}px`, async ({ page, pageUrl }) => {
      await open(page, pageUrl(state), width);
      expect(await page.evaluate(targetProblems, TARGETS)).toEqual([]);
      expect((await axeScan(page, ["color-contrast", "link-in-text-block"])).violations).toEqual([]);
      expect(await page.evaluate(edgeProblems)).toEqual([]);
    });
  }
}

test("A11Y-006: a planted 20 px target beside another is reported", async ({ page, pageUrl }) => {
  const [state] = STATES_TO_CHECK;
  if (state === undefined) throw new Error("no page state to plant into");
  await open(page, pageUrl(state), 320);
  await page.evaluate(() => {
    const row = document.createElement("p");
    for (const label of ["A", "B"]) {
      const button = document.createElement("button");
      Object.assign(button.style, { width: "20px", height: "20px", padding: "0", margin: "0" });
      button.textContent = label;
      row.append(button);
    }
    document.querySelector("main")?.append(row);
  });
  const problems = await page.evaluate(targetProblems, TARGETS);
  expect(problems.filter((p) => p.includes("is 20x20 px"))).toHaveLength(2);
});
