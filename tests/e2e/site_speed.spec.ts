/// <reference types="node" />
// WEB-029 and WEB-030 runtime checks (T612; DESIGN_SPEC `:47`, `:128`, `:129`, `:147`, `:152` to `:154`;
// D-3292, D-4246, D-4252, D-5864, D-5840). Every GET route of the `site` crate's router (read from
// app/site/src) has no `<script>` and requests none, and preloads Gelasio 500 and DM Sans 400 from
// /static/site/fonts/, each preload used. The hero photo has width, height, `fetchpriority="high"`, is
// not lazy, and each address it may load is 100 KB or less. On every page state, motion on, each CSS
// animation and transition is recorded as it starts (`animationstart` and `transitionrun` listeners
// before navigation, then `document.getAnimations()`): the quote page's change only `transform`, run at
// most 3 times and end within 900 ms; elsewhere nothing moves on load; home and quote buttons' transitions
// last under 350 ms; no `@view-transition`, one outermost italic at most and none in a quote card, form,
// button or link, no uppercase heading, label, button or link. Fonts blocked, the home first screen holds
// at 390x844 and 320x740; the Spanish home's h1 and first call to action fit 320x740 with no sideways
// scroll; with `contrast: more` no card edge is `line` and no box-shadow is left. A11Y-009's
// reduced-motion test stays in tests/a11y/reflow_spacing.spec.ts. Each check has a planted case. Fake data only.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { PAGE_STATES, STATES_TO_CHECK, expect, open, test } from "../a11y/fixtures";

/** One recorded animation or transition: its element and name, the properties it changes, its timing. */
interface Motion { what: string; props: string[]; iterations: number; end: number }
interface Recorder { motion: Motion[]; keepMotion: (a: Animation) => void }

const KB = 1024;
const QUOTE_ROUTE = "/q/{token}";
const SITE_SRC = join(import.meta.dirname, "..", "..", "app", "site", "src");

/** Every GET route in `text`, read as tests/a11y/fixtures.ts reads them: each `.route(` call holding `get(`,
 * its path a literal or a `const`; a path it can't read, or a `.route(` it misses, is an "unread" entry. */
function siteRoutes(text: string): string[] {
  const consts = new Map([...text.matchAll(/const ([A-Z0-9_]+): &str = "([^"]*)";/g)].map((m) => [m[1] ?? "", m[2] ?? ""]));
  const calls = [...text.matchAll(/\.route\(\s*([^,]+),([\s\S]*?)(?=\.route\(|\.route_service\(|\.fallback|\.nest|\.with_state|\.layer|;)/g)];
  const pathOf = (arg: string): string => /^"([^"]+)"$/.exec(arg.trim())?.[1] ?? consts.get(arg.trim()) ?? `unread ${arg.trim()}`;
  const missed = text.split(".route(").length - 1 - calls.length;
  const read = calls.map((m) => [pathOf(m[1] ?? ""), m[2] ?? ""] as const)
    .flatMap(([path, rest]) => (path.startsWith("unread") || /\bget\(/.test(rest) ? [path] : []));
  return [...new Set([...read, ...(missed === 0 ? [] : [`unread: ${String(missed)} .route( calls missed`])])];
}
const SITE_ROUTES = siteRoutes(readdirSync(SITE_SRC).filter((f) => f.endsWith(".rs")).map((f) => readFileSync(join(SITE_SRC, f), "utf8")).join("\n"));

/** Runs before the page's own code (D-5864): records each CSS animation and transition as it starts. */
function recordMotion(): void {
  const w = window as unknown as Recorder;
  const kept = new WeakSet<Animation>();
  const nameOf = (a: Animation): string => (a instanceof CSSAnimation ? a.animationName : a instanceof CSSTransition ? a.transitionProperty : a.id);
  w.motion = [];
  w.keepMotion = (a) => {
    if (kept.has(a)) return;
    kept.add(a);
    const effect = a.effect as KeyframeEffect | null;
    const timing = effect?.getComputedTiming();
    const keys = a instanceof CSSTransition ? [a.transitionProperty] : (effect?.getKeyframes() ?? []).flatMap((k) => Object.keys(k));
    w.motion.push({ what: `<${effect?.target?.tagName ?? "?"}${effect?.pseudoElement ?? ""}> ${nameOf(a)}`,
      props: [...new Set(keys)].filter((p) => !["offset", "computedOffset", "easing", "composite"].includes(p)),
      iterations: timing?.iterations ?? 0, end: Number(timing?.endTime ?? 0) });
  };
  const started = (e: Event): void => {
    const name = e instanceof AnimationEvent ? e.animationName : (e as TransitionEvent).propertyName;
    const found = document.getAnimations().find((a) => (a.effect as KeyframeEffect | null)?.target === e.target && nameOf(a) === name);
    if (found) w.keepMotion(found);
    else w.motion.push({ what: `<${(e.target as Element).tagName}> ${name}`, props: ["unread"], iterations: 0, end: 0 });
  };
  for (const kind of ["animationstart", "transitionrun"]) window.addEventListener(kind, started, true);
}

/** What moved since navigation, after two frames: the listeners' records and every animation left. */
async function motionSeen(page: Page): Promise<Motion[]> {
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  return page.evaluate(() => {
    const w = window as unknown as Recorder;
    for (const a of document.getAnimations()) w.keepMotion(a);
    return w.motion;
  });
}

/** WEB-029: movement by `transform` alone, at most 3 iterations, ended within 900 ms of load. */
function limitProblems(seen: Motion[]): string[] {
  return seen.flatMap((m) => [
    ...m.props.filter((p) => p !== "transform").map((p) => `${m.what} changes ${p}`),
    ...(m.iterations > 3 ? [`${m.what} repeats ${String(m.iterations)} times`] : []),
    ...(m.end > 900 ? [`${m.what} ends at ${String(m.end)} ms`] : []),
  ]);
}
/** WEB-029, D-4252: outside the quote page nothing moves on load. */
const loadProblems = (seen: Motion[]): string[] => seen.map((m) => `${m.what} moves on load`);
/** Runs in the page: each button whose transition (delay plus duration) lasts 350 ms or more. */
function transitionsOf(): string[] {
  const ms = (list: string): number[] => list.split(",").map((v) => parseFloat(v) * (v.trim().endsWith("ms") ? 1 : 1000));
  return [...document.querySelectorAll("button, .button")].flatMap((el) => {
    const style = getComputedStyle(el);
    const [durations, delays] = [ms(style.transitionDuration), ms(style.transitionDelay)];
    const longest = Math.max(...durations.map((d, i) => d + (delays[i % delays.length] ?? 0)));
    return longest >= 350 ? [`<${el.tagName}> "${el.textContent.trim()}" transition lasts ${String(longest)} ms`] : [];
  });
}

/** WEB-029: button transitions at rest, with the first one in `main` hovered, then pressed (no click). */
async function buttonProblems(page: Page): Promise<string[]> {
  const out = await page.evaluate(transitionsOf);
  const first = page.locator("main button:visible, main .button:visible").first();
  if ((await first.count()) === 0) return out;
  await first.hover();
  out.push(...(await page.evaluate(transitionsOf)));
  await page.mouse.down();
  out.push(...(await page.evaluate(transitionsOf)));
  await page.mouse.move(0, 0);
  await page.mouse.up();
  return [...new Set(out)];
}

/** Runs in the page: the italic and uppercase rules of DESIGN_SPEC section 3 (D-4246, D-4252). */
function typeProblems(): string[] {
  const italic = (el: Element | null): boolean => el !== null && /italic|oblique/.test(getComputedStyle(el).fontStyle);
  const all = [...document.querySelectorAll("body *")].filter((el) => italic(el));
  const outer = all.filter((el) => !italic(el.parentElement));
  const out = outer.length > 1 ? [`${String(outer.length)} outermost italic elements`] : [];
  for (const el of all) if (el.closest(".quote, form, button, a")) out.push(`italic <${el.tagName}> inside a quote card, form, button or link`);
  const upper = [...document.querySelectorAll("h1, h2, h3, h4, h5, h6, label, button, a")].filter((el) => getComputedStyle(el).textTransform === "uppercase");
  return [...out, ...upper.map((el) => `<${el.tagName}> is uppercase`)];
}

/** WEB-029: no stylesheet the page loads or holds has `@view-transition` (read as text). */
async function sheetProblems(page: Page): Promise<string[]> {
  const { hrefs, inline } = await page.evaluate(() => ({ inline: [...document.querySelectorAll("style")].map((s) => s.textContent),
    hrefs: [...document.querySelectorAll<HTMLLinkElement>('link[rel~="stylesheet"]')].map((l) => l.href) }));
  const texts = [...inline, ...(await Promise.all(hrefs.map(async (h) => (await page.request.get(h)).text())))];
  return texts.some((t) => t.includes("@view-transition")) ? ["a stylesheet holds @view-transition"] : [];
}

/** WEB-030: every script request the page makes, as it makes it. */
function watchScripts(page: Page): string[] {
  const seen: string[] = [];
  page.on("request", (r) => { if (r.resourceType() === "script") seen.push(`requests script ${new URL(r.url()).pathname}`); });
  return seen;
}
/** Runs in the page: each `<script>` element. */
const scriptElements = (): string[] => [...document.querySelectorAll("script")].map((s) => `<script> ${s.src}`);

/** Runs in the page (WEB-030): the font preloads, each matched to its @font-face rules and loaded faces. */
async function fontProblems(): Promise<string[]> {
  await document.fonts.ready;
  const bare = (s: string): string => s.replace(/["']/g, "").trim();
  const rules = [...document.styleSheets].flatMap((s) => [...s.cssRules]).filter((r): r is CSSFontFaceRule => r instanceof CSSFontFaceRule);
  const facesAt = (href: string): string[] => rules.filter((r) => [...r.style.getPropertyValue("src").matchAll(/url\(["']?([^"')]+)/g)]
    .some((m) => new URL(m[1] ?? "", r.parentStyleSheet?.href ?? location.href).href === href))
    .map((r) => `${bare(r.style.getPropertyValue("font-family"))} ${r.style.getPropertyValue("font-style") || "normal"} ${r.style.getPropertyValue("font-weight")}`);
  const loaded = new Set([...document.fonts].filter((f) => f.status === "loaded").map((f) => `${bare(f.family)} ${f.style} ${f.weight}`));
  const [out, preloaded]: [string[], string[]] = [[], []];
  for (const link of document.querySelectorAll<HTMLLinkElement>('link[rel~="preload"][as="font"]')) {
    const url = new URL(link.href);
    const faces = facesAt(url.href);
    if (url.origin !== location.origin || !url.pathname.startsWith("/static/site/fonts/")) out.push(`${url.pathname} is not preloaded from /static/site/fonts/`);
    if (!faces.some((f) => loaded.has(f))) out.push(`${url.pathname} is preloaded but unused`);
    preloaded.push(...faces);
  }
  for (const face of ["Gelasio normal 500", "DM Sans normal 400"]) if (!preloaded.includes(face)) out.push(`${face} is not preloaded`);
  return out;
}

/** Runs in the page (WEB-030): the hero photo's attributes, and every address it may load. */
function heroOf(): { problems: string[]; urls: string[] } {
  const img = document.querySelector<HTMLImageElement>("main img");
  if (img === null) return { problems: ["no hero photo"], urls: [] };
  const problems = ["width", "height"].filter((a) => !img.hasAttribute(a)).map((a) => `the hero photo has no ${a}`);
  if (img.getAttribute("fetchpriority") !== "high") problems.push('the hero photo lacks fetchpriority="high"');
  if (img.loading === "lazy") problems.push("the hero photo is lazy");
  const sets = [img, ...(img.closest("picture")?.querySelectorAll("source") ?? [])].map((e) => e.getAttribute("srcset") ?? "");
  const urls = [img.getAttribute("src") ?? "", ...sets.flatMap((s) => s.split(",").map((c) => c.trim().split(/\s+/)[0] ?? ""))];
  return { problems, urls: [...new Set(urls.filter((u) => u !== "").map((u) => new URL(u, location.href).href))] };
}
/** WEB-030: each hero address over 100 KB. */
const heavyProblems = (sizes: [string, number][]): string[] =>
  sizes.filter(([, bytes]) => bytes > 100 * KB).map(([url, bytes]) => `${new URL(url).pathname} is ${String(bytes)} bytes, over 100 KB`);

/** WEB-030: the hero photo's attribute faults and each of its addresses over 100 KB. */
async function heroProblems(page: Page): Promise<string[]> {
  const { problems, urls } = await page.evaluate(heroOf);
  const sizes = await Promise.all(urls.map(async (u): Promise<[string, number]> => [u, (await (await page.request.get(u)).body()).length]));
  return [...problems, ...heavyProblems(sizes)];
}

/** Runs in the page (`:147`): the boxes, by name, that leave the first `height` px or have none. */
function offScreen([boxes, height]: readonly [[string, string][], number]): string[] {
  const out = boxes.filter(([, selector]) => {
    const r = document.querySelector(selector)?.getBoundingClientRect();
    return r === undefined || r.height < 1 || r.top < 0 || r.bottom > height;
  }).map(([name]) => `${name} is not on the first screen`);
  const root = document.documentElement;
  return root.scrollWidth > root.clientWidth ? [...out, "the page scrolls sideways"] : out;
}
const HOME_BOXES: [string, string][] = [["the photo", "main picture img"], ["Get quotes", "main form button[type=submit]"]];
const ES_BOXES: [string, string][] = [["the h1", "main h1"], ["the first call to action", "main :is(a, button)"]];

/** Runs in the page (`:47`, D-4252): with `contrast: more`, card edges still `line` and box-shadows left. */
function contrastProblems(line: string): string[] {
  const lined = [...document.querySelectorAll(".card, article")].filter((el) =>
    ["top", "right", "bottom", "left"].some((side) => getComputedStyle(el).getPropertyValue(`border-${side}-color`) === line));
  return [...lined.map((el) => `<${el.tagName}> edge is still line`), ...[...document.querySelectorAll("*")].flatMap((el) => [null, "::before", "::after"]
    .filter((pseudo) => getComputedStyle(el, pseudo).boxShadow !== "none").map((pseudo) => `<${el.tagName}>${pseudo ?? ""} keeps a box-shadow`))];
}

/** Serves `html` at /__planted on the app's origin (so its relative addresses load) and opens it. */
async function plant(page: Page, html: string): Promise<void> {
  await page.unrouteAll();
  await page.route("**/__planted", (r) => r.fulfill({ contentType: "text/html", body: html }));
  await page.goto("/__planted");
}
const QUOTES = PAGE_STATES.find((s) => s.name === "three quotes, #2 gone");

test("WEB-030: every .route( call of the site router is read, the home page among them; planted unread calls fail", () => {
  expect(SITE_ROUTES).toContain("/");
  expect(SITE_ROUTES.filter((r) => r.startsWith("unread"))).toEqual([]);
  expect(siteRoutes('const A: &str = "/a";\n.route(A, post(x).get(y)).route(B, post(z)).route("/c",\n axum::routing::get(w));'))
    .toEqual(["/a", "unread B", "/c"]);
  expect(siteRoutes(".route(only)")).toEqual(["unread: 1 .route( calls missed"]);
});

for (const route of SITE_ROUTES) {
  test(`WEB-030: ${route} loads no script and preloads the two first-screen fonts, each used`, async ({ page }) => {
    const scripts = watchScripts(page);
    await page.goto(route);
    expect([...(await page.evaluate(scriptElements)), ...scripts]).toEqual([]);
    expect(await page.evaluate(fontProblems)).toEqual([]);
  });
}

test("WEB-030: planted script elements, script requests and font preloads fail", async ({ page }) => {
  let scripts = watchScripts(page);
  await plant(page, '<!doctype html><title>Planted</title><script src="/static/site/planted.js"></script>');
  expect([...(await page.evaluate(scriptElements)), ...scripts]).toEqual([expect.stringContaining("<script>"), "requests script /static/site/planted.js"]);
  scripts = watchScripts(page);
  await plant(page, '<!doctype html><title>Planted</title><link rel="preload" as="script" href="/static/site/other.js">');
  expect(scripts).toContain("requests script /static/site/other.js");
  const home = await (await page.request.get("/")).text();
  const dropped = home.replace(/<link rel="preload"[^>]*dm-sans[^>]*>/, "")
    .replace("</head>", '<link rel="preload" as="font" type="font/woff2" crossorigin href="/static/site/fonts/dm-sans-latin-400-normal.woff2"></head>')
    .replace("</head>", '<link rel="preload" as="font" type="font/woff2" crossorigin href="/static/fonts/planted.woff2"></head>');
  await plant(page, dropped);
  expect(await page.evaluate(fontProblems)).toEqual([
    "/static/site/fonts/dm-sans-latin-400-normal.woff2 is preloaded but unused",
    "/static/fonts/planted.woff2 is not preloaded from /static/site/fonts/", "/static/fonts/planted.woff2 is preloaded but unused",
    "DM Sans normal 400 is not preloaded",
  ]);
});

test("WEB-030: the hero photo is 100 KB or less with width, height and high priority, never lazy; planted faults fail", async ({ page }) => {
  await page.goto("/");
  expect(await heroProblems(page)).toEqual([]);
  await plant(page, '<!doctype html><title>Planted</title><main><img src="/static/site/photos/hero-kitchen.webp" srcset="/static/site/brand/mark.svg 2x" loading="lazy" alt=""></main>');
  expect(await heroProblems(page)).toEqual(["the hero photo has no width", "the hero photo has no height", 'the hero photo lacks fetchpriority="high"', "the hero photo is lazy"]);
  expect(heavyProblems([["http://x.test/static/site/photos/big.webp", 101 * KB]])).toEqual([`/static/site/photos/big.webp is ${String(101 * KB)} bytes, over 100 KB`]);
});

for (const state of STATES_TO_CHECK) {
  const quote = state.route === QUOTE_ROUTE;
  test(`WEB-029: ${state.route} (${state.name}) keeps the motion, italic, uppercase and view-transition rules`, async ({ page, pageUrl }) => {
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await page.addInitScript(recordMotion);
    await open(page, pageUrl(state), 1280);
    const seen = await motionSeen(page);
    expect([...limitProblems(seen), ...(quote ? [] : loadProblems(seen))]).toEqual([]);
    if (state.name === "eight quotes") expect(seen.length, "the recorder sees the cards' entrance").toBeGreaterThan(0);
    expect(await page.evaluate(typeProblems)).toEqual([]);
    expect(await sheetProblems(page)).toEqual([]);
    if (quote || state.route === "/") expect(await buttonProblems(page)).toEqual([]);
  });
}

test("WEB-029: a planted page's animations, @starting-style entrance, slow button and type faults each fail", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.addInitScript(recordMotion);
  await plant(page, `<!doctype html><title>Planted</title><style>@keyframes fade { from { opacity: 0 } }
    @keyframes spin { from { transform: rotate(10deg) } } .fade { animation: fade 100ms } .spin { animation: spin 100ms 4 }
    .slow { animation: spin 950ms } @starting-style { h1 { opacity: 0; translate: 0 20px } }
    h1 { transition: opacity 400ms, translate 400ms } button { transition: color 400ms } em { font-style: italic }
    h2, label, a { text-transform: uppercase } @view-transition { navigation: auto; }</style>
    <main><h1>Planted</h1><h2>Head</h2><p class="fade">Fades</p><p class="spin">Spins</p><p class="slow">Slow</p>
    <p><em>One <a href="/">see</a></em></p><form><label>Name <em>Two</em></label><button>Planted</button></form><a href="/">Home</a></main>`);
  const seen = await motionSeen(page);
  expect(limitProblems(seen)).toEqual(expect.arrayContaining(["<P> fade changes opacity", "<P> spin repeats 4 times",
    "<P> spin ends at 950 ms", "<H1> opacity changes opacity", "<H1> translate changes translate"]));
  expect(loadProblems(seen)).toEqual(expect.arrayContaining(["<H1> opacity moves on load", "<P> fade moves on load"]));
  expect(await buttonProblems(page)).toEqual(['<BUTTON> "Planted" transition lasts 400 ms']);
  expect(await page.evaluate(typeProblems)).toEqual(["2 outermost italic elements", "italic <A> inside a quote card, form, button or link",
    "italic <EM> inside a quote card, form, button or link", "<H2> is uppercase", "<A> is uppercase", "<LABEL> is uppercase", "<A> is uppercase"]);
  expect(await sheetProblems(page)).toEqual(["a stylesheet holds @view-transition"]);
});

for (const [width, height] of [[390, 844], [320, 740]] as const) {
  test(`WEB-027 D-4252 D-5864: with every font blocked the home first screen holds at ${String(width)} by ${String(height)}`, async ({ page }) => {
    await page.route("**/static/site/fonts/**", (r) => r.abort());
    await page.setViewportSize({ width, height });
    await page.goto("/");
    await page.locator("main picture img").evaluate((img: HTMLImageElement) => img.decode());
    expect(await page.evaluate(offScreen, [HOME_BOXES, height] as const)).toEqual([]);
    await page.evaluate(() => document.querySelector("main")?.prepend(Object.assign(document.createElement("div"), { style: "height: 900px" })));
    expect(await page.evaluate(offScreen, [HOME_BOXES, height] as const)).toEqual(["the photo is not on the first screen", "Get quotes is not on the first screen"]);
  });
}

test("WEB-027 D-4252: the Spanish home's h1 and first call to action fit 320 by 740 with no sideways scroll", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 740 });
  await page.goto("/es");
  await page.evaluate(() => document.fonts.ready);
  expect(await page.evaluate(offScreen, [ES_BOXES, 740] as const)).toEqual([]);
  await page.evaluate(() => document.querySelector("main")?.prepend(Object.assign(document.createElement("div"), { style: "height: 900px; width: 400px" })));
  expect(await page.evaluate(offScreen, [ES_BOXES, 740] as const)).toEqual(["the h1 is not on the first screen",
    "the first call to action is not on the first screen", "the page scrolls sideways"]);
});

test("WEB-027 D-4252: with contrast more no card edge is line and no box-shadow is left; a planted one fails", async ({ page, pageUrl }) => {
  if (QUOTES === undefined) throw new Error("no three-quotes state");
  for (const url of ["/", pageUrl(QUOTES)]) {
    await page.emulateMedia({ contrast: "no-preference" });
    await open(page, url, 390);
    const line = await page.evaluate(() => getComputedStyle(document.body.appendChild(Object.assign(document.createElement("span"), { style: "color: var(--line)" }))).color);
    await page.emulateMedia({ contrast: "more" });
    expect(await page.evaluate(contrastProblems, line), url).toEqual([]);
    await page.evaluate((edge) => {
      const card = document.body.appendChild(Object.assign(document.createElement("div"), { className: "card" }));
      card.style.border = `1px solid ${edge}`;
      card.style.setProperty("box-shadow", "0 1px 2px black", "important");
    }, line);
    expect(await page.evaluate(contrastProblems, line)).toEqual(["<DIV> edge is still line", "<DIV> keeps a box-shadow"]);
  }
});
