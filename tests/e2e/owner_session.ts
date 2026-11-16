/// <reference types="node" />
// The one owner's sign-in for every e2e spec (T303; SEC-005, SEC-043, SEC-044; D-3744, D-3844).
//
// `bootstrap-owner` works once per database and Playwright runs every spec on one database, so this
// helper is the only place that runs it: the first `signInOwner` of a run calls the server binary's
// `bootstrap-owner` command (`app/server/src/main.rs`) with a fake `ASKHAVA_OWNER_BOOTSTRAP_TOKEN` of
// 48 characters in its environment, a fake random password on standard input and the run's database
// as the migration URL, and reads the authenticator secret it prints. The password, that secret and
// the last TOTP step used are kept in a file (mode 0600) in the system temporary folder, named by
// a SHA-256 hash of the run's `controlSecret()` (tests/e2e/webserver.ts), so a later spec file or a
// restarted worker of the same run reuses them and a new run never finds them. Nothing in it is
// printed or logged; every value is fake.
//
// The server takes each TOTP step once (`use_totp_step`, app/portals/src/models.rs) and accepts the
// step before and after its own (totp-rs's skew of one), so `ownerCode` uses the current step, or
// the next one when the current one was used, and waits for the clock to reach the step before a
// step it may use when both were used. A database that already has an owner this run did not make
// (a local rerun: webserver.ts makes a new scratch folder per run but migrates the same database)
// fails at once with what to do. Owner POSTs are re-sent with a same-origin https Origin, as the J1
// specs do (D-1250), since the suite serves plain http.
import { spawnSync } from "node:child_process";
import { createHash, createHmac, randomBytes } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { controlSecret, webPort } from "./webserver";

const REPO_ROOT = join(import.meta.dirname, "..", "..");
const STEP_SECONDS = 30;

/** What the run keeps about its owner: fake values only. */
interface OwnerCache {
  password: string;
  secret: string;
  lastStep: number;
}

/** This run's cache file: keyed on the run's control secret, so no other run finds it. */
function cachePath(): string {
  const key = createHash("sha256").update(controlSecret()).digest("hex").slice(0, 32);
  return join(tmpdir(), `askhava-e2e-owner-${key}.json`);
}

function save(cache: OwnerCache): void {
  writeFileSync(cachePath(), JSON.stringify(cache), { mode: 0o600 });
}

/** Runs `sql` through psql on DATABASE_URL and returns its unaligned, tuples-only output. */
function psql(sql: string): string {
  const done = spawnSync("psql", [process.env.DATABASE_URL ?? "", "-v", "ON_ERROR_STOP=1", "-q", "-A", "-t", "-c", sql], { encoding: "utf8" });
  if (done.status !== 0) throw new Error(`psql failed with status ${String(done.status)}`);
  return done.stdout.trim();
}

/** The environment `bootstrap-owner` runs with: the suite's CI settings and a fake token. */
function bootstrapEnv(): NodeJS.ProcessEnv {
  const database = process.env.DATABASE_URL ?? "";
  const inherited = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("ASKHAVA_") && !key.startsWith("E2E_KEYED_")));
  return {
    ...inherited,
    ASKHAVA_APP_ENV: "ci",
    ASKHAVA_DATABASE_URL: database,
    ASKHAVA_MIGRATION_DATABASE_URL: database,
    ASKHAVA_OWNER_BOOTSTRAP_TOKEN: randomBytes(24).toString("hex"),
  };
}

/** Creates the one owner through the server binary and keeps what it printed. */
function bootstrap(): OwnerCache {
  if (psql("SELECT count(*) FROM portals.owner") !== "0") {
    throw new Error("The end-to-end database already has an owner from an earlier run: drop and recreate it, then run again.");
  }
  const password = `fake owner ${randomBytes(12).toString("hex")}`;
  const done = spawnSync(join(REPO_ROOT, "target", "debug", "server"), ["bootstrap-owner"], {
    cwd: REPO_ROOT,
    env: bootstrapEnv(),
    input: `${password}\n`,
    encoding: "utf8",
  });
  if (done.status !== 0) throw new Error(`bootstrap-owner failed with status ${String(done.status)}`);
  const secret = /Authenticator secret: ([A-Z2-7]+)/.exec(done.stdout)?.[1];
  if (secret === undefined) throw new Error("bootstrap-owner printed no authenticator secret");
  const cache = { password, secret, lastStep: 0 };
  save(cache);
  return cache;
}

/** The run's owner: from the cache file, else bootstrapped now. */
function owner(): OwnerCache {
  const path = cachePath();
  return existsSync(path) ? (JSON.parse(readFileSync(path, "utf8")) as OwnerCache) : bootstrap();
}

function base32(text: string): Buffer {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = "";
  for (const char of text.replace(/=+$/, "")) bits += alphabet.indexOf(char).toString(2).padStart(5, "0");
  const bytes = bits.match(/.{8}/g) ?? [];
  return Buffer.from(bytes.map((byte) => parseInt(byte, 2)));
}

/** RFC 6238's 6-digit SHA-1 code for `secret` (base32) at 30-second step `step`. */
export function totpCode(secret: string, step: number): string {
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const hash = createHmac("sha1", base32(secret)).update(counter).digest();
  const offset = (hash[19] ?? 0) & 0x0f;
  const word = hash.readUInt32BE(offset) & 0x7fffffff;
  return String(word % 1_000_000).padStart(6, "0");
}

/**
 * A TOTP code for a step the server has not taken yet, recorded as used: the current step, else
 * the next (inside the server's skew), else after waiting for the clock to move on.
 */
export async function ownerCode(): Promise<string> {
  const cache = owner();
  for (;;) {
    const now = Math.floor(Date.now() / 1000 / STEP_SECONDS);
    const step = Math.max(cache.lastStep + 1, now);
    if (step <= now + 1) {
      save({ ...cache, lastStep: step });
      return totpCode(cache.secret, step);
    }
    await new Promise((resolve) => setTimeout(resolve, (step - 1) * STEP_SECONDS * 1000 - Date.now() + 500));
  }
}

const routed = new WeakSet<Page>();

/** Re-sends every owner POST of `page` with a same-origin https Origin (D-1250). */
export async function ownerOrigin(page: Page): Promise<void> {
  if (routed.has(page)) return;
  routed.add(page);
  await page.route("**/owner/**", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const headers = { ...request.headers(), origin: `https://${new URL(request.url()).host}`, "sec-fetch-site": "same-origin" };
    return route.fulfill({ response: await route.fetch({ headers, maxRedirects: 0 }) });
  });
}

/**
 * Signs the run's owner in on `page` at `baseUrl` (the suite's web binary by default; `keyedUrl()`
 * for the Stripe steps) with the password and a fresh TOTP code, ending on the step-up page.
 */
export async function signInOwner(page: Page, baseUrl = `http://127.0.0.1:${String(webPort())}`): Promise<void> {
  const { password } = owner();
  await ownerOrigin(page);
  await page.goto(`${baseUrl}/owner/sign-in`);
  await page.getByLabel("Password").fill(password);
  await page.getByLabel("Code", { exact: true }).fill(await ownerCode());
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await page.waitForURL(/\/owner\/step-up$/);
}
