// TD-INT-48's browser half: the served photo upload page on T452's keyed web binary (T674, D-4360,
// D-5082; INT-044, PRIV-014, PRIV-015, PRIV-025, PRIV-028, CARD-013, SEC-034, A11Y-027).
//
// Each test seeds, with the page's own `PAGE_ENTRIES` seed (tests/a11y/page_states.ts), a fake
// homeowner with a session, her request and its job, and opens her upload page with her session
// cookie. With JavaScript off she reaches the native file input and each button by keyboard alone:
// she adds a photo, sees it as businesses will, sends it (one `photo_ok` row), adds another and takes
// it off. With JavaScript on, the preview and the error screens (answers to a POST, so no
// `PAGE_STATES` entry) are axe-clean at 320, 768 and 1280 pixels. The photo is a screenshot of the
// page itself, so no real picture is used. Fake data only; no key or value is printed or logged.
import type { Locator, Page } from "@playwright/test";
import { axeScan, WIDTHS } from "../a11y/fixtures";
import { PAGE_ENTRIES } from "../a11y/page_states";
import { test, expect } from "./fixtures";
import { psql } from "./keyed";

const ROUTE = "/h/requests/{job}/photos";

/** Seeds her request, signs the browser in with her session cookie and opens her upload page; the job number. */
async function openHers(page: Page): Promise<string> {
  const entry = PAGE_ENTRIES.find((e) => e.route === ROUTE);
  const state = entry?.states[0];
  if (entry === undefined || state === undefined) throw new Error("the upload page has no PAGE_ENTRIES entry");
  const seeded = entry.seed(state);
  if (typeof seeded === "string") throw new Error("the upload page's seed gives her session cookie");
  const domain = new URL(seeded.url).hostname;
  const cookies = seeded.cookies.map(({ name, value }) => ({ name, value, domain, path: "/", secure: true, httpOnly: true, sameSite: "Lax" as const }));
  await page.context().addCookies(cookies);
  const reply = await page.goto(seeded.url);
  expect(reply?.status()).toBe(200);
  expect(reply?.headers()["cache-control"]).toContain("no-store");
  return /requests\/(\d+)\/photos/.exec(seeded.url)?.[1] ?? "";
}

/** How many rows of `table` (alias `t`, joined to her job as `n`) match `where`. */
const rows = (number: string, table: string, where: string): string =>
  psql(`SELECT count(*) FROM ${table} t JOIN jobs.job_number n ON n.job_id = t.job_id WHERE n.number = ${number} AND ${where}`);
const photos = (number: string): string => rows(number, "privacy.photo", "true");
const confirmed = (number: string): string =>
  psql(`SELECT count(*) FROM intake.intake_answer a JOIN intake.conversation t ON t.id = a.conversation_id
    JOIN jobs.job_number n ON n.job_id = t.job_id WHERE n.number = ${number} AND a.question_id = 'photo_ok'`);

/** Tabs until `target` has the focus (at most 40 stops), by keyboard alone. */
async function tabTo(page: Page, target: Locator): Promise<void> {
  for (let stop = 0; stop < 40; stop += 1) {
    await page.keyboard.press("Tab");
    if (await target.evaluate((el) => el === document.activeElement)) return;
  }
  throw new Error("the keyboard never reached the control");
}

/** Chooses `file` in the focused native file input and presses "Add photo" by keyboard. */
async function addPhoto(page: Page, file: Buffer, name = "fake.png"): Promise<void> {
  const input = page.getByLabel("Photo", { exact: true });
  await tabTo(page, input);
  await input.setInputFiles({ name, mimeType: "image/png", buffer: file });
  await tabTo(page, page.getByRole("button", { name: "Add photo" }));
  await page.keyboard.press("Enter");
}

test.describe("with JavaScript off", () => {
  test.use({ javaScriptEnabled: false });

  test("TD-INT-48: she adds, checks, sends and takes off photos by keyboard", async ({ page }) => {
    const number = await openHers(page);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Add photos to request ${number}`);
    await expect(page.locator("main")).toContainText("Please leave people, house numbers and license plates out of photos.");
    const file = await page.screenshot({ clip: { x: 0, y: 0, width: 400, height: 400 } });

    await addPhoto(page, file);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Check your photo for request ${number}`);
    const link = (await page.locator("#photo-preview").getAttribute("src")) ?? "";
    expect(link).toMatch(/^\/photos\/[0-9a-f]{64}$/);
    const photo = await page.request.get(new URL(link, page.url()).href);
    expect(photo.status()).toBe(200);
    expect(photo.headers()["content-type"]).toBe("image/jpeg");
    expect(photo.headers()["cache-control"]).toContain("no-store");
    expect(confirmed(number), "not on the card before she confirms (CARD-013)").toBe("0");

    await tabTo(page, page.getByRole("button", { name: "Send this photo" }));
    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Add photos to request ${number}`);
    expect([photos(number), confirmed(number)]).toEqual(["1", "1"]);

    await addPhoto(page, file);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Check your photo for request ${number}`);
    expect(photos(number)).toBe("2");
    await tabTo(page, page.getByRole("button", { name: "Take it off" }));
    await page.keyboard.press("Enter");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Add photos to request ${number}`);
    expect([photos(number), confirmed(number)]).toEqual(["1", "1"]);

    const dead = await page.request.get(new URL(`/photos/${"0".repeat(64)}`, page.url()).href);
    expect(dead.status()).toBe(404);
    expect(dead.headers()["cache-control"]).toContain("no-store");
  });
});

test("TD-INT-48: the preview and the error screens are axe-clean", async ({ page }) => {
  const number = await openHers(page);
  const scan = async (screen: string): Promise<void> => {
    for (const width of WIDTHS) {
      await page.setViewportSize({ width, height: 900 });
      const { violations } = await axeScan(page);
      expect(violations, `${screen} at ${String(width)} px`).toEqual([]);
    }
  };
  await addPhoto(page, Buffer.from("not a photo at all"), "fake.txt");
  await expect(page).toHaveTitle(/^Error: /);
  await expect(page.locator("#error-summary")).toBeFocused();
  await expect(page.getByRole("alert")).toContainText("We couldn't open that photo");
  await scan("error screen");
  expect(photos(number)).toBe("0");

  await page.setViewportSize({ width: 1280, height: 900 });
  await addPhoto(page, await page.screenshot({ clip: { x: 0, y: 0, width: 400, height: 400 } }));
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(`Check your photo for request ${number}`);
  await expect(page.locator("#photo-preview")).toHaveJSProperty("complete", true);
  await scan("preview");
});
