# FLAKE-AUDIT-1 verdicts (Opus, read-only, origin/main 404e829ae)

Method: full test + exercised code read on a detached worktree; kit-state `last_failure`/`requeues`/`plan_reservations` grepped for every test name. No test run; probabilities are reasoned estimates.

## The 8 findings

| # | Test | Verdict | Why (sequence, rough probability) | CI history |
|---|---|---|---|---|
| 1 | `tests/integration/tests/db_metrics.rs:187-203` `td_ops_41_a_held_connection_makes_the_second_acquire_wait` | **CONFIRMED (rare)** | `sqlx::test` runs a current-thread runtime. `tokio::time::sleep(HOLD)` fixes its deadline (t0+300 ms) *before* the parent yields; the waiter's `acquire()` starts its own `Instant` (sqlx `acquired_after_secs`, read by `DbMetrics::record_acquire`, `app/core/src/db_metrics.rs:158`) only after the yield (t1). Measured wait ≈ 300 ms + timer lateness + return-to-pool − (t1−t0). Fails `wait_max_us >= hold_us` when the OS preempts the thread for more than ~1 ms in that microsecond window. Roughly 0.01-0.1 % per run on a loaded runner. | No failure recorded. |
| 2 | `tests/e2e/keyed_server.spec.ts:32-56` (via J1 clock move) | **WRONG** | The sign-in link route uses `SystemClock` (`app/server/src/business_signin.rs:124`, `SystemClock::now()` = `Timestamp::now()`, `app/core/src/clock.rs:52`), not the test-control offset clock. The seed uses DB `now()`. J1's forward move cannot expire this link. D-5809 had already reached the same conclusion. | No failure recorded. |
| 3 | `tests/e2e/j1_homeowner.spec.ts:189-214` `quoteTextQueued` | **UNLIKELY** (heuristic, not proven) | The daytime 8 s bound is generous: worker idle poll is 200 ms to 5 s (`app/core/src/tasks.rs:456-457`). The 2 s quiet window could return a link that is revoked later only if the update text's task becomes due more than 2 s after the first-quote text. Not shown. T861 (MERGE_PENDING) owns this file and rewrites `quotePath`, so it is the place to add a terminal-state wait. | No J1 homeowner failure; only `j1_keyboard`/`j1_reduced`. |
| 4 | `tests/integration/tests/link_request.rs:468-486` `td_call_23_a_failed_link_step_still_answers_the_dial` | **UNLIKELY** | The forced link-step WARN callsite is reached only by this test. Its own `set_default` runs first and rebuilds interest with `Lines` registered (`enabled` = true for non-sqlx). The interest-cache race in `tests/integration/src/log_capture.rs:3-8` needs another thread to first-register the same callsite during that rebuild. The needles scan log text, not random bytes. Migrating to `log_capture::start` is hygiene, not a flake fix. | T370/T414 failures were the *other* test (`..._blocked_call_that_connects...`, whole-row scan), already fixed by T419. |
| 5 | `tests/integration/tests/intake_order_photos.rs:538-541` `td_int_49_a_picture_text_is_stored_confirmed_and_put_on_the_card` | **CONFIRMED** | `payload["inbox"]` is `messaging.inbox.id`, `DEFAULT gen_random_uuid()` (`migrations/0043_messaging_inbox_delivery.sql:23`). The hyphenated UUID has about 20 usable 3-hex windows, so the chance of "555" is about 20/4096 ≈ **0.5 % per run**, with correct code. `message`/`media` are the fixed `MM…01`/`ME…01`. | No failure recorded yet. |
| 6 | `tests/integration/tests/message_log_retention.rs:144-149` | **UNLIKELY** | The regex `555.?0193` scans whole outbox rows: uuids, the 64-hex `idempotency_key` and a 32-byte digest. That is about 1e-6 per run. Not worth a task. | none |
| 7 | `tests/integration/tests/offdb_stream.rs:196-220` | **UNLIKELY** | The folder date comes from `RequestLine.at = Timestamp::now()` (system, `app/server/src/offdb_stream.rs:438`, used at `:137-140`), not the +3 h `ManualClock`. Only a UTC-midnight crossing between the request and the test's `Timestamp::now()` (about 1 s) fails it, about 1e-5. | Many E2E `last_failure` hits are just worker log noise (`ops.offdb_stream` task lines), not this test. |
| 8 | `tests/integration/tests/home_numbers_queries.rs:308-326` | **UNLIKELY** | `WINDOW` = 1.5 s (`:37`). Warm GET follows cold GET immediately, so this needs a stall of 1.5 s or more between cache fill and warm lookup. That is rare, and a real fix means injecting a clock into production `HomeNumbersCache`. | none |

## Can't-tell rows checked

| Row | Verdict | Why |
|---|---|---|
| `web_chat.spec.ts:39-40` `fakeNumber` (41 numbers, with replacement) | **UNLIKELY** | Two draws per run (`:133`, `:171`), so a repeat is about 2.4 %, but a repeat is harmless. `send_code` keys on the session (`app/intake/src/web_chat.rs:606-631`). The Verify stand-in resets attempts on every new send (`stand_ins/twilio.py:366-374`). Test 2 reads only rows `since(from)`. The earlier real flake (T541 attempt 2: another spec opting a number out) was fixed by narrowing the range to 0101-0141. |
| `web_chat_errors.spec.ts:20-21` | **WRONG** | Widths are `[320, 1280]` (`:53`). That is one draw per 5-number bucket per run, so no collision is possible. |
| `summary_step.rs:194-197` job number vs "555" | **WRONG** | The job number comes from `jobs.job_number_next` seeded 1001 (`migrations/0034_jobs_numbers.sql:22`). With a fresh DB per test, three requests give 1001-1003, which is deterministic. |
| Quote dates from `Date.now()` (J1 `:168`, `action_done_served.spec.ts:70-76`) | **WRONG** | UTC now+24 h is always at least Arizona tomorrow. `action_done_served` (+10 d) sorts before J1, and the main-project offset is at most about 12.5 h. |

## Fixes for the CONFIRMED (tighten only)

1. `db_metrics.rs` (about 6 lines): the waiter does `started.send(())` immediately before `pool.acquire()`. The parent does `ready.await` and only then `tokio::time::sleep(HOLD)`. On the current-thread runtime the waiter's sqlx start instant then always comes before the hold starts, so wait ≥ HOLD is deterministic. All assertions stay unchanged: acquires = 2, max ≥ hold, sum ≥ hold, bucket slot, deltas sum, pool (1,1).
2. `intake_order_photos.rs` (about 8 lines): read `SELECT id FROM messaging.inbox WHERE message_sid = $1` (the `message` SID). Assert `*payload == json!({"inbox": inbox.to_string(), "media": [media], "message": message})`, an exact whole-payload equality that is strictly stronger than the key list plus the substring check. Keep `!contains("http")`. Replace `!contains("555")` with absence of the setup's full fake phone in its E.164 and 10-digit forms. Keep every other assertion.

Owners: `db_metrics.rs` is T416 (DONE); `intake_order_photos.rs` is T675 (DONE). Neither has a QUEUED owner, so a **harness** task is needed.

## Batching

There should be one small harness task, on Opus with Opus review: `tests/integration/tests/db_metrics.rs` + `tests/integration/tests/intake_order_photos.rs` + `docs/decisions.md`. That is about 15 test lines and about 6 decision lines. Optional, same task, still small: the `offdb_stream.rs` midnight guard (read the date before and after the request and assert they are equal, about 4 lines). Leave out #6 and #8.

## J1 outside LATE

No queued task moves J1 into `LATE`. T852/T853 add new `j1_full_variants`/`j2_full_variants` specs that already match LATE. T120 (QUEUED, owns `playwright.config.ts`) adds only `**/sentinel*.spec.ts` (D-5876). T861 (MERGE_PENDING) changes only J1's text observation. Because #2 is WRONG (sign-in uses the system clock), this is hardening only. If it is wanted, a one-glob plan change can ride on T120's config edit (add `**/j1_homeowner.spec.ts`; T120 already edits `:21`), consistent with D-5809's "later hardening". It is not a flake fix.
