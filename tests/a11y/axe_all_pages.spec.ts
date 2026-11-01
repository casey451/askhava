// axe-core on every page and state at 320, 768 and 1280 px (T053; A11Y-001, A11Y-002, A11Y-025;
// TD-A11Y-01's incomplete-result cases; docs/test-design.md section 6).
//
// The page list is the route table (fixtures.ts `ROUTES`): a GET route with no state in
// `PAGE_STATES` fails here, and so does an "unparsed route" or other routing call the scan can't
// list. Each state at each width must have no violation of the WCAG 2.0, 2.1 and 2.2 A and AA
// rules; every axe "incomplete" result is attached and annotated in the JSON report, printed to
// the console, and fails unless listed in `REVIEWED_INCOMPLETE`. The planted case proves a
// violation is reported. Fake data only.
import { PAGE_STATES, REVIEWED_INCOMPLETE, ROUTES, STATES_TO_CHECK, WIDTHS, axeScan, expect, open, test } from "./fixtures";

test("A11Y-002: every GET route in the route table has page states", () => {
  expect(ROUTES.length).toBeGreaterThan(0);
  const missing = ROUTES.filter((route) => !PAGE_STATES.some((state) => state.route === route));
  expect(missing, "routes with no entry in PAGE_STATES").toEqual([]);
});

for (const state of STATES_TO_CHECK) {
  for (const width of WIDTHS) {
    test(`A11Y-001 A11Y-002 A11Y-025 TD-A11Y-01: axe on ${state.route} (${state.name}) at ${String(width)}px`, async ({ page, pageUrl }, info) => {
      await open(page, pageUrl(state), width);
      await expect(page.getByText(state.shows).first()).toBeVisible();
      await expect(page.getByText(state.hides)).toHaveCount(0);
      const { violations, incomplete } = await axeScan(page);
      await info.attach("axe incomplete (A11Y-025)", { body: JSON.stringify(incomplete), contentType: "application/json" });
      info.annotations.push({ type: "axe incomplete (A11Y-025)", description: incomplete.join("; ") || "none" });
      if (incomplete.length > 0) console.log(`axe incomplete on ${state.route} (${state.name}) at ${String(width)}px:`, incomplete);
      expect(violations).toEqual([]);
      const ruleOf = (result: string): string => result.split(":")[0] ?? result;
      const unreviewed = incomplete.filter((result) => !REVIEWED_INCOMPLETE.includes(`${ruleOf(result)} on ${state.route}`));
      expect(unreviewed, "TD-A11Y-01: unreviewed axe incomplete results").toEqual([]);
    });
  }
}

test("A11Y-002: a planted violation is reported", async ({ page, pageUrl }) => {
  const [state] = STATES_TO_CHECK;
  if (state === undefined) throw new Error("no page state to plant into");
  await open(page, pageUrl(state), 320);
  await page.evaluate(() => {
    const image = document.createElement("img");
    image.src = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";
    document.querySelector("main")?.append(image);
  });
  const { violations } = await axeScan(page);
  expect(violations.some((v) => v.startsWith("image-alt:"))).toBe(true);
});
