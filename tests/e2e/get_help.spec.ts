// The business Get help link and form, and the site help form, in a browser (T200; TD-BIZ-23,
// TD-PRIV-16; BIZ-047, OPS-036, CON-034, CON-035, PRIV-031; D-3155, D-4293).
//
// T309 serves the Get help form and the help form's POST (D-2128); until then a browser can check
// what an unserved page allows: no public page carries the business-only "Get help" link, the
// business help page links the form, the help page's form has its optional mobile field, and a
// signed-out Get help or help-form post opens and answers nothing. The answers themselves (topic
// answer first, then "Got it ... within 2 days", the /h/sign-in link) are checked on the rendered
// pages in app/portals/src/help.rs. Fake data only.
import { test, expect } from "./fixtures";

const GET_HELP = "/b/help";

test("TD-BIZ-23: no public page links Get help; the business help page links the form", async ({
  page,
}) => {
  for (const path of ["/", "/help", "/business/help"]) {
    await page.goto(path);
    await expect(page.locator("[data-get-help]"), path).toHaveCount(0);
  }
  await page.goto("/business/help");
  await expect(page.getByRole("link", { name: "Get help form" })).toHaveAttribute("href", GET_HELP);
});

test("TD-BIZ-23: a signed-out browser gets no Get help form or answer", async ({ request }) => {
  const shown = await request.get(GET_HELP, { maxRedirects: 0 });
  expect(shown.status()).toBeGreaterThanOrEqual(300);
  expect(await shown.text()).not.toContain("What is it about?");
  const sent = await request.post(GET_HELP, {
    form: { topic: "emails", job: "1042", message: "Fake: no emails since Monday." },
  });
  expect(sent.status()).toBeGreaterThanOrEqual(400);
  expect(await sent.text()).not.toContain("Got it");
});

test("TD-PRIV-16: the help form has an optional mobile field and a bare post deletes nothing", async ({
  page,
  request,
}) => {
  await page.goto("/help");
  const mobile = page.getByLabel("Your mobile number (optional)");
  await expect(mobile).toHaveAttribute("name", "mobile");
  await expect(mobile).toHaveAttribute("type", "tel");
  await expect(mobile).toHaveAttribute("autocomplete", "tel");
  await expect(mobile).toHaveAttribute("aria-describedby", "help-mobile-hint");
  const answer = await request.post("/help", {
    form: { message: "Please delete my data.", mobile: "+19285550100" },
  });
  expect(answer.status()).toBeGreaterThanOrEqual(400);
  const body = await answer.text();
  expect(body).not.toContain("Message sent");
  expect(body).not.toContain("We've stopped texting");
});
