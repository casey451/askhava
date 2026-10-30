// Playwright fixtures for the e2e and accessibility journeys (SEC-035; docs/test-design.md e2e layer, D-453).
//
// `control` drives the test-control router that a `test-control` build of the web binary serves
// (app/core/src/test_control.rs): read and advance the clock, and read the texts the Twilio
// stand-in received, from its own sent log, never from the app's tables (D-491). Every call sends
// the per-run bearer secret; without it the router answers 404 like an absent route. The secret is
// the `controlSecret` option, which T227's playwright.config.ts sets from the value it gave the
// web binary as ASKHAVA_TEST_CONTROL_SECRET; requests go to the config's `baseURL`. Specs import
// `test` and `expect` from here instead of from @playwright/test. Fake data only.
import { test as base, expect, type APIRequestContext } from "@playwright/test";

/** One text the Twilio stand-in accepted, as its Messages list returns it. */
export interface SentText {
  sid: string;
  to: string;
  from: string;
  body: string;
  status: string;
  direction: string;
}

/** Filters for the sent log: the recipient and the app's derived message key (OPS-017). */
export interface SentFilter {
  to?: string;
  key?: string;
}

/** The test-control router, reached with the per-run bearer secret (SEC-035). */
export interface Control {
  /** The app's current time, ISO 8601 in UTC. */
  now(): Promise<string>;
  /** Moves the app's clock forward (web and worker share it); returns the new time. */
  advanceClock(seconds: number): Promise<string>;
  /** The texts the stand-in carrier received, oldest first. */
  sentTexts(filter?: SentFilter): Promise<SentText[]>;
}

interface Now {
  now: string;
}

interface SentLog {
  messages: SentText[];
}

/** Fixtures each test gets. */
export interface ControlFixtures {
  control: Control;
}

/** Options the Playwright config sets once per run. */
export interface ControlOptions {
  controlSecret: string;
}

async function readJson<T>(reply: Awaited<ReturnType<APIRequestContext["get"]>>, what: string): Promise<T> {
  if (!reply.ok()) {
    throw new Error(`test-control ${what} answered ${String(reply.status())}; is the web binary built with --features askhava-core/test-control and given the same secret?`);
  }
  return (await reply.json()) as T;
}

function makeControl(request: APIRequestContext, secret: string): Control {
  const headers = { authorization: `Bearer ${secret}` };
  return {
    async now() {
      const reply = await request.get("/__test/clock", { headers });
      return (await readJson<Now>(reply, "clock")).now;
    },
    async advanceClock(seconds: number) {
      if (!Number.isInteger(seconds) || seconds < 0) {
        throw new Error("advanceClock takes a whole, non-negative number of seconds");
      }
      const reply = await request.post("/__test/clock/advance", { headers, data: { seconds } });
      return (await readJson<Now>(reply, "clock advance")).now;
    },
    async sentTexts(filter: SentFilter = {}) {
      const params: Record<string, string> = {};
      if (filter.to !== undefined) params.to = filter.to;
      if (filter.key !== undefined) params.key = filter.key;
      const reply = await request.get("/__test/messages", { headers, params });
      return (await readJson<SentLog>(reply, "messages")).messages;
    },
  };
}

/** Playwright's `test` with the `control` fixture and the `controlSecret` option. */
export const test = base.extend<ControlFixtures, ControlOptions>({
  controlSecret: ["", { option: true, scope: "worker" }],
  control: async ({ request, controlSecret }, use) => {
    if (controlSecret.length < 32) {
      throw new Error("set controlSecret in the Playwright config to the run's ASKHAVA_TEST_CONTROL_SECRET");
    }
    await use(makeControl(request, controlSecret));
  },
});

export { expect };
