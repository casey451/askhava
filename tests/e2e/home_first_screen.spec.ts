// The home page's first screen and its photography assets (T216; WEB-027, WEB-030; DESIGN_SPEC
// sections 5 and 9; D-3331, D-3844, D-3890, D-4246, D-4252).
//
// At 390 by 844 and 320 by 740, normal text size, the hero photo (found through its `<img>`, which
// draws the phone crop) and the whole "Get quotes" link are inside the first viewport, and no text
// box on the page overlaps the photo's box. The photo is decorative (`alt=""`, width and height
// set); every served crop is the right image type, 100 KB or less, and its bytes hold no `Exif`,
// `<x:xmpmeta` or `GPS` block (`scripts/pii_scan.py` does not read AVIF, D-4252). The mark sits
// beside, not inside, the home link, hidden from assistive tech, and the served mark.svg has no
// script. Fake data only: the page shows no person's details.
import type { Page } from "@playwright/test";
import { test, expect } from "./fixtures";

/** The two first screens DESIGN_SPEC section 9 measures. */
const SCREENS = [
  { width: 390, height: 844 },
  { width: 320, height: 740 },
];

/** Every served hero crop and its type. */
const PHOTOS = [
  { path: "/static/site/photos/hero-kitchen-phone.avif", type: "image/avif" },
  { path: "/static/site/photos/hero-kitchen-phone.webp", type: "image/webp" },
  { path: "/static/site/photos/hero-kitchen.webp", type: "image/webp" },
];

/** Runs in the page: each text box (one rect per line of a text node) that overlaps `photo`. */
function textOnPhoto(photo: { x: number; y: number; width: number; height: number }): string[] {
  const out: string[] = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = (node.textContent ?? "").trim();
    if (!text) continue;
    const range = document.createRange();
    range.selectNodeContents(node);
    for (const r of range.getClientRects()) {
      const apart = r.right <= photo.x || r.left >= photo.x + photo.width
        || r.bottom <= photo.y || r.top >= photo.y + photo.height;
      if (!apart && r.width > 0 && r.height > 0) out.push(text.slice(0, 40));
    }
  }
  return out;
}

/** Opens the home page at `width` by `height`, scrolled to the top. */
async function openHome(page: Page, width: number, height: number): Promise<void> {
  await page.setViewportSize({ width, height });
  await page.goto("/");
  await page.locator("main picture img").evaluate((img: HTMLImageElement) => img.decode());
}

for (const { width, height } of SCREENS) {
  test(`WEB-027 D-4252: at ${String(width)} by ${String(height)} the photo and Get quotes are on the first screen and no text sits on the photo`, async ({ page }) => {
    await openHome(page, width, height);
    const img = page.locator("main picture img");
    await expect(img).toHaveCount(1);
    await expect(img).toHaveAttribute("alt", "");
    await expect(img).toHaveAttribute("width", "557");
    await expect(img).toHaveAttribute("height", "941");
    expect(await img.evaluate((el: HTMLImageElement) => el.currentSrc)).toContain("/static/site/photos/hero-kitchen-phone.");
    const photo = await img.boundingBox();
    if (!photo) throw new Error("the photo has no box");
    expect(photo.height, "the photo shows").toBeGreaterThanOrEqual(120);
    expect(photo.y, "photo top").toBeGreaterThanOrEqual(0);
    expect(photo.y + photo.height, "photo bottom").toBeLessThanOrEqual(height);
    const button = await page.getByRole("link", { name: "Get quotes", exact: true }).first().boundingBox();
    if (!button) throw new Error("Get quotes has no box");
    expect(button.y, "Get quotes top").toBeGreaterThanOrEqual(photo.y + photo.height);
    expect(button.y + button.height, "Get quotes bottom").toBeLessThanOrEqual(height);
    expect(await page.evaluate(textOnPhoto, photo)).toEqual([]);
  });
}

test("WEB-027 D-4252: every hero crop is served as its type, 100 KB or less, with no metadata", async ({ request }) => {
  for (const { path, type } of PHOTOS) {
    const reply = await request.get(path);
    expect(reply.status(), path).toBe(200);
    expect(reply.headers()["content-type"], path).toBe(type);
    const bytes = await reply.body();
    expect(bytes.length, path).toBeLessThanOrEqual(100 * 1024);
    for (const block of ["Exif", "<x:xmpmeta", "GPS"]) {
      expect(bytes.includes(Buffer.from(block, "latin1")), `${path} ${block}`).toBe(false);
    }
  }
});

test("WEB-027 D-3890: the mark sits beside the home link, hidden, and the served SVG has no script", async ({ page, request }) => {
  await openHome(page, 390, 844);
  const mark = page.locator("header .wordmark > svg.mark");
  await expect(mark).toHaveAttribute("aria-hidden", "true");
  await expect(page.locator("header a svg")).toHaveCount(0);
  await expect(page.locator("footer.site-footer > svg.mark")).toHaveAttribute("aria-hidden", "true");
  const box = await mark.boundingBox();
  expect(box && [Math.round(box.width), Math.round(box.height)]).toEqual([30, 27]);
  const reply = await request.get("/static/site/brand/mark.svg");
  expect(reply.status()).toBe(200);
  expect(reply.headers()["content-type"]).toBe("image/svg+xml");
  const svg = await reply.text();
  expect(svg).toContain('id="mark"');
  expect(svg).not.toMatch(/<script|<title|<metadata| on[a-z]+=/i);
});
