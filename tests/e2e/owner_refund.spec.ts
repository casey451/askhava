// The owner refund review step and audit log viewer in a browser (T205; TD-SEC-01, TD-FEE-14;
// FEE-026, SEC-006; D-2128, D-4326).
//
// T304 serves these pages behind the owner session; until then what a request can check is the
// privacy edge: a signed-out GET of each page gets a status of 400 or more (never pinned to an
// exact 404, which T304's serving would change) and a body with no charge, refund, dispute or
// contact detail. No POST is sent. The pages' words and every press are checked in
// tests/integration/tests/owner_refund.rs; T304's spec signs in and drives them. Fake data only.
import { test, expect } from "./fixtures";

const PAGES = [
  "/owner/refunds/00000000-0000-0000-0000-0000000000b5",
  "/owner/audit-log",
];

const OWNER_WORDS = [
  "Refund review",
  "Refund this charge",
  "Disputes on this charge",
  "evidence in Stripe myself",
  "Audit log rows",
  "picks.pick",
];

test("TD-SEC-01, TD-FEE-14: a signed-out browser gets no refund or audit page", async ({
  request,
}) => {
  for (const path of PAGES) {
    const answer = await request.get(path, { maxRedirects: 0 });
    expect(answer.status(), path).toBeGreaterThanOrEqual(400);
    const body = await answer.text();
    for (const words of OWNER_WORDS) {
      expect(body, `${path}: ${words}`).not.toContain(words);
    }
    expect(body).not.toMatch(/\$\d+\.\d{2}/);
    expect(body).not.toMatch(/\d{3}-\d{3}-\d{4}/);
    expect(body).not.toMatch(/(pi|re|dp|ch)_[A-Za-z0-9]+/);
  }
});
