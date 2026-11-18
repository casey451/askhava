// The owner portal's tools in a browser (T108; TD-SEC-22, TD-OPS-01, TD-BIZ-07; OPS-003,
// FEE-007, LIC-002, SEC-043, BIZ-027; D-2128, D-3890).
//
// T310 serves these pages behind the owner session; until then what a request can check is the
// privacy edge: a signed-out browser asking for the owner home or settings page, or posting any
// owner change, gets a refusal with no owner detail (no pause switch, fee mode, alert, opt-out
// rate or needs-review line) and changes nothing. The pages' words and every change are checked
// in tests/integration/tests/owner_portal_pages.rs; T310's spec signs in and drives them. Fake
// data only.
import { test, expect } from "./fixtures";

const PAGES = ["/owner/home", "/owner/settings"];

const POSTS: [string, Record<string, string>][] = [
  ["/owner/pause", { change: "pause_all", to: "on" }],
  ["/owner/fee-mode", { change: "fee_mode", target: "flat" }],
  ["/owner/category", { change: "category", target: "plumbing", to: "off" }],
  ["/owner/texting", { change: "homeowner_texting", to: "off" }],
  ["/owner/banner", { change: "banner_off" }],
  [
    "/owner/alert-ack",
    { change: "acknowledge", target: "00000000-0000-0000-0000-0000000000a1" },
  ],
];

const OWNER_WORDS = [
  "Pause or resume",
  "Save fee mode",
  "Opt-outs each week",
  "Sign-ups waiting for you",
  "Category settings",
  "calendar days",
];

test("TD-SEC-22: a signed-out browser gets no owner page", async ({
  request,
}) => {
  for (const path of PAGES) {
    const answer = await request.get(path, { maxRedirects: 0 });
    expect(answer.status(), path).toBeGreaterThanOrEqual(400);
    const body = await answer.text();
    for (const words of OWNER_WORDS) {
      expect(body, `${path}: ${words}`).not.toContain(words);
    }
  }
});

test("TD-OPS-01, TD-BIZ-07: a signed-out owner change is refused", async ({
  request,
}) => {
  for (const [path, form] of POSTS) {
    const answer = await request.post(path, { form, maxRedirects: 0 });
    expect(answer.status(), path).toBeGreaterThanOrEqual(400);
    const body = await answer.text();
    for (const words of OWNER_WORDS) {
      expect(body, `${path}: ${words}`).not.toContain(words);
    }
  }
});
