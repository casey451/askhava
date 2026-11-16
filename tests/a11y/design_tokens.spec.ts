/// <reference types="node" />
// The design-token test (T166; TD-WEB-11, TD-A11Y-01; WEB-027, A11Y-028; D-3331, D-3878).
//
// The tokens and their hex values are read from docs/design/DESIGN_SPEC.md section 2, the one
// source of the colours. The served /static/site/site.css may use only those values, declares
// each token it names with the spec's value, sets no font size under 16 px and loads fonts only
// from /static/site/fonts/. Each text pair section 2 allows computes at least 4.5:1, and each
// input border and focus ring at least 3:1, against every surface it can sit on (canvas, surface,
// band, night), with no rounding up. Rendered pages styled by site.css alone (a sample page that
// links it, and every page state whose page links no other stylesheet) show only token colours,
// no text under 16 px, borders and focus rings at 3:1 or more and fonts from the site itself.
// The portal stylesheets and the email colours are T520's (D-3878). The planted cases prove a
// `#9A9A9A` text colour on `canvas` fails. Fake data only.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { STATES_TO_CHECK, expect, open, test } from "./fixtures";

const REPO_ROOT = join(import.meta.dirname, "..", "..");

/** Section 2's colour tokens, name to `#RRGGBB`, from the spec's table rows. */
const TOKENS: Record<string, string> = Object.fromEntries(
  [...readFileSync(join(REPO_ROOT, "docs/design/DESIGN_SPEC.md"), "utf8")
    .matchAll(/^\| `([a-z-]+)` \| `(#[0-9A-Fa-f]{6})` \|/gm)]
    .map((m) => [m[1] ?? "", (m[2] ?? "").toUpperCase()]),
);

const LIGHT = ["canvas", "surface", "band"];

/** One pair: a foreground token on a surface token, with its floor (4.5 text, 3 edges and rings). */
interface Pair { fg: string; on: string; min: number }

const pairs = (fgs: string[], ons: string[], min: number): Pair[] =>
  fgs.flatMap((fg) => ons.map((on) => ({ fg, on, min })));

/** WEB-027, A11Y-028: every pair section 2 lets sit together. */
const PAIRS: Pair[] = [
  ...pairs(["ink", "ink-soft", "evergreen", "evergreen-deep", "alert"], LIGHT, 4.5),
  ...pairs(["on-evergreen"], ["evergreen", "evergreen-deep"], 4.5),
  ...pairs(["on-night", "on-night-soft"], ["night"], 4.5),
  ...pairs(["field-edge", "focus"], LIGHT, 3),
  ...pairs(["focus-on-night"], ["night"], 3),
];

/** Tokens that are only surfaces or decoration (section 2 marks `line` and `night-line` decorative). */
const NOT_FOREGROUND = ["canvas", "surface", "band", "night", "line", "night-line"];

/** WCAG 2.x contrast ratio of two `#RRGGBB` colours. */
function ratio(a: string, b: string): number {
  const lum = (hex: string): number => {
    const [r = 0, g = 0, bl = 0] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
      .map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * bl;
  };
  const [hi = 0, lo = 0] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** WEB-027: the pairs under their floor, or naming a colour that is no token. */
function pairProblems(colours: Record<string, string>, list: Pair[]): string[] {
  return list.flatMap(({ fg, on, min }) => {
    const [a, b] = [colours[fg], colours[on]];
    if (a === undefined || b === undefined) return [`${fg} on ${on}: not a section 2 token`];
    const r = ratio(a, b);
    return r >= min ? [] : [`${fg} on ${on}: ${r.toFixed(2)}:1, under ${String(min)}:1`];
  });
}

/** WEB-027: a stylesheet's colours, font sizes and font sources against section 2 and section 3. */
function cssProblems(css: string): string[] {
  const values = new Set(Object.values(TOKENS));
  const colours = [...css.matchAll(/#[0-9A-Fa-f]{3,8}\b/g)].map((m) => m[0].toUpperCase())
    .filter((hex) => !values.has(hex)).map((hex) => `${hex} is not a section 2 token`);
  const declared = [...css.matchAll(/--([a-z-]+):\s*(#[0-9A-Fa-f]{6})/g)]
    .filter((m) => TOKENS[m[1] ?? ""] !== (m[2] ?? "").toUpperCase())
    .map((m) => `--${m[1] ?? ""} is ${m[2] ?? ""}, not section 2's value`);
  const sizes = [...css.matchAll(/font-size:\s*([^;}]+)/g)].map((m) => (m[1] ?? "").trim())
    .filter((size) => size !== "inherit" && !(/^\d+(\.\d+)?px$/.test(size) && parseFloat(size) >= 16))
    .map((size) => `font-size ${size} is not 16 px or more`);
  const sources = [...css.matchAll(/url\(\s*["']?([^"')]+)/g)].map((m) => m[1] ?? "")
    .filter((url) => !/^fonts\/[a-z0-9-]+\.woff2$/.test(url)).map((url) => `url ${url} is not a site font`);
  const imports = css.includes("@import") ? ["@import loads another stylesheet"] : [];
  return [...colours, ...declared, ...sizes, ...sources, ...imports];
}

/** Runs in the page: token colours, 16 px text, 4.5:1 text and 3:1 field edges (WEB-027, A11Y-028). */
function renderedProblems(tokens: Record<string, string>): string[] {
  const hex = (rgb: string): string => `#${(rgb.match(/\d+/g) ?? []).slice(0, 3)
    .map((v) => Number(v).toString(16).padStart(2, "0")).join("").toUpperCase()}`;
  const clear = (c: string): boolean => /rgba\(.*, 0\)$|transparent/.test(c);
  const lum = (h: string): number => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255)
    .map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
    .reduce((sum, c, i) => sum + c * ([0.2126, 0.7152, 0.0722][i] ?? 0), 0);
  const ratio = (a: string, b: string): number => {
    const [hi = 0, lo = 0] = [lum(a), lum(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  };
  const behind = (el: Element | null): string => {
    for (let at = el; at !== null; at = at.parentElement) {
      const c = getComputedStyle(at).backgroundColor;
      if (!clear(c)) return hex(c);
    }
    return "#FFFFFF";
  };
  const values = new Set(Object.values(tokens));
  const name = (el: Element): string => `<${el.tagName}> "${el.textContent.trim().slice(0, 30)}"`;
  return [...document.querySelectorAll("body *")].flatMap((el) => {
    const style = getComputedStyle(el);
    if (style.display === "none" || style.visibility === "hidden") return [];
    const out: string[] = [];
    if (!clear(style.backgroundColor) && !values.has(hex(style.backgroundColor))) out.push(`${name(el)} background is not a token`);
    const text = [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? "").trim() !== "");
    if (text) {
      const [fg, bg] = [hex(style.color), behind(el)];
      if (!values.has(fg)) out.push(`${name(el)} colour ${fg} is not a token`);
      if (ratio(fg, bg) < 4.5) out.push(`${name(el)} ${fg} on ${bg} is under 4.5:1`);
      if (parseFloat(style.fontSize) < 16) out.push(`${name(el)} is ${style.fontSize}, under 16 px`);
    }
    if (el.matches('input:not([type="hidden"]), textarea, select') && ratio(hex(style.borderTopColor), behind(el.parentElement)) < 3) {
      out.push(`${name(el)} border is under 3:1`);
    }
    return out;
  });
}

/** Runs in the page: the focused element's ring against what it sits on (WEB-027, A11Y-028). */
function ringProblem(): string | null {
  const el = document.activeElement;
  if (el === null || el === document.body) return null;
  const hex = (rgb: string): string => (rgb.match(/\d+/g) ?? []).slice(0, 3)
    .map((v) => Number(v).toString(16).padStart(2, "0")).join("");
  const lum = (h: string): number => [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16) / 255)
    .map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4))
    .reduce((sum, c, i) => sum + c * ([0.2126, 0.7152, 0.0722][i] ?? 0), 0);
  let bg = "ffffff";
  for (let at = el.parentElement; at !== null; at = at.parentElement) {
    const c = getComputedStyle(at).backgroundColor;
    if (!/rgba\(.*, 0\)$|transparent/.test(c)) { bg = hex(c); break; }
  }
  const style = getComputedStyle(el);
  const [hi = 0, lo = 0] = [lum(hex(style.outlineColor)), lum(bg)].sort((x, y) => y - x);
  const ok = style.outlineStyle !== "none" && parseFloat(style.outlineWidth) >= 3 && (hi + 0.05) / (lo + 0.05) >= 3;
  return ok ? null : `<${el.tagName}> "${el.textContent.trim().slice(0, 30)}" focus ring under 3:1 or 3 px`;
}

/** Tabs through the page and returns every focus ring under 3:1 (A11Y-028). */
async function ringProblems(page: Page): Promise<string[]> {
  const out: string[] = [];
  for (let step = 0; step < 80; step += 1) {
    await page.keyboard.press("Tab");
    if (await page.evaluate(() => document.activeElement === document.body)) break;
    const problem = await page.evaluate(ringProblem);
    if (problem !== null) out.push(problem);
  }
  return out;
}

/** Every font the page requested comes from /static/site/fonts/ on its own host (WEB-027). */
function watchFonts(page: Page): string[] {
  const foreign: string[] = [];
  page.on("request", (request) => {
    if (request.resourceType() !== "font") return;
    const url = new URL(request.url());
    if (url.origin !== new URL(page.url()).origin || !url.pathname.startsWith("/static/site/fonts/")) foreign.push(url.href);
  });
  return foreign;
}

/** The families and weights the page has loaded, as "DM Sans 400". */
async function loadedFonts(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    await document.fonts.ready;
    return [...document.fonts].filter((f) => f.status === "loaded")
      .map((f) => `${f.family.replace(/"/g, "")} ${f.weight}`);
  });
}

/** A sample page styled by site.css alone, with each surface, text kind and control. Fake data. */
const SAMPLE = `<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><title>Sample</title></head>
<body><link rel="stylesheet" href="/static/site/site.css"><a class="skip-link" href="#main">Skip to main content</a>
<section class="site-banner" aria-label="Site status"><p>A sample banner line.</p></section>
<header><p class="wordmark"><a href="/">Sample</a></p><nav aria-label="Site"><a href="/help">Help</a></nav></header>
<main id="main" tabindex="-1"><h1>Sample heading</h1><h2>Second heading</h2><h3>Third heading</h3>
<p>Body text with <a href="/help">a link</a>. <small>Small print.</small></p>
<form novalidate><label for="w">What needs doing?</label><input id="w" name="w" value="Fix a sample tap">
<label for="t">More detail</label><textarea id="t" name="t">Sample words</textarea>
<label for="s">Town</label><select id="s" name="s"><option>Sampletown</option></select>
<button type="submit">Send</button></form>
<section class="night"><h2>Sample band</h2><p>White words with <a href="/help">a link</a>.</p></section></main></body></html>`;

test("TD-WEB-11 WEB-027 A11Y-028: every section 2 pair meets its floor on every surface it sits on", () => {
  expect(Object.keys(TOKENS).length).toBeGreaterThanOrEqual(17);
  expect(pairProblems(TOKENS, PAIRS)).toEqual([]);
  const foregrounds = Object.keys(TOKENS).filter((t) => !NOT_FOREGROUND.includes(t));
  expect(foregrounds.filter((t) => !PAIRS.some((p) => p.fg === t))).toEqual([]);
});

test("TD-WEB-11 WEB-027: site.css uses only section 2 tokens, 16 px text and the site's own fonts", async ({ request }) => {
  const reply = await request.get("/static/site/site.css");
  expect(reply.status()).toBe(200);
  expect(cssProblems(await reply.text())).toEqual([]);
  for (const file of ["gelasio-latin-400-normal", "gelasio-latin-500-normal", "dm-sans-latin-400-normal", "dm-sans-latin-500-normal", "dm-sans-latin-700-normal"]) {
    const font = await request.get(`/static/site/fonts/${file}.woff2`);
    expect(font.status(), file).toBe(200);
    expect(font.headers()["content-type"], file).toBe("font/woff2");
  }
});

test("TD-WEB-11: a planted #9A9A9A text colour on canvas fails", async ({ request }) => {
  const planted = { ...TOKENS, planted: "#9A9A9A" };
  expect(pairProblems(planted, [{ fg: "planted", on: "canvas", min: 4.5 }])).toHaveLength(1);
  const css = `${await (await request.get("/static/site/site.css")).text()}\np { color: #9A9A9A; }`;
  expect(cssProblems(css)).toEqual(["#9A9A9A is not a section 2 token"]);
});

test("TD-WEB-11 TD-A11Y-01 WEB-027 A11Y-028: a page styled by site.css shows tokens, 16 px text, 3:1 edges and rings and site fonts", async ({ page, baseURL }) => {
  await page.route(`${baseURL ?? ""}/__design-sample`, (route) => route.fulfill({ contentType: "text/html", body: SAMPLE }));
  const foreign = watchFonts(page);
  await page.setViewportSize({ width: 320, height: 900 });
  await page.goto("/__design-sample");
  expect(await loadedFonts(page)).toEqual(expect.arrayContaining(["DM Sans 400", "DM Sans 700", "Gelasio 500"]));
  expect(await page.evaluate(renderedProblems, TOKENS)).toEqual([]);
  expect(await ringProblems(page)).toEqual([]);
  expect(foreign).toEqual([]);
  await page.evaluate(() => {
    const p = document.createElement("p");
    p.style.color = "#9A9A9A";
    p.textContent = "Planted grey words";
    document.querySelector("main")?.append(p);
  });
  expect((await page.evaluate(renderedProblems, TOKENS)).filter((p) => p.includes("Planted"))).toHaveLength(2);
});

test("TD-WEB-11 WEB-027 A11Y-028: every rendered page styled by site.css alone passes the token checks", async ({ page, pageUrl }) => {
  const foreign = watchFonts(page);
  for (const state of STATES_TO_CHECK) {
    await open(page, pageUrl(state), 320);
    const ownSheets = await page.evaluate(() => [...document.querySelectorAll('link[rel="stylesheet"], style')]
      .every((el) => el instanceof HTMLLinkElement && new URL(el.href).pathname.startsWith("/static/site/")));
    if (!ownSheets) continue;
    const where = `${state.route} (${state.name})`;
    expect(await loadedFonts(page), where).toContain("DM Sans 400");
    expect(await page.evaluate(renderedProblems, TOKENS), where).toEqual([]);
    expect(await ringProblems(page), where).toEqual([]);
  }
  expect(foreign).toEqual([]);
});
