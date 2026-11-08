// The homeowner portal in a browser (T087; TD-PRIV-05, TD-PRIV-15, TD-CON-28, TD-A11Y-02,
// TD-SEC-25; SEC-008, PRIV-012, PRIV-021, PRIV-031, SEC-046; D-491, D-1934).
//
// The homeowner portal routes are not served yet (D-1934): sign-in by texted code waits on T089's
// Verify send caps (PICK-055, D-493), so no browser can hold a homeowner session. What a request
// can check now is the privacy edge: a signed-out browser asking for the portal home, "Download my
// data" or a delete request gets no homeowner record, no file and no number, and a cross-site post
// to the texts-off switch, a delete request, its code step or the sign-in code step changes nothing.
// The pages' words, the one-time-code fields, the forbidden-field check, the switch and the code-checked requests are tested in
// app/portals/src/homeowner_portal.rs and app/portals/src/data_download.rs; when the routes are
// served this spec signs in and checks them here. Fake data only.
import { test, expect } from "./fixtures";

const PAGES = ["/h", "/h/my-data"];
const POSTS = ["/h/texts-off", "/h/my-data/request", "/h/my-data/code", "/h/sign-in/code"];

test("TD-PRIV-15: a signed-out browser gets no record and no data file", async ({ request }) => {
  for (const path of PAGES) {
    const answer = await request.get(path, { maxRedirects: 0 });
    expect(answer.status(), path).toBeGreaterThanOrEqual(400);
    const body = await answer.text();
    expect(body).not.toContain("Mobile number ending in");
    expect(body).not.toContain('"requests"');
    expect(body).not.toMatch(/\d{3}-\d{3}-\d{4}/);
  }
});

test("TD-CON-28: a signed-out post turns nothing off and deletes nothing", async ({ request }) => {
  for (const path of POSTS) {
    const answer = await request.post(path, {
      form: { kind: "delete", request: "1", phone: "+19285550100", code: "123456" },
      maxRedirects: 0,
    });
    expect(answer.status(), path).toBeGreaterThanOrEqual(400);
    const body = await answer.text();
    expect(body).not.toContain("Texts are off.");
    expect(body).not.toContain("delete");
  }
});
