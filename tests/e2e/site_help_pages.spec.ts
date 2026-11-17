// The services, coverage, help and ROC complaint pages served through the whole app with JavaScript
// off (T201; TD-WEB-01, TD-WORD-02, TD-WORD-03, TD-WEB-07, TD-TERMS-04, TD-WEB-06; WEB-008, WEB-011,
// WEB-015, WEB-023, WEB-025, WORD-009, AI-018; D-3878, D-3879, D-4044).
//
// The trades and their plurals are read from docs/catalog/categories.yaml, the catalog the web
// binary stores at start-up; the brand and its forbidden_alone word from the brand file, never
// typed. The help page shows exactly one of AI-018's wordings: which one follows the AI switch in
// the suite's environment. No data is seeded.
import { readFileSync } from "node:fs";
import { test, expect } from "./fixtures";

const BRAND_FILE = JSON.parse(readFileSync("app/core/data/brand.json", "utf8")) as { display_name: string; forbidden_alone: string[] };
const BRAND = BRAND_FILE.display_name;
const HAVA = BRAND_FILE.forbidden_alone[0] ?? "";
const STATEMENT = `${BRAND} is not a contractor and does not perform, bid on or supervise work. We pass along your request and show you quotes from independent local businesses, as they wrote them. You choose, and you hire the business directly.`;
const AI_OFF = "Our texts are automated. When it's switched on, an AI service helps word our questions";
const AI_ON = "To sort your request and help word our questions, an AI service from Anthropic";

/** A category's top-level `key` in the catalog file, its YAML quotes removed. */
function field(block: string, key: string): string {
  return (new RegExp(`\\n {4}${key}: (.+)`).exec(block)?.[1] ?? "").trim().replace(/^"(.*)"$/, "$1");
}

/** Each category of the catalog file: code, name, plural, switch and whether it needs a license. */
function catalog(): { code: string; name: string; plural: string; enabled: boolean; licensed: boolean }[] {
  const text = readFileSync("docs/catalog/categories.yaml", "utf8");
  return text.split(/\n {2}- id: /).slice(1).map((block) => ({
    code: block.split("\n")[0]?.trim() ?? "",
    name: field(block, "name"),
    plural: field(block, "plural"),
    enabled: /\n {4}enabled: true/.test(block),
    licensed: !/\n {4}license: \{kind: none/.test(block),
  }));
}

test.use({ javaScriptEnabled: false });

test("TD-WEB-01 TD-WORD-02 WEB-015: each licensed trade's section opens with the licensed line and the statement", async ({ page }) => {
  expect((await page.goto("/services"))?.status()).toBe(200);
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.locator("script")).toHaveCount(0);
  const licensed = catalog().filter((c) => c.enabled && c.licensed);
  expect(licensed.length).toBeGreaterThan(0);
  for (const trade of licensed) {
    const section = page.locator(`section#${trade.code}`);
    await expect(section.locator("h2")).toHaveText(trade.name);
    const lines = await section.locator("p").allTextContents();
    expect(lines.slice(0, 2), trade.code).toEqual([`Get quotes from licensed ${trade.plural}.`, STATEMENT]);
  }
});

test("TD-WEB-01 WEB-023: the coverage page lists open and not-yet trades and the Canadian-number line", async ({ page }) => {
  expect((await page.goto("/coverage"))?.status()).toBe(200);
  await expect(page.locator("h1")).toHaveText("What we cover now");
  const open = await page.locator("h2:has-text('Open now') + ul li").allTextContents();
  const enabled = catalog().filter((c) => c.enabled).map((c) => c.name);
  for (const name of enabled) expect(open, name).toContain(name);
  expect(open.some((name) => /pest/i.test(name))).toBe(enabled.some((name) => /pest/i.test(name)));
  await expect(page.locator("main")).toContainText("Canadian numbers work. Your phone company may charge roaming fees.");
});

test("TD-WEB-01 TD-WORD-02 TD-WORD-03 TD-WEB-07 TD-TERMS-04 WEB-008 WORD-009 AI-018: the help page's answers", async ({ page }) => {
  expect((await page.goto("/help"))?.status()).toBe(200);
  await expect(page.locator("script")).toHaveCount(0);
  const main = page.locator("main");
  await expect(main.locator("h2", { hasText: `Is ${BRAND} a person?` })).toHaveCount(1);
  await expect(main).toContainText(`${BRAND} is a local service. Our texts are automated.`);
  await expect(main).toContainText(`The ${BRAND} owner reads HELP messages and flagged messages, usually within 2 days, or within 24 hours if your job is urgent. Messages are not watched around the clock. In an emergency, call 911.`);
  await expect(main).not.toContainText("a real person reviews anything flagged");
  await expect(main).not.toContainText("AI-powered");
  const text = (await main.textContent()) ?? "";
  expect([text.includes(AI_OFF), text.includes(AI_ON)].filter(Boolean)).toHaveLength(1);
  const form = page.locator("form[method='post'][action='/help']");
  await expect(page.locator("h2#message")).toHaveText(`Send the ${BRAND} owner a message`);
  await expect(form.getByLabel("Your message")).toBeVisible();
  await expect(form.locator("input[type='hidden'][name='csrf']")).toHaveCount(1);
  await expect(form.getByRole("button", { name: "Send message" })).toBeEnabled();
});

test("TD-WEB-06 WEB-025 WEB-008: the help page's WEB-025 answer in English and Spanish", async ({ page }) => {
  await page.goto("/help");
  const main = page.locator("main");
  await expect(main).toContainText(`${BRAND} isn't connected to any business with "${HAVA}" in its name.`);
  await expect(main.locator("p[lang='es']")).toHaveText(`${BRAND} no tiene ninguna relación con negocios que lleven "${HAVA}" en su nombre.`);
});

test("TD-WEB-01 WEB-011: the ROC complaint page links on to the ROC", async ({ page }) => {
  expect((await page.goto("/help/roc-complaint"))?.status()).toBe(200);
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.getByRole("link", { name: "File a complaint on the ROC website" })).toHaveAttribute("href", "https://roc.az.gov/file-complaint");
  await page.goto("/help");
  await expect(page.getByRole("link", { name: "How to complain to the Registrar of Contractors" })).toHaveAttribute("href", "/help/roc-complaint");
});
