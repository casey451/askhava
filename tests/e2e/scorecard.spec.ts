// The owner's pilot scorecard in a browser (T158; TD-OPS-15's browser edge; OPS-039, CALL-024,
// OPS-046, OPS-047; D-2128, D-3890).
//
// T305 serves the scorecard behind the owner session and runs TD-OPS-15's browser half; until
// then, and after it, a signed-out browser asking for the page gets a refusal (a status of 400 or
// more, never asserted as a 404 so serving does not break this) with no figure, and a signed-out
// post of the weekly minutes form saves nothing. The figures, colors and the form are checked in
// tests/integration/tests/scorecard.rs. Fake data only.
import { test, expect } from "./fixtures";

const FIGURES = [
  "Pilot scorecard",
  "Median time to the first quote",
  "Masked calls a person answered",
  "paid introductions",
  "Your minutes this week",
];

test("TD-OPS-15: a signed-out browser gets no scorecard figure", async ({ request }) => {
  const answer = await request.get("/owner/scorecard", { maxRedirects: 0 });
  expect(answer.status()).toBeGreaterThanOrEqual(400);
  const body = await answer.text();
  for (const words of FIGURES) {
    expect(body).not.toContain(words);
  }
});

test("TD-OPS-15: a signed-out minutes post saves nothing", async ({ request }) => {
  const answer = await request.post("/owner/scorecard/minutes", {
    form: { recruiting: "30", other: "50", csrf: "fake-csrf" },
    maxRedirects: 0,
  });
  expect(answer.status()).toBeGreaterThanOrEqual(400);
  expect(await answer.text()).not.toContain("Your minutes for this week are saved.");
});
