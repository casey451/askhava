// The legal and data pages served through the whole app with JavaScript off (T221; TD-WEB-01,
// TD-PRIV-06, TD-TERMS-04, TD-WEB-05; WEB-014, TERMS-021, TERMS-023, AI-018, WORD-002; D-3144,
// D-3844).
//
// /terms holds the homeowner terms then the business terms, each its own section; /privacy and
// /sms-terms show their drafts; /your-data and /business-data exist in English and, under /es/, in
// Spanish. Every page carries the not-a-contractor statement in its language. Each data page holds
// exactly one of AI-018's wordings: which one follows the AI switch in the suite's environment, so
// both branches are rendered and checked in app/site/src/legal_pages.rs. The consent text's links
// (/terms, /privacy) and /sms-terms answer 200. The brand is read from the brand file. No data is
// seeded.
import { readFileSync } from "node:fs";
import { test, expect } from "./fixtures";

const BRAND = (JSON.parse(readFileSync("app/core/data/brand.json", "utf8")) as { display_name: string }).display_name;
const STATEMENT = `${BRAND} is not a contractor and does not perform, bid on or supervise work.`;
const ESTATEMENT = `${BRAND} no es contratista y no realiza, cotiza ni supervisa trabajos.`;
const AI_OFF = "Our texts are automated. When it's switched on, an AI service helps word our questions";
const AI_ON = "To sort your request and help word our questions, an AI service from Anthropic";

test.use({ javaScriptEnabled: false });

test("TD-WEB-01 WEB-014 WORD-002: the legal pages render their drafts without JavaScript", async ({ page }) => {
  const pages: [string, string][] = [["/terms", "Terms"], ["/privacy", `${BRAND} Privacy Policy`], ["/sms-terms", `${BRAND} SMS Terms`]];
  for (const [path, h1] of pages) {
    expect((await page.goto(path))?.status(), path).toBe(200);
    expect(await page.locator("html").getAttribute("lang")).toBe("en");
    await expect(page.locator("h1")).toHaveText(h1);
    await expect(page.locator("script")).toHaveCount(0);
    await expect(page.locator("main")).toContainText("Draft prepared with AI research; not reviewed by a lawyer.");
    await expect(page.locator("footer")).toContainText(STATEMENT);
  }
});

test("TD-WEB-01 D-3144: /terms holds the homeowner terms, then the business terms", async ({ page }) => {
  await page.goto("/terms");
  const ids = await page.locator("main section").evaluateAll((all) => all.map((s) => s.id));
  expect(ids).toEqual(["homeowner-terms", "business-terms"]);
  await expect(page.locator("#homeowner-terms h2").first()).toHaveText(`${BRAND} Homeowner Terms`);
  await expect(page.locator("#business-terms h2").first()).toHaveText(`${BRAND} Business Terms`);
  await expect(page.locator("main .legal-version")).toHaveCount(2);
});

test("TD-PRIV-06 TD-WEB-05 TD-TERMS-04 TERMS-021 TERMS-023 AI-018: the data pages in English and Spanish", async ({ page }) => {
  const pages: [string, string, string, string][] = [
    ["/your-data", "en", "Your data", STATEMENT],
    ["/es/your-data", "es", "Sus datos", ESTATEMENT],
    ["/business-data", "en", "Business data", STATEMENT],
    ["/es/business-data", "es", "Datos de negocios", ESTATEMENT],
  ];
  for (const [path, lang, h1, statement] of pages) {
    expect((await page.goto(path))?.status(), path).toBe(200);
    expect(await page.locator("html").getAttribute("lang"), path).toBe(lang);
    await expect(page.locator("h1")).toHaveText(h1);
    await expect(page.locator("footer")).toContainText(statement);
    const main = (await page.locator("main").textContent()) ?? "";
    expect(main.toLowerCase()).not.toContain("ai-powered");
    if (lang === "en") expect([main.includes(AI_OFF), main.includes(AI_ON)].filter(Boolean), path).toHaveLength(1);
  }
  await page.goto("/business-data");
  await expect(page.locator("main")).toContainText("We'll tell you about any legal demand for your business's data unless the law forbids it.");
  await expect(page.locator("main")).toContainText("call logs hold no recordings.");
});

test("TD-WEB-01 WEB-014: the consent text's links resolve", async ({ request }) => {
  for (const path of ["/terms", "/privacy", "/sms-terms"]) expect((await request.get(path)).status(), path).toBe(200);
});
