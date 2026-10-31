/// <reference types="node" />
// Starts the app for every e2e spec and stops it after the run (T227, D-491, SEC-035).
//
// Playwright's `globalSetup` in playwright.config.ts runs the default export once before the first
// spec and calls the function it returns after the last. The setup (1) starts the stand-in server
// (`python3 -m stand_ins.server`, SEC-032), (2) builds the web and worker binaries with
// `--features askhava-core/test-control`, (3) applies the migrations with `server migrate`, (4) starts
// the worker and the web binary against the Postgres in DATABASE_URL (the CI service, role askhava_ci;
// the Ci job creates the database and roles first), and (5) waits until the web binary answers on its
// port. Both binaries get the per-run bearer secret from `controlSecret()` and one shared clock file,
// so an advance made through the web binary also moves the worker's clock. The teardown sends SIGTERM
// to all three and waits for them to exit. Fake data only: the secrets are random per run, kept in
// memory and in this process's environment, and never printed. A start-up failure names the process
// and its exit code, never an environment value.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

const SECRET_KEY = "ASKHAVA_TEST_CONTROL_SECRET";
const REPO_ROOT = join(import.meta.dirname, "..", "..");
const STAND_IN_PORT = 4010;
const START_TIMEOUT_MS = 60_000;

/** The port the web binary listens on: `E2E_PORT`, else 3000. */
export function webPort(): number {
  return Number(process.env.E2E_PORT ?? "3000");
}

/**
 * The run's test-control secret. The first call (in the Playwright main process, while it loads the
 * config) makes a random one and puts it in the environment, which the worker processes inherit, so
 * the config, the fixtures and the web binary all see the same value (SEC-035).
 */
export function controlSecret(): string {
  process.env[SECRET_KEY] ??= randomBytes(24).toString("hex");
  return process.env[SECRET_KEY];
}

function requireEnv(name: string): string {
  const value = process.env[name];
  if (value === undefined || value === "") {
    throw new Error(`${name} must be set to the CI Postgres URL (the Ci job's service)`);
  }
  return value;
}

/** The environment both binaries start with: CI settings, the stand-ins, a shared clock file. */
function appEnv(scratch: string): NodeJS.ProcessEnv {
  const database = requireEnv("DATABASE_URL");
  return {
    ...process.env,
    PORT: String(webPort()),
    ASKHAVA_APP_ENV: "ci",
    ASKHAVA_DATABASE_URL: database,
    ASKHAVA_MIGRATION_DATABASE_URL: database,
    ASKHAVA_ADAPTER_MODE_TWILIO: "stand_in",
    ASKHAVA_TWILIO_BASE_URL: `http://127.0.0.1:${String(STAND_IN_PORT)}/twilio`,
    ASKHAVA_STORAGE_BASE_URL: `file://${join(scratch, "storage")}`,
    ASKHAVA_ID_SECRET: randomBytes(24).toString("hex"),
    ASKHAVA_TEST_CONTROL_CLOCK_FILE: join(scratch, "clock"),
    [SECRET_KEY]: controlSecret(),
  };
}

function run(command: string, args: string[], env: NodeJS.ProcessEnv): void {
  const done = spawnSync(command, args, { cwd: REPO_ROOT, env, stdio: "inherit" });
  if (done.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with status ${String(done.status)}`);
  }
}

/** Rejects if `child` exits before it is stopped, so a crash during start-up is not a timeout. */
function exitedEarly(name: string, child: ChildProcess): Promise<never> {
  return new Promise((_, reject) => {
    child.once("exit", (code) => {
      reject(new Error(`${name} exited during start-up with code ${String(code)}`));
    });
  });
}

function canConnect(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = connect({ port, host: "127.0.0.1" });
    socket.once("connect", () => {
      socket.destroy();
      resolve(true);
    });
    socket.once("error", () => {
      resolve(false);
    });
  });
}

async function waitForPort(port: number, deadline: number): Promise<void> {
  while (Date.now() < deadline) {
    if (await canConnect(port)) return;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`nothing answered on port ${String(port)} within ${String(START_TIMEOUT_MS / 1000)} seconds`);
}

function stop(child: ChildProcess): Promise<void> {
  return new Promise((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) {
      resolve();
      return;
    }
    child.once("exit", () => {
      resolve();
    });
    child.kill("SIGTERM");
  });
}

/** Playwright `globalSetup`: starts everything and returns the teardown. */
export default async function setup(): Promise<() => Promise<void>> {
  const scratch = mkdtempSync(join(tmpdir(), "askhava-e2e-"));
  const env = appEnv(scratch);
  const children: ChildProcess[] = [];
  const teardown = async (): Promise<void> => {
    await Promise.all(children.map(stop));
    rmSync(scratch, { recursive: true, force: true });
  };
  try {
    run("cargo", ["build", "-p", "askhava-server", "--bins", "--features", "askhava-core/test-control"], env);
    run(join(REPO_ROOT, "target", "debug", "server"), ["migrate"], env);
    const start = (name: string, command: string, args: string[]): Promise<never> => {
      const child = spawn(command, args, { cwd: REPO_ROOT, env, stdio: "inherit" });
      children.push(child);
      return exitedEarly(name, child);
    };
    const crashes = [
      start("stand-ins", "python3", ["-m", "stand_ins.server", "--port", String(STAND_IN_PORT)]),
      start("worker", join(REPO_ROOT, "target", "debug", "worker"), []),
      start("web", join(REPO_ROOT, "target", "debug", "server"), []),
    ];
    const deadline = Date.now() + START_TIMEOUT_MS;
    await Promise.race([
      Promise.all([waitForPort(STAND_IN_PORT, deadline), waitForPort(webPort(), deadline)]),
      ...crashes,
    ]);
  } catch (error) {
    await teardown();
    throw error;
  }
  return teardown;
}
