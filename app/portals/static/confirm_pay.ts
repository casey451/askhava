// The business Confirm page's card step (T224; FEE-014, FEE-016, FEE-047, PICK-021, PICK-022,
// TD-FEE-25, D-1479). Stripe.js, loaded only on this page from js.stripe.com, mounts the card field
// in `#card-element`, so card data goes to Stripe and never to Ask Hava. A press turns the card into
// a `pm_…` ID, posts it with the form's one-time press ID, and writes the server's answer into the
// page at once: a decline in the alert region, with the next press ID the server gave (the next
// attempt key, FEE-016); a bank approval through Stripe's in-page 3-D Secure step. Nothing here
// decides a payment: the release waits on Stripe's signed webhook (PICK-022). The fee is the
// server's, read from the button, never sent. The button stays enabled; the server handles a
// double press (A11Y-031). Fake data only in tests.

interface StripeCard {
  mount(selector: string): void;
}

interface StripeElements {
  create(kind: "card"): StripeCard;
}

interface StripeResult {
  paymentMethod?: { id: string };
  error?: { message?: string };
}

interface StripeClient {
  elements(): StripeElements;
  createPaymentMethod(options: { type: "card"; card: StripeCard }): Promise<StripeResult>;
  handleNextAction(options: { clientSecret: string }): Promise<StripeResult>;
}

/** The server's answer to one press (`portals::api::PressReply`). */
interface PressReply {
  state: string;
  message: string;
  next_press: string | null;
  client_secret?: string;
}

type StripeFactory = (key: string) => StripeClient;

function field(form: HTMLFormElement, name: string): HTMLInputElement | null {
  const found = form.elements.namedItem(name);
  return found instanceof HTMLInputElement ? found : null;
}

function say(id: string, words: string): void {
  const region = document.getElementById(id);
  if (region !== null) region.textContent = words;
}

async function send(form: HTMLFormElement): Promise<PressReply> {
  const body = new URLSearchParams();
  for (const [name, value] of new FormData(form)) {
    if (typeof value === "string") body.append(name, value);
  }
  const reply = await fetch(form.action, {
    method: "POST",
    body,
    headers: { accept: "application/json" },
  });
  return (await reply.json()) as PressReply;
}

async function show(stripe: StripeClient, form: HTMLFormElement, reply: PressReply): Promise<void> {
  const press = field(form, "press");
  if (reply.next_press !== null && press !== null) press.value = reply.next_press;
  if (reply.state === "approve" && reply.client_secret !== undefined) {
    say("confirm-status", reply.message);
    await stripe.handleNextAction({ clientSecret: reply.client_secret });
    return;
  }
  say(reply.state === "declined" ? "confirm-error" : "confirm-status", reply.message);
}

async function press(stripe: StripeClient, card: StripeCard, form: HTMLFormElement): Promise<void> {
  say("confirm-error", "");
  const made = await stripe.createPaymentMethod({ type: "card", card });
  const method = field(form, "payment_method");
  if (made.paymentMethod === undefined || method === null) {
    say("confirm-error", "Check the card number, expiry date and security code, then press the button again.");
    return;
  }
  method.value = made.paymentMethod.id;
  await show(stripe, form, await send(form));
}

function start(): void {
  const form = document.getElementById("confirm-form");
  const factory = (window as unknown as { Stripe?: StripeFactory }).Stripe;
  const key = form?.dataset.stripeKey;
  if (!(form instanceof HTMLFormElement) || factory === undefined || key === undefined) return;
  const stripe = factory(key);
  const card = stripe.elements().create("card");
  card.mount("#card-element");
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void press(stripe, card, form);
  });
}

start();
