// The business portal home and the released job's page in a browser (T085; TD-BIZ-04, TD-PICK-39;
// BIZ-013, PICK-024, SEC-004; D-491, D-1728).
//
// The business portal routes are not served yet (D-1728): business sign-in and its mail wait on
// the composition, so no browser can hold a business session. What a request can check now is
// the privacy edge: a signed-out browser asking for the portal home or a released job's page gets
// no homeowner detail and no tip line. The pages' words (the first name once, the PICK-024 tip, the
// email line, the call log, "Your numbers") are checked on the rendered templates in
// app/portals/src/job_release.rs and app/portals/src/business_portal.rs; when the routes are
// served this spec signs in and checks them here (D-1896). Fake data only.
import { test, expect } from "./fixtures";

const PATHS = ["/b", "/b/picks/00000000-0000-0000-0000-000000000007"];

test("TD-PICK-39: a signed-out browser gets no release details", async ({ request }) => {
  for (const path of PATHS) {
    const answer = await request.get(path, { maxRedirects: 0 });
    expect(answer.status(), path).toBeGreaterThanOrEqual(400);
    const body = await answer.text();
    expect(body).not.toContain("Tip: on your first call");
    expect(body).not.toContain("Homeowner chose");
    expect(body).not.toMatch(/\d{3}-\d{3}-\d{4}/);
  }
});
