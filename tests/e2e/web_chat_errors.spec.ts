// TD-A11Y-04 (T656; A11Y-030, A11Y-035; D-4326, D-5742): the /request chat with JavaScript off on
// the keyed binary (its code step needs the chat-codes part, D-3744). Each failed submit's error
// summary must come first in main, focused by autofocus alone; axe needs scripts, so each scan
// loads the served HTML at its URL and width into a script-on context. Other notices: chat.rs's
// unit tests. Fake data only: 928-555-019x numbers, which no other spec uses, and a made-up job.
import type { Browser, Page } from "@playwright/test";
import { axeScan } from "../a11y/fixtures";
import { test, expect } from "./fixtures";
import { keyedUrl } from "./keyed";

test.use({ javaScriptEnabled: false });

/** A made-up job with no 911-list words. */
const JOB = "The porch light flickers when it rains.";

/** A number too short to be a US cell number. */
const BAD_NUMBER = "92855501";

/** A fresh fake number in the E.164 form Verify takes: 928-555-0190 to 0194 at 320 px, 0195 to 0199 else. */
function fakeNumber(width: number): string {
  return `+1928555019${String((width === 320 ? 0 : 5) + Math.floor(Math.random() * 5))}`;
}

/** The Verify code the stand-in sent to `e164`. */
async function codeFor(e164: string): Promise<string> {
  const reply = await fetch(`http://127.0.0.1:4010/_stand_in/twilio/code?to=${encodeURIComponent(e164)}`);
  return ((await reply.json()) as { code: string }).code;
}

/** No axe violation on the page as it stands, named `what`, scanned in a context with scripts on. */
async function axeClean(browser: Browser, page: Page, what: string): Promise<void> {
  const context = await browser.newContext({ javaScriptEnabled: true, viewport: page.viewportSize() });
  const scan = await context.newPage();
  await scan.goto(page.url());
  await scan.setContent(await page.content(), { waitUntil: "load" });
  const { violations } = await axeScan(scan);
  await context.close();
  expect(violations, what).toEqual([]);
}

/** A11Y-030: the error page for `field`, as TD-A11Y-04 asks, then its axe scan. */
async function errorSummary(browser: Browser, page: Page, field: string, what: string): Promise<void> {
  await expect(page).toHaveTitle(/^Error: /);
  const first = page.locator("main > :first-child");
  await expect(first).toHaveAttribute("id", "chat-notice");
  await expect(first).toHaveAttribute("role", "alert");
  await expect.poll(() => page.evaluate(() => document.activeElement?.id), what).toBe("chat-notice");
  await expect(first.locator("a")).toHaveAttribute("href", `#${field}`);
  await expect(page.locator(`#${field}`)).toHaveAttribute("aria-invalid", "true");
  await axeClean(browser, page, what);
}

for (const width of [320, 1280]) {
  test(`TD-A11Y-04 A11Y-030 A11Y-035: an empty message, a bad number and a wrong code each show a focused error summary first, with what she typed kept, at ${String(width)} px`, async ({ browser, page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`${keyedUrl()}/request`);
    const words = page.getByLabel("What job do you need done?");
    await words.fill("   ");
    await page.getByRole("button", { name: "Send" }).click();
    await errorSummary(browser, page, "chat-words", `empty message at ${String(width)} px`);
    await expect(words).toHaveValue("   ");

    await words.fill(JOB);
    await page.getByRole("button", { name: "Send" }).click();
    const number = page.getByLabel("Your mobile number");
    await expect(number).toBeVisible();
    await expect(page).not.toHaveTitle(/^Error: /);
    await axeClean(browser, page, `number step at ${String(width)} px`);

    await number.fill(BAD_NUMBER);
    await page.getByRole("button", { name: "Text me a code" }).click();
    await errorSummary(browser, page, "chat-number", `bad number at ${String(width)} px`);
    await expect(number).toHaveValue(BAD_NUMBER);

    const her = fakeNumber(width);
    await number.fill(her);
    await page.getByRole("button", { name: "Text me a code" }).click();
    const code = page.getByLabel("The code we texted you");
    await expect(code).toBeVisible();
    await expect(page).not.toHaveTitle(/^Error: /);
    await axeClean(browser, page, `code step at ${String(width)} px`);

    const sent = await codeFor(her);
    await code.fill(sent === "000000" ? "111111" : "000000");
    await page.getByRole("button", { name: "Check the code" }).click();
    await errorSummary(browser, page, "chat-code", `wrong code at ${String(width)} px`);
    await expect(code).toHaveValue("");
  });
}
