// The styled 404 page, the site icons and the head's share tags, served through the whole app
// (T615; WEB-027, WEB-004, WEB-007, A11Y-001, A11Y-003, SEC-012; DESIGN_SPEC section 5, D-4252).
//
// A path no route serves answers 404 with the site layout: the header (the wordmark links home,
// Help in the nav), one h1 and the copy file's one sentence, the footer, no script, axe-clean at
// 390 and 1440 px, every security header the home page carries and `Cache-Control: no-store`.
// The site mount's unknown name keeps its own empty 404. The icon, the touch icon and the share
// image answer 200 with their types, and the home page's head names each icon and share tag once.
// Expected brand words come from the brand file (D-1932). Fake data only: no data is seeded.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { axeScan } from "../a11y/fixtures";
import { test, expect } from "./fixtures";

/** The brand file the app embeds (app/core/data/brand.json, D-1932). */
const BRAND = JSON.parse(readFileSync(join(import.meta.dirname, "..", "..", "app", "core", "data", "brand.json"), "utf8")) as {
  display_name: string;
  domain: string;
};

/** WEB-004's tagline, word for word (`docs/spec/website.md:8`). */
const TAGLINE = "Local quotes. Your choice.";

/** The SEC-012 headers every page carries, the 404 included. */
const SECURITY_HEADERS = ["content-security-policy", "strict-transport-security", "permissions-policy", "x-content-type-options", "referrer-policy"];

test("WEB-027 A11Y-001 A11Y-003 D-4252: an unknown path answers 404 with the site page, axe-clean at 390 and 1440", async ({ page }) => {
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    const answer = await page.goto("/no-such-page");
    expect(answer?.status()).toBe(404);
    await expect(page.locator("h1")).toHaveCount(1);
    await expect(page.locator("h1")).toHaveText("We can't find that page.");
    await expect(page.getByText("That link may be old, so use the links at the top to go home or to Help.")).toBeVisible();
    await expect(page.locator("header a[href='/']")).toHaveText(BRAND.display_name);
    await expect(page.locator("header a[href='/help']").first()).toHaveText("Help");
    await expect(page.locator("footer.site-footer")).toContainText(TAGLINE);
    await expect(page.locator("script")).toHaveCount(0);
    expect((await axeScan(page)).violations, `${String(width)}px`).toEqual([]);
  }
});

test("SEC-012 D-4252: the 404 keeps the home page's security headers and is no-store", async ({ request }) => {
  const home = (await request.get("/")).headers();
  const missing = await request.get("/a/b/no-such-page?x=1");
  expect(missing.status()).toBe(404);
  expect(missing.headers()["content-type"]).toContain("text/html");
  expect(missing.headers()["cache-control"]).toBe("no-store");
  for (const name of SECURITY_HEADERS) {
    expect(missing.headers()[name], name).toBe(home[name]);
    expect(home[name], name).toBeTruthy();
  }
  const mount = await request.get("/static/site/no-such-file.png");
  expect(mount.status()).toBe(404);
  expect(await mount.text()).toBe("");
});

test("WEB-027 D-4252: the icon, touch icon and share image answer with their types", async ({ request }) => {
  for (const [path, type] of [
    ["/static/site/brand/mark.svg", "image/svg+xml"],
    ["/static/site/brand/touch-icon.png", "image/png"],
    ["/static/site/brand/share.png", "image/png"],
  ] as const) {
    const answer = await request.get(path);
    expect(answer.status(), path).toBe(200);
    expect(answer.headers()["content-type"], path).toBe(type);
  }
});

test("WEB-027 WEB-004 D-4252: the home page's head names each icon and share tag once", async ({ page }) => {
  await page.goto("/");
  const tags: [string, string, string][] = [
    ["link[rel='icon']", "href", "/static/site/brand/mark.svg"],
    ["link[rel='icon']", "type", "image/svg+xml"],
    ["link[rel='apple-touch-icon']", "href", "/static/site/brand/touch-icon.png"],
    ["meta[name='theme-color']", "content", "#F7F5F0"],
    ["meta[name='description']", "content", TAGLINE],
    ["meta[property='og:title']", "content", BRAND.display_name],
    ["meta[property='og:description']", "content", TAGLINE],
    ["meta[property='og:type']", "content", "website"],
    ["meta[property='og:image']", "content", `https://${BRAND.domain}/static/site/brand/share.png`],
  ];
  for (const [selector, attribute, value] of tags) {
    const tag = page.locator(`head ${selector}`);
    await expect(tag, selector).toHaveCount(1);
    await expect(tag, selector).toHaveAttribute(attribute, value);
  }
  await expect(page.locator("footer")).toContainText(TAGLINE);
});
