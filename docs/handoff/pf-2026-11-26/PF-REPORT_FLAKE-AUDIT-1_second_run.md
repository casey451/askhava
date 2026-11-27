# FLAKE-AUDIT-1 — Laftly nondeterministic test review

**6 likely flaky tests found (2 high, 4 medium).**

This is a count of new actionable test sites (including one journey/fixture-level site and one conditional payload finding), not six reproduced failures. The supplied TD-SEC-44 incident is already known and excluded from the count. High means strong evidence of a consequential timing or shared-state defect; medium means a plausible failure with some missing implementation detail or uncommon trigger. Do not treat the probabilities below as measured CI rates.

## Findings, in remediation priority order

Rust paths below are relative to `tests/integration/tests/`; browser paths are relative to `tests/e2e/`. Line numbers are the original source lines in the bundle, not the line numbers of `excerpts.md`. Where a declaration is outside the supplied excerpt, the helper and assertion identify the affected test; its exact test function name must be resolved in the full file before assigning ownership.

| # | Test (file:line, test name) | Pattern | How it fails (concrete sequence) | Estimated failure chance per run (with reasoning) | Smallest exact fix (tightens, never weakens) |
|---|---|---|---|---|---|
| 1 HIGH | `db_metrics.rs:187–204`, pool-acquisition contention metrics test (name omitted) | Timer starts before waiter actually begins | Holder acquires; waiter is spawned; parent starts `HOLD` timer. Waiter begins acquisition some milliseconds later; parent drops holder as soon as the timer expires. Actual waiter duration can be less than `HOLD`, so max/sum assertions fail despite correct metrics. | `P(actual measured wait < HOLD)`; potentially frequent under CPU/DB contention; no defensible numeric rate without traces. The margin is zero. | Establish an observable waiter-start/queued handshake at the acquisition measurement boundary, then hold for `HOLD` using a monotonic clock. Keep the two-acquire, max, sum and histogram assertions. Prefer a deterministic instrumented clock for exact bucket tests, with a separate real-pool contention test. |
| 2 HIGH | `j1_homeowner.spec.ts:183–205`, `quoteTextQueued` / J1 journey; `playwright.config.ts:21,38–39` | Clock-advancing spec is missing from `late`; shared clock vs wall-clock fixtures | During Arizona quiet hours the J1 journey advances the common clock to 08:30. Its filename does not match `j1_full*` or any other LATE glob, so it runs in main. A later main case seeds a session or link using real SQL `now()`, or signs a webhook using wall time. If the route checks the advanced clock, the fresh real-time fixture can already be expired or stale. | Quiet hours cover 12/24 hours, so up to roughly 50% exposure under uniform start-time assumptions, **not** a 50% failure rate. Actual probability also requires J1 to advance and a later affected case. | Add `**/j1_homeowner.spec.ts` to `LATE`; then make late-suite ordering independent by seeding every time-sensitive row using the app clock of the receiving server. Do not assume the late project itself resets time. Preserve expiry, authorization, and webhook-signature assertions. |
| 3 MEDIUM | `intake_order_photos.rs:529–541`, photo webhook queues IDs-only task (name omitted) | Broad negative substring in JSON containing an inbox ID | If `payload["inbox"]` is a random hex UUID, it can contain `555`; the test rejects a valid IDs-only payload even when no phone was stored. Other opaque IDs could cause the same issue. | About 0.55–0.63% per random UUID under uniform hex, depending on counting overlapping matches and UUID fixed positions; conditional on random inbox ID. The ID generator is outside the excerpt, so this is an actionable hypothesis, not proof. | Assert exact object equality with the expected inbox ID, message ID and media list (obtain the inbox ID from the fixture's actual inbound row); retain exact keys and type checks. Validate all fields and reject extras. This proves no phone or URL was stored without misreading an identifier as contact content. |
| 4 MEDIUM | `offdb_stream.rs:196–220`, request-log export writes one line (name omitted) | UTC midnight rollover / mixed clocks | A request is handled just before midnight UTC; writer queues and drains; line 208 reads today's date after midnight and searches the next day's directory. If storage partitions by the captured request date, the directory is missing. The fixture's stream clock is also initialized three hours ahead (`:88–95`), another potential partition mismatch if used for the path. | For a vulnerable interval of 1–5 s and uniform times, about 0.0012–0.0058% per run from midnight alone. If the +3h clock controls the partition while wall time supplies the assertion, disagreement could occur across a 3h window (12.5% exposure). Partition implementation not supplied. | Pin request-event time and stream clock to explicit instants; assert the exact intended UTC date partition, single file, matching request ID, route, status, and absence of sensitive data. Add a deterministic before/after midnight case using the same production partition contract. |
| 5 MEDIUM | `home_numbers_queries.rs:308–326`, cold/warm/expired home numbers query-count test (name omitted) | Real-time cache expiry between cold and warm reads | The cold request populates the cache. A CI pause longer than the cache window occurs before the warm GET. Warm path recomputes, correctly issuing extra statements, but the test requires exactly one. | `P(gap >= WINDOW)`; likely small if WINDOW is large, unknown without the constant and load distribution. The post-expiry sleep is not itself the principal problem. | Inject a monotonic cache clock. At identical time check cold/warm; advance to `WINDOW + 100ms`, insert the fourth job, then assert exactly three statements and count four. Preserve the query-budget requirements, including `(1,1)` warm. |
| 6 MEDIUM | `j1_homeowner.spec.ts:173–214`, `quoteTextQueued` / `quotesIn` | Fixed quiescence sleep substitutes for completion of the second quote notification | The first text creates token A; the second notification is delayed more than two seconds. Poll sees A unchanged over one two-second interval and returns A. The worker later creates B and revokes A before the browser opens A, causing a stale-link failure. | Load-dependent: probability of the update arriving after the stability interval but before use. No worker latency measurements supplied. Distinct from #2, but both affect the same journey; deduplicate as one task if ownership matches. | Wait for the two quote submissions and the expected notification/task terminal states for that job, including any explicit coalesced/suppressed outcome. Then read the newest persisted live link and verify it is not revoked. Keep the real served-page journey and all quote-count/status checks; do not simply increase the two-second sleep. |

**Counting note:** Two distinct hazards (#2 and #6) occur in the same J1 journey. Thus the six findings cover at most five distinct test functions/journeys; the exact count cannot be established without the omitted test declarations/call sites. The requested verdict counts actionable flaky test sites, not duplicated failure executions.

## Quoted evidence and fix details

### 1. Pool metrics: measured wait does not necessarily equal hold interval

`tests/integration/tests/db_metrics.rs:187–203`:

```rust
let held = pool.acquire().await.expect("first acquire");
let waiter = tokio::spawn({
    let pool = pool.clone();
    async move { pool.acquire().await.map(drop) }
});
tokio::time::sleep(HOLD).await;
drop(held);
// ...
assert!(after.wait_max_us >= hold_us, /* ... */);
assert!(after.wait_sum_us - before.wait_sum_us >= hold_us);
```

The spawn call is not an acquisition-start barrier. Even if the holder and waiter are on the same executor, the sleep starts before the waiter necessarily runs. An arbitrary larger sleep merely reduces the odds; a signal from the actual acquisition measurement point removes the assumption. The definition of `HOLD` and the histogram assertion beyond line 204 are omitted, so preserve those existing semantics when implementing the fix.

### 2. J1 advances the clock from the main project

`tests/e2e/j1_homeowner.spec.ts:184–187,201–204` explicitly says this file makes a forward clock move and warns about later short-lived links. The code is:

```ts
const at = new Date(await control.now());
const arizona = ((at.getUTCHours() + 17) % 24) * 60 + at.getUTCMinutes();
if (arizona >= 8 * 60 && arizona < 20 * 60) break;
await control.advanceClock(((8 * 60 + 30 - arizona + 1440) % 1440) * 60 + 5);
```

`tests/e2e/playwright.config.ts`:

```ts
const LATE = ["**/j1_full*.spec.ts", "**/j2_full*.spec.ts", "**/j1_j2_web_email*.spec.ts", "**/j3_*.spec.ts", "**/j4_*.spec.ts"];
// ...
{ name: "main", testIgnore: LATE },
{ name: "late", testMatch: LATE, dependencies: ["main"] },
```

The config explicitly describes the clock as shared and forward-only. One worker serializes execution but cannot undo an advance. Examples of vulnerable consumers in the bundle:

- `keyed.ts`, `sessionSql`: `SELECT decode('${hash}', 'hex'), 'business', id, now(), now() + interval '1 day'`.
- `business_signup_served.spec.ts:87–88`: creates a 15-minute login link using SQL `now()`.
- `keyed.ts`, `ownerSql`: license list date uses `(now() AT TIME ZONE 'America/Phoenix')::date`, which may differ from the app's current Arizona date.
- `j1_homeowner.spec.ts:168`: chooses a visit date from `Date.now()` rather than the already-available `control.now()`; `action_done_served.spec.ts:72` does the same with a ten-day margin. These may become past dates after enough advances.

Immediate config change:

```ts
const LATE = [
  "**/j1_homeowner.spec.ts",
  "**/j1_full*.spec.ts", "**/j2_full*.spec.ts",
  "**/j1_j2_web_email*.spec.ts", "**/j3_*.spec.ts", "**/j4_*.spec.ts",
];
```

Regression: prove every spec using forward clock advances is in the correct project, including helper-mediated advances; run reordered late cases against freshly seeded short-lived links. Whether the keyed server and Stripe/TOTP validation actually consume this same clock should be checked before changing their clock sources. Do not change signature expiry policy or add a tolerance to production.

### 3. IDs-only photo-task assertion

`tests/integration/tests/intake_order_photos.rs:536–541`:

```rust
let payload = &tasks[0].1;
let keys: Vec<&String> = payload.as_object().expect("object").keys().collect();
assert_eq!(keys, ["inbox", "media", "message"], "IDs only");
assert_eq!(payload["message"], message.as_str());
assert_eq!(payload["media"], serde_json::json!([media]));
assert!(!payload.to_string().contains("http") && !payload.to_string().contains("555"));
```

The first two value checks are already exact; the inbox value is the unverified field. A v4 UUID has about 26 candidate contiguous all-random three-hex windows in its hyphen-delimited representation, with a subset involving fixed version/variant characters. The union-bound estimate is roughly 0.6%; overlaps slightly reduce it. Force an otherwise-valid inbox ID such as `55500000-0000-4000-8000-000000000001` to demonstrate the problem if the production payload indeed serializes an inbox UUID. Replace the substring assertion with full expected JSON equality. Do not simply remove the privacy assertion or ignore opaque fields wholesale.

### 4. Request export directory uses a later day read

`tests/integration/tests/offdb_stream.rs:207–214`:

```rust
assert_eq!(run_due(&pool, &stream, &registry).await, 1);
let today = Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date();
let day = std::fs::read_dir(root.join(format!("request/{today}"))).expect("today's folder");
let files: Vec<_> = day.map(|entry| entry.expect("entry").path()).collect();
assert_eq!(files.len(), 1, "one line per request");
// ...
assert_eq!(line["request_id"].as_str(), Some(request_id.as_str()));
```

The helper at lines 88–90 creates `ManualClock::new(Timestamp::now() + SignedDuration::from_hours(3))`. The correct fix is to pin the time used by the **partition contract**, not simply to search both days or recursively look for any matching file. If the storage partition is defined from event time, assert that date exactly; if from export time, assert the explicitly advanced export date.

### 5. Warm cache assumes uninterrupted scheduling

`tests/integration/tests/home_numbers_queries.rs:312–326`:

```rust
let before = metrics.snapshot();
let warm = get_home(&router).await;
assert_eq!(moved(&before, &metrics.snapshot()), (1, 1), "only the banner");
assert!(warm.contains("Requests sent to businesses: 3"));
tokio::time::sleep(WINDOW + Duration::from_millis(100)).await;
bulk_jobs(&test_pool, 1).await;
// ...
assert_eq!(statements, 3, "the banner and the numbers again");
assert!(later.contains("Requests sent to businesses: 4"));
```

If the cache uses `std::time::Instant`, Tokio's paused timer alone will not freeze it. Inject the clock used by the cache, or use paused Tokio time only after confirming it is the cache's actual time source. No production query budget changes.

### 6. A stable token is not a completed worker operation

`tests/e2e/j1_homeowner.spec.ts:174–179,191–199` documents that each message mints a token and revokes the last:

```ts
while (Date.now() < deadline && quotePath(chat) === "") await new Promise((r) => setTimeout(r, 300));
if (quotePath(chat) !== "") {
  let path = "";
  while (path !== quotePath(chat)) {
    path = quotePath(chat);
    await new Promise((r) => setTimeout(r, 2_000));
  }
  return path;
}
```

`quotesIn` at lines 210–214 submits two quotes, asserts `quotes_ready` and returns this helper. That job state alone does not establish completion of notifications. Use a positive, job-scoped worker completion condition and assert link validity. Also replace the unbounded stability loop with a bounded, diagnostic condition wait.

## Known flake — do not count as new

`EXAMPLE_known_flake_td_sec_44.md` documents `app/server/tests/action_links_route.rs:545–548`, `!again.text.contains("990")`. The page includes a random CSRF hash and UUID/link material. For 64 independent hex characters, the union-bound approximation is `62 / 4096 = 1.51%`; additional random fields raise exposure. The example estimates about 2% or more per run. Fix by checking that exactly one quote remains, amount exactly 32,500 cents, and the rendered quote amount is the expected amount; also verify any privacy/visibility contract that the original assertion was intended to protect. No action-links source was included in the excerpt set, so this diagnosis is attributed to the supplied example and not independently reproduced here.

## Checked and fine (within the shown evidence)

- **Deterministic copy and redaction strings:** `quote_notes.rs:130–144`, `intake_boundary_count.rs:235–243`, `help_requests.rs:334–343`, `ai_zero_retention.rs:183–188`: checks operate on scrubbed text or specific content fields, not indiscriminately on entire opaque JSON/page objects.
- **Token-aware email assertion:** `pick_lapsed_email.rs:293–295` removes link tokens and asserts exactly one was removed before checking for the amount. This is the right general pattern; confirm the token parser still checks link structure separately.
- **Fixed DST/Arizona dates:** `quote_texts.rs:295–306` explicitly uses 2026-11-04 and `America/Chicago`; the expected Central time is standard time on that date. `confirm_reminders.rs:193–226` uses explicit `az(...)` inputs. No current date is read in those shown copy assertions.
- **Single captured time used for both operations:** `busy_for.rs:99–104` passes the same `now` to seeding and `set_busy`, then compares against exactly 72 hours with sub-millisecond precision allowance. It is not comparing independent wall-clock reads.
- **Readiness polling, not unconditional sleeps:** `intake_order_paths.spec.ts:98–104`, `j1_homeowner.spec.ts:145–151`, `webserver.ts:204–209` poll actual conditions. Their finite budgets can fail under extreme load, but their assertions do not assume completion after a single fixed sleep.
- **Database row-lock polling:** `fee_limit_race.rs:67–79` polls a DB condition. Similar 25/50ms loops in the excerpts should not be condemned solely because they use a sleep between polls.
- **Intentional delayed work:** `task_queue.rs:39–59` deliberately simulates slow/hanging handlers; `:98–108` polls the number of open tasks. These sleeps are not evidence of arbitrary readiness assumptions. Unbounded loops warrant separate watchdog review.
- **Isolation:** `repo/tests/integration/src/lib.rs` documents a fresh migrated database for every SQLx test. Repeated fixed fixture IDs across those independent databases are not inherently collisions. Do not mechanically randomize fixed fixtures.
- **High-entropy IDs:** v4 UUIDs and 32-byte random tokens are appropriate for isolation. Collision risk is negligible for ordinary suite sizes. Do not seed cryptographic randomness globally to fix substring assertions.
- **Stand-in faults:** `repo/tests/e2e/fixtures.ts` clears faults before and in `finally` after each use of the `control` fixture; `playwright.config.ts` uses one worker and zero retries. This mitigates fault-state leakage in tests that use that fixture.
- **Race tests with a real held DB lock:** `quote_notes.rs:307–315,336–347`, `page_pick.rs:417–428`, `help_requests.rs:448–460`, `job_facts.rs:134–150`: a delayed waiter generally does not make the final correct result wrong. Their `!is_finished()` check can pass before the waiter ever attempts the contested lock, however. This is a false-pass/test-strength problem, not counted as a proven flaky red. Strengthen with `pg_blocking_pids` or a test-only lock-acquisition barrier before committing.
- **Existing tracing mitigation:** `log_capture.rs` installs one process-global subscriber with `OnceLock` and rebuilds callsite interest; it specifically addresses the earlier subscriber-cache race. Thread-local capture remains appropriate only for logs produced on the capture thread.

## Can't tell — targeted follow-up, not included in the six

| Site | Evidence / uncertainty | Required check and correction if confirmed |
|---|---|---|
| `web_chat.spec.ts:33–41,49–52` | `Math.floor(Math.random() * 41)` samples only 41 numbers with replacement. Avoiding fixed 0146 prevents a known cross-spec collision but not collisions within this spec. Two draws collide 2.44%; five draws collide ~22.4%. | Inspect call sites, whether the number is stored/consented/opted out, and cleanup. Allocate unique reserved numbers per test case and width; preserve intentional reuse only when explicitly tested. |
| `web_chat_errors.spec.ts:19–22` | Five numbers per width; two draws at the same width collide 20%. Different width groups are disjoint for the 320-vs-other split, but 768 and 1280 share a group. | Verify test matrix and persistent state. Use a unique case+width number allocator (not independent random draws). |
| `radius_matching.rs:274–294` | `!text.contains("86406")` scans every audit column. If a random UUID column is present it may accidentally include the ZIP, roughly 0.001% per UUID. `text != "30"` is also not a complete nested-value privacy check. | Inspect query projection and row schema. Assert exact permitted field names and expected typed metadata; assert no extra raw before/after values and inspect complete typed sensitive fields. Do not drop the audit privacy invariant. |
| `notice_log.rs:222–225` | Whole rendered `page` rejects `555` and `0198`. Random job/business/mailbox IDs are seeded at :107–108; whether they appear in the rendered page is unknown. | Inspect the template and actual response. For UUIDs/token attributes assert exact legitimate IDs or valid structures; keep full PII and forbidden-content checks on visible text, link destinations and relevant attributes. If a random token is rendered, force a matching substring to demonstrate the flake. |
| `business_numbers.rs:408–410` | Whole billing HTML rejects `4444` and `0005`; random payment IDs exist but the renderer receives brand/last-four-only objects, and `billing_page` receives `None` for its third argument. | Inspect template and statement fields for other random IDs. Keep exact `shown` vector at :397 and assert exact visible card list plus no serialized Stripe method IDs anywhere. No demonstrated random content in the HTML yet. |
| `summary_step.rs:185–197` | Exact body is already asserted; `number` is a dynamic request number and the redundant `!got.contains("555")` could reject legitimate request 555/1555. | Inspect allocator/fixture initial value. If variable, use exact generated body plus field-level phone redaction; deliberately test a request numbered 555. |
| `intake_order_paths.spec.ts:42` and `j1_homeowner.spec.ts:125` | ROC number random from 899,999 possibilities, two draws per seed call. Collision probability 1/899999 per pair; cross-test rows persist. | Inspect uniqueness constraints and all seed calls. Use run-local reserved license numbers, distinct from imported fixture records and each other. Avoid changing production license-number rules. |
| `owner_session.ts:126–136`, `j2.ts:187`, `keyed_stripe.spec.ts:55–67` | TOTP and Stripe signatures use wall clock; server validation clock implementation absent. The signature at :65 reuses time from :55 after several requests. | Determine whether verification intentionally uses wall clock or app clock, and exact skew/window. Test boundary cases with explicit verifier time. Re-sign successful deliveries immediately before sending, while keeping the stale/mismatched-signature negative tests exact. |
| `health.rs:95–102` | Sets `done_at = now() - interval '30 seconds'`, then probes; age assertion is outside excerpt. | Inspect next lines. If exact 30 is expected, use fixed injected time and explicitly check 29/30/31-second rounding boundaries. If a robust range is already used, close candidate. |
| `health.rs:151–157` | Real 151-second timeout surrounding a 150-second liveness limit. A delayed poll or DB connection attempt might exceed the one-second margin. | Inspect `watch_web` timing and pool timeout; use a deterministic unreachable pool and paused/injected time to assert nontermination before the limit and termination at it. Keep an outer safety watchdog distinct from the behavior assertion. |
| `optout_wrote_again` test at `:172–185` (resolve file name from full source) | Two rows seeded with `at(0)` and random UUIDs; asserts `rows[0]` contains first UUID. Ordering query is outside the excerpt. | Verify `listed()` has a stable ordering key and whether chronology is required; if order is not semantic, assert exact unordered set of both row identities. If required, use explicit monotonic fixture timestamps and assert defined order. |
| `webserver.ts:212–216`, stand-in URL `127.0.0.1:4010` | Port free check is not a reservation; a parallel suite/process can claim it before bind. | Inspect CI process/network isolation, actual port allocation and teardown. If concurrent suites share a namespace, allocate distinct port sets with atomic bind or serialize complete suite lifetimes. Do not bypass stale-process detection. |
| `log_capture.rs`, `thread_local! CURRENT` | Capture does not follow logs onto other threads. Whether any positive log assertion runs the subject on a spawned/migrating task is not established. | Inspect runtime flavor and call sites; if required, scope capture by test/task identity rather than OS thread; prove logs from the tested task are captured and unrelated ones excluded. |
| `business_contacts.rs:163,196`, `optout_flow.rs:329`, other fixed lock delays | Commit-after-sleep tests may fail to actually exercise the intended overlap if contender is slow. | Test with observed blocked backend before release and keep existing final-state checks. Treat as coverage hardening unless evidence shows incorrect red results. |

## Implementation and verification gates

1. Assign independent owner-approved test fixes; do not alter unrelated product behavior or relax CI checks. First repair #1 and #2. Then #3 after verifying its inbox generator, #4, #5, #6.
2. Reproduce deterministically before patching: delay waiter startup; start J1 in Arizona quiet hours; force an ID containing `555`; cross UTC midnight; advance cache past TTL; delay the second quote notification beyond two seconds. Confirm each failing assertion matches its proposed mechanism.
3. Verify the corrected test passes under the forced condition, then deliberately break the corresponding application behavior and confirm the corrected test still fails. Preserve privacy, payment, query-count, and authorization coverage.
4. Add a preflight lint/review rule for broad short numeric negative substrings on serialized structured objects or token-bearing content; distinguish legitimate visible copy checks from unsafe whole-object checks. Also enforce clock-domain consistency in time-sensitive fixtures and explicit completion signals in concurrency tests.
5. Track original failing test, deterministic reproduction, fix commit, and repeated-run results in the audit ledger. Do not classify a real product regression as a flake merely because a rerun passes.

## Scope and limitations

Static review of the uploaded `excerpts.md` (412 merged excerpt blocks representing 212 unique file paths; README reports 615 signal hits), the supplied known-flake example, and shared fixture sources. Signal-focused search and context inspection, not a complete line-by-line repository audit. The package omits production implementations, full test functions, schema constraints, CI histories, and some test call sites. No test suite was run, no repo accessed, no code modified, no empirical failure probabilities measured. Excerpt-only candidates have been explicitly qualified. In particular, the photo payload inbox generator and off-DB partition contract need verification, and the exact unique test-function count cannot be established from the excerpts.
