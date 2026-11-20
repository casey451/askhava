/// <reference types="node" />
// The site stylesheet's own limits (T613; WEB-029, WEB-030, A11Y-009; DESIGN_SPEC section 7
// `:128` and `:129`; D-4246, D-4252, D-4807).
//
// The served /static/site/site.css is read as text and fails when it is over 60 KB as bytes or
// over 25 KB through Node's zlib.gzipSync; when a @keyframes changes any property but `transform`;
// when an `animation` or `transition` (other than `none`) sits outside a
// `prefers-reduced-motion: no-preference` block; when `@view-transition` appears; when a transition
// (delay plus duration) lasts 350 ms or more; when an animation's end time (delay plus duration
// times iterations) passes 900 ms, with the largest delay and iteration count anywhere in the
// sheet, each `var()` resolved to its largest declared value; when a delay alone is 350 ms or
// more (transition) or over 900 ms (animation); when `calc(` appears in an animation or transition
// value; when the `rise` stagger has no `:nth-child(n+6)` cap; when an iteration count is `infinite` or above 3; or when `linear-gradient(` appears. Each rule
// has a planted stylesheet string that fails it. The italic accent's font answers 200 as
// font/woff2 with the bytes of docs/design/fonts/, and the home page's first view at 390 by 844 (every response body until the load
// event, decoded) is 280 KB or less, with a planted list of sizes that fails the same sum.
import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import { test, expect } from "./fixtures";

/** One declaration and the preludes (selectors and at-rules) around it, outermost first. */
interface Decl { prop: string; value: string; context: string[] }

const KB = 1024;

/** WEB-030: the stylesheet is 60 KB or less as bytes and 25 KB or less gzipped. */
function sizeProblems(css: string): string[] {
  const bytes = Buffer.byteLength(css);
  const gzipped = gzipSync(Buffer.from(css)).length;
  return [
    ...(bytes > 60 * KB ? [`${String(bytes)} bytes, over 60 KB as bytes`] : []),
    ...(gzipped > 25 * KB ? [`${String(gzipped)} bytes gzipped, over 25 KB gzipped`] : []),
  ];
}

/** WEB-030: the first view's response bodies total 280 KB or less. */
function weightProblems(sizes: number[]): string[] {
  const total = sizes.reduce((sum, size) => sum + size, 0);
  return total > 280 * KB ? [`first view ${String(total)} bytes, over 280 KB`] : [];
}

/** Every declaration in `css` (comments dropped) with its enclosing preludes. */
function declarations(css: string): Decl[] {
  const out: Decl[] = [];
  const stack: string[] = [];
  let buf = "";
  for (const ch of css.replace(/\/\*[\s\S]*?\*\//g, "")) {
    if (ch === "{") {
      stack.push(buf.trim().replace(/\s+/g, " "));
      buf = "";
    } else if (ch === ";" || ch === "}") {
      const at = buf.indexOf(":");
      const prop = buf.slice(0, at).trim().toLowerCase();
      if (at > 0 && /^-?-?[a-z][a-z-]*$/.test(prop)) out.push({ prop, value: buf.slice(at + 1).trim(), context: [...stack] });
      buf = "";
      if (ch === "}") stack.pop();
    } else {
      buf += ch;
    }
  }
  return out;
}

/**
 * `value` with each `var(--name)` replaced by the custom property's largest declared value (the
 * longest time, then the highest count), since any rule declaring it may reach the element.
 */
function resolve(value: string, decls: Decl[]): string {
  const weight = (v: string): [number, number] => [Math.max(0, ...times(v)), iterations(v)];
  const largest = (name: string): string => decls.filter((d) => d.prop === name).map((d) => d.value)
    .reduce((best, v) => {
      const [a, b] = [weight(v), weight(best)];
      return a[0] > b[0] || (a[0] === b[0] && a[1] > b[1]) ? v : best;
    }, "0ms");
  let out = value;
  for (let round = 0; round < 5 && out.includes("var("); round += 1) {
    out = out.replace(/var\((--[a-z0-9-]+)\)/g, (_, name: string) => largest(name));
  }
  return out;
}

/** `value` split at the commas outside parentheses. */
function splitTop(value: string): string[] {
  const items: string[] = [];
  let depth = 0;
  let item = "";
  for (const ch of value) {
    depth += ch === "(" ? 1 : ch === ")" ? -1 : 0;
    if (ch === "," && depth === 0) {
      items.push(item.trim());
      item = "";
    } else {
      item += ch;
    }
  }
  return [...items, item.trim()];
}

/** The times in `item`, in ms, in order (duration first, then delay). */
function times(item: string): number[] {
  return [...item.matchAll(/(?<![\w.%-])(\d*\.?\d+)(ms|s)\b/g)]
    .map((m) => parseFloat(m[1] ?? "0") * (m[2] === "s" ? 1000 : 1));
}

/** The iteration count in an animation shorthand item or count value (1 when none is given). */
function iterations(item: string): number {
  let bare = item;
  while (/\([^()]*\)/.test(bare)) bare = bare.replace(/\([^()]*\)/g, "");
  const words = bare.replace(/!important/, "").split(/[\s,]+/);
  if (words.includes("infinite")) return Infinity;
  return Math.max(1, ...words.filter((w) => /^\d*\.?\d+$/.test(w)).map(Number));
}

const isNone = (value: string): boolean => value.replace(/!important/, "").trim() === "none";
const selectorOf = (d: Decl): string => [...d.context].reverse().find((c) => !c.startsWith("@")) ?? "";
const moving = (d: Decl): boolean => d.context.some((c) => /prefers-reduced-motion:\s*no-preference/.test(c));

/** WEB-029: keyframes move by transform alone; animations and transitions sit in no-preference. */
function placeProblems(decls: Decl[]): string[] {
  return decls.flatMap((d) => {
    const frames = d.context.find((c) => c.startsWith("@keyframes"));
    if (frames !== undefined && d.prop !== "transform") return [`${frames} changes ${d.prop}`];
    if (!/^(animation|transition)(-|$)/.test(d.prop) || isNone(d.value) || moving(d)) return [];
    return [`${d.prop} at ${selectorOf(d)} outside prefers-reduced-motion: no-preference`];
  });
}

/** WEB-029: an animation or transition value uses no `calc(`, whose time these checks cannot read. */
function calcProblems(decls: Decl[]): string[] {
  return decls.filter((d) => /^(animation|transition)(-|$)/.test(d.prop) && resolve(d.value, decls).includes("calc("))
    .map((d) => `${d.prop} at ${selectorOf(d)} uses calc(`);
}

/**
 * The delays (ms) anywhere in the sheet for `kind` (`transition` or `animation`): every `-delay`
 * longhand and every shorthand item's second time. Taken sheet-wide, since a selector list, a
 * broader rule or a narrower one can each add a delay to an element.
 */
function sheetDelays(decls: Decl[], kind: string): number[] {
  return decls.filter((x) => (x.prop === `${kind}-delay` || x.prop === kind) && !isNone(x.value)).flatMap((x) => {
    const items = splitTop(resolve(x.value, decls));
    return x.prop === kind ? items.map((item) => times(item)[1] ?? 0) : items.flatMap(times);
  });
}

/**
 * WEB-029: every transition (delay plus duration) is under 350 ms. The shorthand and the
 * `transition-duration` longhand each count, with the largest delay anywhere in the sheet; a
 * delay alone at 350 ms or more fails too.
 */
function transitionProblems(decls: Decl[]): string[] {
  const delay = Math.max(0, ...sheetDelays(decls, "transition"));
  const lone = delay >= 350 ? [`transition-delay ${String(delay)} ms is 350 ms or more`] : [];
  return [...lone, ...decls.filter((d) => /^transition(-duration)?$/.test(d.prop) && !isNone(d.value)).flatMap((d) =>
    splitTop(resolve(d.value, decls)).map((item) => (times(item)[0] ?? 0) + delay)
      .filter((ms) => ms >= 350).map((ms) => `transition at ${selectorOf(d)} lasts ${String(ms)} ms`))];
}

/**
 * WEB-029: no animation repeats more than 3 times, and each ends within 900 ms of load. The
 * shorthand and the `animation-duration` longhand each count, with the largest delay and
 * iteration count anywhere in the sheet (longhands and shorthand items alike); a delay alone over
 * 900 ms fails too.
 */
function animationProblems(decls: Decl[]): string[] {
  const counts = decls.filter((d) => /^animation(-iteration-count)?$/.test(d.prop) && !isNone(d.value))
    .flatMap((d) => splitTop(resolve(d.value, decls)).map(iterations));
  const out = counts.filter((n) => n > 3).map((n) => `animation repeats ${String(n)} times`);
  const delay = Math.max(0, ...sheetDelays(decls, "animation"));
  if (delay > 900) out.push(`animation-delay ${String(delay)} ms ends after 900 ms`);
  for (const d of decls.filter((x) => /^animation(-duration)?$/.test(x.prop) && !isNone(x.value))) {
    const selector = selectorOf(d);
    for (const item of splitTop(resolve(d.value, decls))) {
      const count = Math.max(iterations(item), ...counts);
      if (count > 3) out.push(`animation at ${selector} repeats ${String(count)} times`);
      const end = delay + (times(item)[0] ?? 0) * count;
      if (end > 900) out.push(`animation at ${selector} ends at ${String(end)} ms`);
    }
  }
  return out;
}

/** D-4252: a `rise` stagger is capped, the sixth card and every later one sharing one delay. */
function capProblems(decls: Decl[]): string[] {
  const staggered = decls.filter((d) => d.prop === "animation-delay" && selectorOf(d).includes(".rise:nth-child("));
  if (staggered.length === 0) return [];
  const capped = staggered.some((d) => /\.rise:nth-child\(n\s*\+\s*6\)/.test(selectorOf(d)));
  return capped ? [] : ["the rise delays have no :nth-child(n+6) cap"];
}

/** Every motion, depth and gradient rule of WEB-029 and D-4246 the stylesheet text breaks. */
function motionProblems(css: string): string[] {
  const decls = declarations(css);
  return [
    ...placeProblems(decls),
    ...calcProblems(decls),
    ...transitionProblems(decls),
    ...animationProblems(decls),
    ...capProblems(decls),
    ...(css.includes("@view-transition") ? ["@view-transition appears"] : []),
    ...(css.includes("linear-gradient(") ? ["linear-gradient( appears"] : []),
  ];
}

/** `rules` inside a prefers-reduced-motion: no-preference block. */
const inMotion = (rules: string): string => `\n@media (prefers-reduced-motion: no-preference) { ${rules} }\n`;

test("WEB-030 WEB-029 D-4252: the served site.css keeps its size, motion and gradient limits", async ({ request }) => {
  const reply = await request.get("/static/site/site.css");
  expect(reply.status()).toBe(200);
  const css = await reply.text();
  expect(sizeProblems(css)).toEqual([]);
  expect(motionProblems(css)).toEqual([]);
  expect(css).toContain(".rise:nth-child(n+6)");
});

test("WEB-030 D-4252: a planted stylesheet over 60 KB as bytes, or over 25 KB gzipped, fails", async ({ request }) => {
  const css = await (await request.get("/static/site/site.css")).text();
  expect(sizeProblems(`${css}/*${"a".repeat(61 * KB)}*/`)).toEqual([expect.stringContaining("over 60 KB as bytes")]);
  expect(sizeProblems(`/*${randomBytes(36 * KB).toString("base64")}*/`)).toEqual([expect.stringContaining("over 25 KB gzipped")]);
});

test("WEB-029 A11Y-009 D-4246 D-4252: each planted motion or gradient rule fails for its reason", async ({ request }) => {
  const css = await (await request.get("/static/site/site.css")).text();
  const planted: [string, string][] = [
    [inMotion("@keyframes fade { from { opacity: 0; } }"), "@keyframes fade changes opacity"],
    [".x { transition: color 100ms ease; }", "transition at .x outside prefers-reduced-motion: no-preference"],
    ["@view-transition { navigation: auto; }", "@view-transition appears"],
    [inMotion(".x { transition: transform 400ms ease; }"), "transition at .x lasts 400 ms"],
    [inMotion(".pz-t { transition: transform 200ms; transition-delay: 300ms; }"), "transition at .pz-t lasts 500 ms"],
    [inMotion(".pz-d { transition-property: transform; transition-duration: 360ms; }"), "transition at .pz-d lasts 360 ms"],
    [inMotion(".pz-a { animation-name: rise; animation-duration: 2000ms; }"), "animation at .pz-a ends at 2360 ms"],
    [inMotion(".pz-b { animation: rise 600ms; animation-delay: 400ms; }"), "animation at .pz-b ends at 1000 ms"],
    [inMotion(".pz-c { animation-name: rise; animation-duration: 400ms; animation-iteration-count: 3; }"),
      "animation at .pz-c ends at 1560 ms"],
    [inMotion(".rise:nth-child(n+6) { animation-delay: 500ms; }"), "animation at .rise ends at 950 ms"],
    [inMotion(".y { animation: rise 200ms infinite; }"), "animation at .y repeats Infinity times"],
    [inMotion(".y { animation: rise 100ms 4; }"), "animation at .y repeats 4 times"],
    [inMotion(".button { transition-delay: 200ms; }"), "transition at button, .button lasts 420 ms"],
    [inMotion(".pz-f { transition-delay: 400ms; }"), "transition-delay 400 ms is 350 ms or more"],
    [inMotion(".a, .b { animation: rise 500ms; } .b { animation-delay: 600ms; }"), "animation at .a, .b ends at 1100 ms"],
    [inMotion(".card { animation-delay: 800ms; } .card.x { animation: rise 300ms; }"), "animation at .card.x ends at 1100 ms"],
    [inMotion(".pz-g { animation-name: rise; animation-delay: 1200ms; }"), "animation-delay 1200 ms ends after 900 ms"],
    [inMotion(".pz-h { animation-iteration-count: 3; } .pz-h.x { animation: rise 400ms; }"),
      "animation at .pz-h.x ends at 1560 ms"],
    [inMotion(".pz-i { animation: rise 100ms 3; } .pz-i.x { animation-duration: 400ms; }"),
      "animation at .pz-i.x ends at 1560 ms"],
    [`.pz-j { --pz-d: 100ms; } .pz-k { --pz-d: 2000ms; }${inMotion(".pz-l { animation: rise var(--pz-d); }")}`,
      "animation at .pz-l ends at 2360 ms"],
    [inMotion(".pz-m { animation: rise 300ms; animation-delay: calc(8 * 90ms); }"), "animation-delay at .pz-m uses calc("],
    [inMotion(".pz-n { --pz-i: 3; transition: transform calc(var(--pz-i) * 90ms); }"), "transition at .pz-n uses calc("],
    [".z { background: linear-gradient(white, white); }", "linear-gradient( appears"],
  ];
  for (const [rules, reason] of planted) expect(motionProblems(css + rules), reason).toContain(reason);
  const uncapped = css.replace(".rise:nth-child(n+6)", ".rise:nth-child(6)");
  expect(motionProblems(uncapped)).toEqual(["the rise delays have no :nth-child(n+6) cap"]);
});

test("WEB-027 D-4246: the italic accent's font is served from the site as font/woff2", async ({ request }) => {
  const font = await request.get("/static/site/fonts/gelasio-latin-500-italic.woff2");
  expect(font.status()).toBe(200);
  expect(font.headers()["content-type"]).toBe("font/woff2");
  const reviewed = readFileSync(join(import.meta.dirname, "..", "..", "docs/design/fonts/gelasio-latin-500-italic.woff2"));
  expect((await font.body()).equals(reviewed)).toBe(true);
});

test("WEB-030 D-4252: the home page's first view at 390 by 844 is 280 KB or less", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const bodies: Promise<number>[] = [];
  const watch = (reply: { body: () => Promise<Buffer> }): void => {
    bodies.push(reply.body().then((b) => b.length, () => 0));
  };
  page.on("response", watch);
  await page.goto("/", { waitUntil: "load" });
  page.off("response", watch);
  const sizes = await Promise.all(bodies);
  expect(sizes.length).toBeGreaterThan(0);
  expect(weightProblems(sizes)).toEqual([]);
  expect(weightProblems([200 * KB, 60 * KB, 21 * KB])).toEqual([`first view ${String(281 * KB)} bytes, over 280 KB`]);
});
