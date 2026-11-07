// The business billing and complaints pages in a browser (T195; TD-BIZ-04, TD-BIZ-09, TD-SEC-17;
// BIZ-015, BIZ-029, SEC-038; D-491, D-492, D-1896, D-1914).
//
// The business portal routes are not served yet (D-1728): business sign-in waits on the
// composition, so no browser can hold a business session. What a request can check now is the
// privacy edge: a signed-out browser asking for the billing page, the CSV statement, the complaints
// page or a billing or reply POST gets no ledger line, no card and no complaint, and no PDF
// statement exists. The pages' words (totals and CSV equal to the ledger, no PDF, kind and date
// with no homeowner detail, the 5-day reply box, the 5-minute code) are checked on the rendered
// templates and the database in app/portals/src/business_portal.rs; when the routes are served
// this spec signs in and checks them here (D-1914). Fake data only.
import { test, expect } from "./fixtures";

const PAGES = [
  "/b/billing",
  "/b/billing/statement.csv",
  "/b/complaints",
  "/b/billing/statement.pdf",
];

test("TD-BIZ-04, TD-BIZ-09: a signed-out browser gets no billing or complaint", async ({
  request,
}) => {
  for (const path of PAGES) {
    const answer = await request.get(path, { maxRedirects: 0 });
    expect(answer.status(), path).toBeGreaterThanOrEqual(400);
    const body = await answer.text();
    expect(body).not.toContain("date,entry,job,amount");
    expect(body).not.toContain("Introduction fee by job size");
    expect(body).not.toContain("card ending in");
    expect(body).not.toContain("Your reply");
    expect(answer.headers()["content-type"] ?? "").not.toContain("pdf");
  }
});

test("TD-SEC-17: a signed-out billing or reply post changes nothing", async ({
  request,
}) => {
  const posts = [
    "/b/billing/limit",
    "/b/complaints/00000000-0000-0000-0000-0000000000c1/reply",
  ];
  for (const path of posts) {
    const answer = await request.post(path, {
      form: { reply: "Fake reply.", limit: "90" },
    });
    expect(answer.status(), path).toBeGreaterThanOrEqual(400);
    expect(await answer.text()).not.toContain("Your reply");
  }
});
