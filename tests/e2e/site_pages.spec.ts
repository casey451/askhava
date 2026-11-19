// The home and how-it-works pages served through the whole app with JavaScript off (T102; TD-WEB-01,
// TD-WEB-07, TD-WEB-09, TD-TXT-15's WEB-009 case; WEB-001, WEB-002, WEB-003, WEB-004, WEB-005,
// WEB-009, WEB-012, WEB-022, WEB-026, WORD-002; D-3144, D-3878).
//
// Each page must render whole without a script, in its language, with the footer's "Homeowners"
// column, tagline and not-a-contractor statement, and no directory, events or /chat link. The old
// directory paths answer 200 with the home page, while any other unknown path stays a 404. The fee
// amounts are checked against the fee tiers in app/site/src/pages.rs (TD-WEB-09) and the WEB-028
// numbers in tests/integration/tests/home_numbers.rs (TD-JOB-04). The brand is read from the brand
// file, never typed. No data is seeded.
import { readFileSync } from "node:fs";
import { test, expect } from "./fixtures";

const BRAND = (JSON.parse(readFileSync("app/core/data/brand.json", "utf8")) as { display_name: string }).display_name;
const STATEMENT = `${BRAND} is not a contractor and does not perform, bid on or supervise work. We pass along your request and show you quotes from independent local businesses, as they wrote them. You choose, and you hire the business directly.`;

test.use({ javaScriptEnabled: false });

test("TD-WEB-01 TD-TXT-15 WEB-001 WEB-003 WEB-004 WEB-005 WEB-009 WORD-002: the home page renders without JavaScript", async ({ page }) => {
  const answer = await page.goto("/");
  expect(answer?.status()).toBe(200);
  expect(await page.locator("html").getAttribute("lang")).toBe("en");
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.locator("script")).toHaveCount(0);
  const footer = page.locator("footer");
  await expect(footer.locator("h2")).toHaveText("Homeowners");
  const links = await footer.locator("a").evaluateAll((all) => all.map((a) => [a.textContent, a.getAttribute("href")]));
  expect(links).toEqual([["How it works", "/how-it-works"], ["Services", "/services"], ["Get quotes", "/request"], ["Help", "/help"]]);
  await expect(footer).toContainText("Local quotes. Your choice.");
  await expect(footer).toContainText(STATEMENT);
  await expect(page.locator("main")).toContainText("Launch offer: your first win is free, on any job size, when you join in the first 90 days or within 90 days after your trade opens.");
  const box = page.locator("main form[method=post][action='/request']");
  await expect(box.getByRole("button", { name: "Get quotes", exact: true })).toHaveCount(1);
  const field = box.locator("textarea");
  await expect(field).toHaveCount(1);
  await expect(box.locator(`label[for="${String(await field.getAttribute("id"))}"]`)).toBeVisible();
  const hrefs = await page.locator("a").evaluateAll((all) => all.map((a) => a.getAttribute("href") ?? ""));
  for (const gone of ["/chat", "/categories", "/events"]) expect(hrefs, gone).not.toContain(gone);
  await expect(page.locator("body")).not.toContainText("More than quotes");
});

test("TD-WEB-07 WEB-022: the home page's Who runs block", async ({ page }) => {
  await page.goto("/");
  const main = page.locator("main");
  await expect(main.locator("h2", { hasText: `Who runs ${BRAND}` })).toHaveCount(1);
  await expect(main).toContainText("help and charge questions usually within 2 days");
  await expect(main).toContainText(`Don't rely on ${BRAND} in an emergency: call 911.`);
  await expect(main).not.toContainText("Safety texts go out at any hour.");
});

test("TD-WEB-01 WEB-012: the old directory paths show the home page; other unknown paths stay 404", async ({ page, request }) => {
  await page.goto("/");
  const headline = await page.locator("h1").innerText();
  for (const path of ["/categories", "/events"]) {
    const answer = await page.goto(path);
    expect(answer?.status(), path).toBe(200);
    await expect(page.locator("h1")).toHaveText(headline);
  }
  expect((await request.get("/directory/no-such-page")).status()).toBe(404);
});

test("TD-WEB-01 TD-WEB-09 WEB-002 WEB-026: the how-it-works pages in English and Spanish", async ({ page }) => {
  expect((await page.goto("/how-it-works"))?.status()).toBe(200);
  await expect(page.locator("h1")).toHaveText("How it works");
  await expect(page.locator("main")).toContainText(/The business you choose pays \S+ a set introduction fee \(\$\d+, \$\d+ or \$\d+, by quote size\)\. You pay \S+ nothing\./);
  await expect(page.getByRole("link", { name: "Get quotes in the chat" })).toHaveAttribute("href", "/request");
  await expect(page.locator("footer")).toContainText(STATEMENT);
  expect((await page.goto("/es/how-it-works"))?.status()).toBe(200);
  expect(await page.locator("html").getAttribute("lang")).toBe("es");
  await expect(page.locator("h1")).toHaveText("Cómo funciona");
  await expect(page.locator("main")).toContainText(/\(\$\d+, \$\d+ o \$\d+, según el tamaño de la cotización\)/);
  await expect(page.locator("footer")).toContainText(`${BRAND} no es contratista y no realiza, cotiza ni supervisa trabajos.`);
  await expect(page.locator("script")).toHaveCount(0);
});
