// The "Who runs Laftly" blocks served with their record-link settings unset (T161; TD-WEB-10;
// WEB-021, WEB-022; D-409, D-3744, D-3890).
//
// The suite starts one binary with a fixed environment (tests/e2e/webserver.ts), which sets neither
// ASKHAVA_LLC_RECORD_URL nor ASKHAVA_CITY_BUSINESS_LICENSE_NUMBER, so here every block (the home
// page, the business page and their Spanish pages) leaves out the record link and the license line.
// The set case, and the switch between the two, is the Rust render test
// app/site/src/tests/who_runs.rs. Every paragraph of a block opens with a known line, so no personal
// name or photo is in it. The brand is read from the brand file, never typed. No data is seeded.
import { readFileSync } from "node:fs";
import { test, expect } from "./fixtures";

const BRAND = (JSON.parse(readFileSync("app/core/data/brand.json", "utf8")) as { display_name: string }).display_name;

const PAGES: readonly [string, string][] = [
  ["/", `Who runs ${BRAND}`],
  ["/business", `Who runs ${BRAND}`],
  ["/es", `Quién opera ${BRAND}`],
  ["/es/business", `Quién opera ${BRAND}`],
];

// The first words of every paragraph a block may hold (WEB-021, WEB-022).
const LINE_STARTS = [
  `${BRAND} is run by `,
  `${BRAND} lo opera `,
  "Mailing address: ",
  "Dirección postal: ",
  "Help email: ",
  "Correo de ayuda: ",
  "The owner has a full-time day job.",
  "El dueño tiene un trabajo de tiempo completo.",
  `${BRAND} isn't an emergency service`,
  `${BRAND} no es un servicio de emergencias`,
  "This page in English",
];

test.use({ javaScriptEnabled: false });

for (const [path, heading] of PAGES) {
  test(`TD-WEB-10 WEB-021 WEB-022: ${path} leaves out the record link and license while unset`, async ({ page }) => {
    const answer = await page.goto(path);
    expect(answer?.status()).toBe(200);
    await expect(page.getByRole("heading", { level: 2, name: heading, exact: true })).toHaveCount(1);
    const lines = await page.evaluate((title) => {
      const start = Array.from(document.querySelectorAll("h2")).find((h) => h.textContent === title);
      const found: [string, boolean][] = [];
      for (let node = start?.nextElementSibling; node?.tagName === "P"; node = node.nextElementSibling) {
        found.push([node.textContent.trim(), node.querySelector("img") !== null]);
      }
      return found;
    }, heading);
    expect(lines.length).toBeGreaterThanOrEqual(3);
    for (const [text, image] of lines) {
      expect(image, text).toBe(false);
      expect(LINE_STARTS.some((start) => text.startsWith(start)), text).toBe(true);
      expect(text).not.toContain("Arizona Corporation Commission");
      expect(text).not.toMatch(/business license:|Licencia comercial/);
    }
    await expect(page.locator("body")).not.toContainText("Arizona Corporation Commission");
  });
}
