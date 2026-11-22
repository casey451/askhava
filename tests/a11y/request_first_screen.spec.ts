/// <reference types="node" />
// The /request start screen's first screen (T831; A11Y-005, D-5502).
//
// Chrome does not scroll a partly visible control into view when Tab reaches it, so a control cut by
// the fold would take focus with its box partly outside the viewport (A11Y-005). At 900 px tall and
// each width the keyboard journeys and Ally use, every control of the chat form (the words box,
// "Send") is wholly inside the first screen or wholly below it, measured as the keyboard tests
// measure (outside by more than half a pixel counts, tests/a11y/j2_keyboard.spec.ts); at 1024 and
// 1280 the words box is wholly inside it. Then, as J3 does, Tab runs from the top of the page to
// `#chat-words` at 1280 by 900 and its box must lie inside the viewport: with AI-026's line (always
// on outside Live, D-4120), with the WEB-007 banner shown, and with the safety-call consent shown.
// The banner and the safety-call wording are put into the served page in the browser, taken from
// app/site/templates/site_layout.html and the `consent_safety_call` templates, so no shared setting
// changes under the rest of the suite. Fake data only: the page holds no personal data.
import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";
import { expect, open, test } from "./fixtures";

/** The widths the keyboard journeys and Ally use (A11Y-005). */
const WIDTHS = [320, 768, 1024, 1280] as const;
/** The widths where the words box must sit wholly inside the first screen. */
const BOX_INSIDE = new Set<number>([1024, 1280]);
const HEIGHT = 900;

/** The WEB-007 banner's markup as site_layout.html writes it. */
const BANNER = /<section class="site-banner"[\s\S]*?<\/section>/.exec(readFileSync("app/site/templates/site_layout.html", "utf8"))?.[0] ?? "";

interface Templates { consent: { text: string }; consent_safety_call: { text: string } }

/** The sentence the safety-call consent adds to the plain one, and the text it goes before. */
function safetyInsert(file: string): { insert: string; before: string } {
  const all = JSON.parse(readFileSync(file, "utf8")) as Templates;
  const plain = all.consent.text;
  const call = all.consent_safety_call.text;
  let head = 0;
  while (plain[head] === call[head]) head += 1;
  let tail = 0;
  while (plain.at(-1 - tail) === call.at(-1 - tail)) tail += 1;
  const before = plain.slice(plain.length - tail, plain.length - tail + 12);
  return { insert: call.slice(head, call.length - tail), before };
}

const SAFETY = [
  { at: '[data-fixed="WEB-014"]', ...safetyInsert("app/messaging/templates/en.json") },
  { at: '[lang="es"] > p:last-child', ...safetyInsert("app/messaging/templates/es.json") },
];

interface Box { id: string; top: number; bottom: number }

/** The chat form's visible controls and their boxes, at the top of the page. */
async function controls(page: Page): Promise<Box[]> {
  return page.locator("form.form-card").evaluate((form) =>
    [...form.querySelectorAll<HTMLElement>("textarea, button, input:not([type=hidden])")].map((el) => {
      const r = el.getBoundingClientRect();
      return { id: el.id || el.textContent.trim(), top: r.top, bottom: r.bottom };
    }),
  );
}

/** Tabs from the top of the page until `#chat-words` has focus and returns its box. */
async function tabToWords(page: Page): Promise<{ top: number; bottom: number; left: number; right: number }> {
  for (let stop = 0; stop < 40; stop += 1) {
    await page.keyboard.press("Tab");
    const box = await page.evaluate(() => {
      const el = document.activeElement;
      if (el?.id !== "chat-words") return null;
      const r = el.getBoundingClientRect();
      return { top: r.top, bottom: r.bottom, left: r.left, right: r.right };
    });
    if (box !== null) return box;
  }
  throw new Error("Tab never reached #chat-words");
}

/** A11Y-005: the focused words box lies inside the viewport (half a pixel's grace, as J2). */
async function expectWordsInView(page: Page, width: number): Promise<void> {
  const box = await tabToWords(page);
  expect(box.top, "words box top").toBeGreaterThanOrEqual(-0.5);
  expect(box.left, "words box left").toBeGreaterThanOrEqual(-0.5);
  expect(box.bottom, "words box bottom").toBeLessThanOrEqual(HEIGHT + 0.5);
  expect(box.right, "words box right").toBeLessThanOrEqual(width + 0.5);
}

for (const width of WIDTHS) {
  test(`A11Y-005: at ${String(width)} by ${String(HEIGHT)} each /request control is wholly inside or wholly below the first screen`, async ({ page }) => {
    await open(page, "/request", width, HEIGHT);
    const boxes = await controls(page);
    expect(boxes.map((b) => b.id)).toContain("chat-words");
    for (const b of boxes) {
      const inside = b.bottom <= HEIGHT + 0.5;
      const below = b.top >= HEIGHT - 0.5;
      expect(inside || below, `${b.id} spans the fold (${String(b.top)} to ${String(b.bottom)})`).toBe(true);
      if (b.id === "chat-words" && BOX_INSIDE.has(width)) expect(inside, `${b.id} inside the first screen`).toBe(true);
    }
  });
}

test("A11Y-005 AI-026: Tab reaches #chat-words at 1280 by 900 with its box inside the viewport", async ({ page }) => {
  await open(page, "/request", 1280, HEIGHT);
  await expect(page.locator('[data-fixed="AI-026"]')).toBeVisible();
  await expectWordsInView(page, 1280);
});

test("A11Y-005 WEB-007: with the banner shown, Tab reaches #chat-words at 1280 by 900 inside the viewport", async ({ page }) => {
  expect(BANNER).toContain("site-banner");
  await open(page, "/request", 1280, HEIGHT);
  await page.locator("body > header").evaluate((header, html) => { header.insertAdjacentHTML("beforebegin", html); }, BANNER);
  await expect(page.locator(".site-banner")).toBeVisible();
  await expectWordsInView(page, 1280);
});

test("A11Y-005 CON-025: with the safety-call consent shown, Tab reaches #chat-words at 1280 by 900 inside the viewport", async ({ page }) => {
  await open(page, "/request", 1280, HEIGHT);
  for (const { at, insert, before } of SAFETY) {
    const p = page.locator(at);
    expect(insert.length).toBeGreaterThan(40);
    await expect(p).toContainText(before);
    await p.evaluate((el, add) => {
      el.textContent = el.textContent.replace(add.before, `${add.insert}${add.before}`);
    }, { insert, before });
    await expect(p).toContainText(insert.trim());
  }
  await expectWordsInView(page, 1280);
});
