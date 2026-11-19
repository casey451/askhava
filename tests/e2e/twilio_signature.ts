/// <reference types="node" />
// Signs a fake inbound Twilio delivery for the keyed web binary (T536; SEC-016, D-3889, D-3892).
//
// `webserver.ts` gives the keyed binary a fake `ASKHAVA_PUBLIC_BASE_URL` and a per-run Twilio auth
// token, so it serves the Twilio webhooks; both values reach the test workers through
// `keyedSecrets()`. Twilio signs the public URL it posted to (the base plus the path and query)
// followed by each form field's name and value in name order, HMAC-SHA1 under the auth token, in
// base64 (`X-Twilio-Signature`), as `tests/e2e/j1_homeowner.spec.ts` does for its own binary.
// `twilioSignature` computes that header; `keyedTextIn` posts one fake inbound text to the keyed
// binary's `/webhooks/twilio/sms` and returns the HTTP status. Fake data only; the token is never
// printed or logged.
import { createHmac, randomBytes } from "node:crypto";
import { keyedPort, keyedSecrets } from "./webserver";

/** The inbound-text route Twilio posts to. */
export const TWILIO_SMS_PATH = "/webhooks/twilio/sms";

/** The fake account SID the keyed binary is configured with (`webserver.ts`). */
const ACCOUNT_SID = `AC${"0".repeat(32)}`;

/** The keyed binary's one masked number (`ASKHAVA_MASKED_NUMBERS` in `webserver.ts`). */
const MASKED_NUMBER = "+15005550100";

/** The `X-Twilio-Signature` Twilio sends for `form` posted to `path` (with any query) on the keyed binary. */
export function twilioSignature(path: string, form: Record<string, string>): string {
  const secrets = keyedSecrets();
  const signed = secrets.publicBaseUrl + path + Object.keys(form).sort().map((name) => name + (form[name] ?? "")).join("");
  return createHmac("sha1", secrets.twilioAuthToken).update(signed).digest("base64");
}

/**
 * Posts one fake inbound text from `from` with `body` to the keyed binary, signed as Twilio signs
 * it (SEC-016), to the masked number unless `to` names another; returns the HTTP status.
 */
export async function keyedTextIn(from: string, body: string, to = MASKED_NUMBER): Promise<number> {
  const form: Record<string, string> = {
    AccountSid: ACCOUNT_SID,
    Body: body,
    From: from,
    MessageSid: `SM${randomBytes(16).toString("hex")}`,
    NumMedia: "0",
    To: to,
  };
  const reply = await fetch(`http://127.0.0.1:${String(keyedPort())}${TWILIO_SMS_PATH}`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", "x-twilio-signature": twilioSignature(TWILIO_SMS_PATH, form) },
    body: new URLSearchParams(form).toString(),
  });
  return reply.status;
}
