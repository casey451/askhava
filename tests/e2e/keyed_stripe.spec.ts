/// <reference types="node" />
// The keyed web binary builds the Stripe webhook (T492, D-3813; SEC-002, SEC-032).
//
// `webserver.ts` gives the keyed binary fake Stripe stand-in settings, a webhook secret, an account,
// a publishable key and a launch date, so `release_sink::stripe_webhook` builds its hooks and the
// binary serves the Stripe webhook route (and, once T261 merges it, the Confirm route). A post with
// a bad signature therefore gets the webhook's refusal (403), never the 404 of a route that is not
// there.
//
// T515 (D-3862, D-3869): a pick brought to "waiting" through the Confirm journey (keyed.ts
// `seedPick` and `confirmToWaiting`) is released once its stand-in `payment_intent.succeeded` is
// delivered signed with the run's webhook secret (`stripeDelivery`); a second delivery adds nothing,
// and the stand-in's own default-secret signature, an altered byte and a stale timestamp are each
// refused. The payload is the stand-in's own serialization of the event (`/_stand_in/stripe/webhook`),
// sent exactly as signed. Fake data only; no key or secret is printed or logged.
import type { APIRequestContext } from "@playwright/test";
import { test, expect } from "./fixtures";
import { confirmToWaiting, keyedUrl, psql, seedPick, stripeDelivery } from "./keyed";

/** The stand-in server's origin (tests/e2e/webserver.ts `STAND_IN_PORT`). */
const STAND_IN = "http://127.0.0.1:4010";
/** Any `sk_test_` key: the stand-in checks only the prefix. */
const STAND_IN_AUTH = { authorization: `Bearer sk_test_${"e2e".repeat(4)}` };
/** The webhook's signature tolerance is 300 seconds; this is well past it. */
const STALE_SECONDS = 600;

interface StandInEvent { id: string; data: { object: { metadata?: { pick_id?: string } } } }

/** The stand-in's `payment_intent.succeeded` for `pick`, signed by the stand-in at `timestamp`. */
async function standInDelivery(request: APIRequestContext, pick: string, timestamp: number): Promise<{ payload: string; signature: string }> {
  const listed = await request.get(`${STAND_IN}/stripe/v1/events?type=payment_intent.succeeded`, { headers: STAND_IN_AUTH });
  const events = ((await listed.json()) as { data: StandInEvent[] }).data;
  const event = events.find((e) => e.data.object.metadata?.pick_id === pick);
  if (event === undefined) throw new Error("the stand-in emitted no payment_intent.succeeded for the pick");
  const signed = await request.post(`${STAND_IN}/_stand_in/stripe/webhook`, { data: { id: event.id, timestamp } });
  const body = (await signed.json()) as { payload: string; headers: { "stripe-signature": string } };
  return { payload: body.payload, signature: body.headers["stripe-signature"] };
}

/** The pick's release rows (PICK-022). */
const releases = (pick: string): number => Number(psql(`SELECT count(*) FROM picks.release WHERE pick_id = '${pick}'`));

test("SEC-032 D-3813: on the keyed binary a Stripe webhook post with a bad signature gets the webhook's refusal, not 404", async ({ request }) => {
  const reply = await request.post(`${keyedUrl()}/webhooks/stripe`, {
    headers: { "content-type": "application/json", "stripe-signature": "t=1,v1=00" },
    data: JSON.stringify({ id: "evt_e2efake", type: "payment_intent.succeeded" }),
    maxRedirects: 0,
  });
  expect(reply.status(), "the Stripe webhook route exists and refused the signature").toBe(403);
});

test("SEC-016 PICK-022 D-3862: a validly signed payment_intent.succeeded releases the waiting pick once; refused deliveries change nothing", async ({ page, request }) => {
  const seeded = seedPick();
  await confirmToWaiting(page, seeded);
  const now = Math.floor(Date.now() / 1000);
  const { payload, signature } = await standInDelivery(request, seeded.pick, now);
  const deliver = async (body: string, headers: Record<string, string>): Promise<number> =>
    (await request.post(`${keyedUrl()}/webhooks/stripe`, { headers, data: body, maxRedirects: 0 })).status();
  const altered = payload.replace('"amount": 2500', '"amount": 2501');
  expect(altered, "one byte changed").not.toBe(payload);
  expect(await deliver(payload, { "content-type": "application/json", "stripe-signature": signature }), "the stand-in's default secret").toBe(403);
  expect(await deliver(altered, stripeDelivery(payload, now)), "one changed byte").toBe(403);
  expect(await deliver(payload, stripeDelivery(payload, now - STALE_SECONDS)), "a stale timestamp").toBe(403);
  expect(releases(seeded.pick), "a refused delivery releases nothing").toBe(0);
  expect(await deliver(payload, stripeDelivery(payload, now)), "the run's secret").toBe(200);
  await expect.poll(() => releases(seeded.pick), "released once").toBe(1);
  expect(await deliver(payload, stripeDelivery(payload, Math.floor(Date.now() / 1000))), "the same event again").toBe(200);
  expect(releases(seeded.pick), "a second delivery adds nothing").toBe(1);
});
