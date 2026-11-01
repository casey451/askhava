// The pick's code-and-contact form in a browser (T187; PICK-013, PICK-014, PICK-018, A11Y-010 to
// A11Y-013, A11Y-015, A11Y-016, A11Y-032; TD-PICK-06's paste and autofill cases, TD-PICK-19's
// contact-choice case; D-1264 to D-1266).
//
// A job in Quotes ready is seeded through fixtures.ts. The quote card's Pick opens the review step;
// the code step (`POST /q/{token}/code`) is reached from the review's final form with its address
// switched, because the final Pick shows "Nothing was picked" until Verify and the stored page
// pick are wired (D-1248, D-1264). POSTs are re-sent with the same-origin https Origin, as
// tests/e2e/pick_flow.spec.ts does, until D-1250's fix lands. Fake data only.
import type { Page } from "@playwright/test";
import { PAGE_STATES, WIDTHS, axeScan, expect, open, test } from "./fixtures";

const READY = PAGE_STATES.find((state) => state.name === "three quotes, #2 gone");

/** Re-sends every POST with a same-origin https Origin (D-1250). */
async function httpsOrigin(page: Page): Promise<void> {
  await page.route("**/q/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const headers = { ...request.headers(), origin: `https://${new URL(request.url()).host}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

async function openCodeStep(page: Page, pageUrl: (state: NonNullable<typeof READY>) => string): Promise<void> {
  if (READY === undefined) throw new Error("no Quotes ready state");
  await httpsOrigin(page);
  await open(page, pageUrl(READY), 1280);
  await page.getByRole("button", { name: "Pick #1" }).click();
  await page.locator("#final-pick").evaluate((button) => {
    const form = button.closest("form");
    if (form === null) throw new Error("no final form");
    form.action = form.action.replace(/\/pick$/, "/code");
  });
  await page.locator("#final-pick").click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Finish your pick for request \d+$/);
}

async function finish(page: Page): Promise<void> {
  await page.getByRole("button", { name: "Finish my pick" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(/^Finish your pick for request \d+$/);
}

test("A11Y-010 A11Y-013 A11Y-032 PICK-014 TD-PICK-06: visible labels, hints before inputs, one code field that takes paste and autofill", async ({ page, pageUrl }) => {
  await openCodeStep(page, pageUrl);
  for (const [field, hint] of [["code", "code-hint"], ["email", "email-hint"]] as const) {
    await expect(page.locator(`label[for="${field}"]`)).toBeVisible();
    const order = await page.evaluate(({ f, h }: { f: string; h: string }) => {
      const at = (selector: string): Element => document.querySelector(selector) ?? document.body;
      const before = (a: Element, b: Element): boolean => (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
      return before(at(`label[for="${f}"]`), at(`#${h}`)) && before(at(`#${h}`), at(`#${f}`));
    }, { f: field, h: hint });
    expect(order, `${field}: label, then hint, then input`).toBe(true);
    await expect(page.locator(`#${field}`)).toHaveAttribute("aria-describedby", new RegExp(hint));
  }
  const code = page.locator("#code");
  await expect(page.locator('input[autocomplete="one-time-code"]')).toHaveCount(1);
  await expect(code).toHaveAttribute("inputmode", "numeric");
  await expect(page.locator("#email")).toHaveAttribute("autocomplete", "email");
  const pasteAllowed = await code.evaluate((input) => {
    const data = new DataTransfer();
    data.setData("text/plain", "123 456");
    return input.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  });
  expect(pasteAllowed, "nothing cancels a paste").toBe(true);
  await code.fill("123 456");
  await page.getByLabel(/^A phone call/).check();
  await finish(page);
  await expect(page.locator("#error-summary")).toHaveCount(0);
  await expect(page.locator("#pick-not-open")).toBeFocused();
});

test("A11Y-011 A11Y-012: errors are named, linked from a focused summary, announced, and every value stays", async ({ page, pageUrl }) => {
  await openCodeStep(page, pageUrl);
  await finish(page);
  await expect(page).toHaveTitle(/^Error: /);
  const summary = page.locator("#error-summary");
  await expect(summary).toBeFocused();
  await expect(summary).toHaveAttribute("role", "alert");
  for (const [target, message] of [["#reach-call", "#reach-error"], ["#code", "#code-error"]] as const) {
    const link = summary.locator(`a[href="${target}"]`);
    await expect(page.locator(message)).toHaveText(`Error: ${await link.innerText()}`);
  }
  await expect(page.locator("#code")).toHaveAttribute("aria-invalid", "true");
  await page.getByLabel(/^Email\. /).check();
  await page.locator("#email").fill("sample-at-example");
  await page.locator("#code").fill("123456");
  await finish(page);
  await expect(summary.locator("a")).toHaveCount(1);
  await expect(summary.locator('a[href="#email"]')).toBeVisible();
  await summary.locator("a").click();
  await expect(page.locator("#email")).toBeFocused();
  await expect(page.locator("#email")).toHaveValue("sample-at-example");
  await expect(page.locator("#code")).toHaveValue("123456");
  await expect(page.getByLabel(/^Email\. /)).toBeChecked();
});

test("A11Y-015 A11Y-016: the code's limit is said in plain words before the field, and Send a new code keeps her answers", async ({ page, pageUrl }) => {
  await openCodeStep(page, pageUrl);
  await expect(page.locator("#code-hint")).toContainText("It works for 10 minutes.");
  await expect(page.locator("#code-hint")).toContainText('If it runs out, press "Send a new code".');
  await expect(page.locator('[role="timer"], time')).toHaveCount(0);
  expect(await page.locator("main").innerText()).not.toMatch(/\d+:\d\d/);
  await page.getByLabel(/^Email\. /).check();
  await page.locator("#email").fill("sample@example.com");
  await page.locator("#code").fill("999999");
  await page.getByRole("button", { name: "Send a new code" }).click();
  await expect(page.getByLabel(/^Email\. /)).toBeChecked();
  await expect(page.locator("#email")).toHaveValue("sample@example.com");
  await expect(page.locator("#code")).toHaveValue("");
  await expect(page.locator("#error-summary")).toHaveCount(0);
});

test("PICK-018 TD-PICK-19: the contact choice is a masked call or a shared email, and no call window is asked", async ({ page, pageUrl }) => {
  await openCodeStep(page, pageUrl);
  const choices = page.locator('input[name="reach"]');
  await expect(choices).toHaveCount(2);
  await expect(page.getByLabel(/^A phone call/)).toHaveAttribute("value", "call");
  await expect(page.getByLabel(/^A phone call/)).not.toBeChecked();
  await expect(page.getByLabel(/^Email\. /)).toHaveAttribute("value", "email");
  await expect(page.locator("main")).toContainText("They never see your number.");
  await expect(page.locator("main select, main input[type=time]")).toHaveCount(0);
  for (const asked of ["What time", "call window", "Who will let", "lockbox"]) await expect(page.locator("main")).not.toContainText(asked);
});

test("A11Y-001 A11Y-011: the code step has no axe violation at 320, 768 and 1280 px, before and after a failed submit", async ({ page, pageUrl }) => {
  await openCodeStep(page, pageUrl);
  for (const failed of [false, true]) {
    if (failed) await finish(page);
    for (const width of WIDTHS) {
      await page.setViewportSize({ width, height: 900 });
      const { violations } = await axeScan(page);
      expect(violations, `${failed ? "after errors" : "first view"} at ${String(width)} px`).toEqual([]);
    }
  }
});
