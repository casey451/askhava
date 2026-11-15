/// <reference types="node" />
// The keyed web binary builds the Stripe webhook (T492, D-3813; SEC-002, SEC-032).
//
// `webserver.ts` gives the keyed binary fake Stripe stand-in settings, a webhook secret, an account,
// a publishable key and a launch date, so `release_sink::stripe_webhook` builds its hooks and the
// binary serves the Stripe webhook route (and, once T261 merges it, the Confirm route). A post with
// a bad signature therefore gets the webhook's refusal (403), never the 404 of a route that is not
// there. Fake data only; no key or secret is printed or logged.
import { test, expect } from "./fixtures";
import { keyedUrl } from "./keyed";

test("SEC-032 D-3813: on the keyed binary a Stripe webhook post with a bad signature gets the webhook's refusal, not 404", async ({ request }) => {
  const reply = await request.post(`${keyedUrl()}/webhooks/stripe`, {
    headers: { "content-type": "application/json", "stripe-signature": "t=1,v1=00" },
    data: JSON.stringify({ id: "evt_e2efake", type: "payment_intent.succeeded" }),
    maxRedirects: 0,
  });
  expect(reply.status(), "the Stripe webhook route exists and refused the signature").toBe(403);
});
