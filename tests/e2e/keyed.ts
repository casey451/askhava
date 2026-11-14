/// <reference types="node" />
// The keyed web binary's handles for the specs (T452, D-3674; SEC-004, PRIV-002).
//
// `webserver.ts` starts a second web binary beside the suite's, with fake Twilio Verify on the
// stand-in, its own ID secret and both vault keys, all made per run. This file gives a spec its base
// URL and ID secret, and writes values in the vault's own formats (app/privacy/src/vault.rs), so a
// spec can seed through psql what that binary opens: `seal` is AES-256-GCM under `VAULT_KEY`
// version 1 with a random 96-bit nonce, stored as the 2-byte big-endian version, the nonce, then the
// ciphertext and its tag, with associated data `<table>|<column>|<id>|v1`; `blindIndex` is
// HMAC-SHA256 under the key HMAC(`VAULT_INDEX_KEY`, `<purpose>|v1`). Node's `crypto` only. Fake data
// only; no key or value is printed or logged.
import { createCipheriv, createHmac, randomBytes } from "node:crypto";
import { keyedPort, keyedSecrets } from "./webserver";

/** The vault key version the keyed binary seals and indexes under. */
const VERSION = 1;

/** The keyed binary's base URL, e.g. for `page.goto(keyedUrl() + path)`. */
export function keyedUrl(): string {
  return `http://127.0.0.1:${String(keyedPort())}`;
}

/** The keyed binary's `ASKHAVA_ID_SECRET`, for a spec that makes an ID or link it checks. */
export function keyedIdSecret(): string {
  return keyedSecrets().idSecret;
}

/** The 32 key bytes of a `1:<base64>` vault key. */
function keyBytes(pair: string): Buffer {
  const [version, encoded = ""] = pair.split(":");
  const bytes = Buffer.from(encoded, "base64");
  if (version !== String(VERSION) || bytes.length !== 32) throw new Error("the keyed vault key is not version 1 with 32 bytes");
  return bytes;
}

/** `plain` sealed for `column` of row `id` of `table`, as the vault stores it (PRIV-002). */
export function seal(table: string, column: string, id: string, plain: string): Buffer {
  const nonce = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", keyBytes(keyedSecrets().vaultKey), nonce);
  cipher.setAAD(Buffer.from(`${table}|${column}|${id}|v${String(VERSION)}`, "utf8"));
  const sealed = Buffer.concat([cipher.update(plain, "utf8"), cipher.final(), cipher.getAuthTag()]);
  const version = Buffer.alloc(2);
  version.writeUInt16BE(VERSION);
  return Buffer.concat([version, nonce, sealed]);
}

/** The vault's blind-index digest of `value` for `purpose` (e.g. `email`) at key version 1. */
export function blindIndex(purpose: string, value: string): Buffer {
  const master = keyBytes(keyedSecrets().vaultIndexKey);
  const derived = createHmac("sha256", master).update(`${purpose}|v${String(VERSION)}`, "utf8").digest();
  return createHmac("sha256", derived).update(value, "utf8").digest();
}
