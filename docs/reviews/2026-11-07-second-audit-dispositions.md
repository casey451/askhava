# Second repository audit of November 7, 2026: dispositions

**Input:** a second external repository-wide code review, pinned to `main` at `0172b97` (T097, #322) with build state `f6092ab`, plus a separate review of PR #323 (T191) at head `ccb3a6e`, received as an attachment. Treated as untrusted input: every finding and every listed gap was checked against `main` at `2e7439c`, where #323 has merged, before anything was planned. B06 to B08 are therefore findings against `main` now. **Plan change:** D-1830 to D-1840, branch `claude/plan-audit2`.

Verdicts: **confirmed** (the code on `main` 2e7439c shows it), **gap confirmed** (a documented missing owner), **already owned** (a queued task's goal and files cover it), **rejected** (not reproducible on `main`). No finding was rejected.

## Findings

| Finding | Verdict | Evidence on `main` 2e7439c | Owner | Acceptance (test-design) | Decision |
|---|---|---|---|---|---|
| B01 Sign-in link cap and code-try cap are check-then-act | Confirmed | `app/portals/src/business_auth.rs:459` counts links with `models::links_since` and `:466` inserts afterwards; `:659`–`:668` count `auth_failed` events, `:681` calls Verify, `:685` records the failure afterwards; no lock or reservation. T089 owns only `limits.rs` | T272 (new); T089's goal now says it never re-counts | TD-SEC-38: 20 simultaneous requests with one allowance left admit one; Verify failures, timeouts and crashes use a try; a concurrent right and wrong code never both sign in | D-1832 |
| B02 Photo plaintext can outlive a rolled-back audit | Confirmed | `app/privacy/src/photos.rs:427` takes the caller's connection, `:449` opens, `:450` audits on that connection, `:451` returns the bytes; the vault's `ensure_bare` (`app/privacy/src/vault.rs:411`) is not used. No photo route is served yet (D-1791) | T273 (new) | TD-PRIV-17: SQLx transaction and raw `BEGIN` refused; bytes only after a committed audit; audit failure, dead link, storage failure | D-1833 |
| B03 Contact purge not serialized with new requests or holds | Confirmed (source; the schedule needs the barrier test) | `app/intake/src/purge.rs:93` decides on reads, `:112` returns the contact, `:153` `run_purge` then calls `clear`, and `app/privacy/src/purge.rs:113` clears in its own transaction; no shared lock with `lock_homeowner` (`app/intake/src/models.rs:598`) or hold placement (`app/privacy/src/purge.rs:38`) | T274 (new) | TD-PRIV-18: barrier tests for a new request, a new hold and a pending hold task between read and clear; job and jobless paths; no deadlock | D-1834 |
| B04 PROBLEM words are never stored | Confirmed | `app/jobs/src/followups.rs:72` (`Inbound` holds the inbox ID and safety flag), `:236` (`flag_problem` names the inbox row); `messaging.inbox` has no body (D-1801); D-1810 says the digest reads the words from it; nothing calls `answer_check_in` (D-1811 (3)) | T277 (new); T204 reads through it | TD-JOB-12: a fictional PROBLEM through the real route, restart, owner-only read returns the scrubbed words for the right job; no business or homeowner read | D-1835 |
| B05 Local write-once storage shares one temporary file between writers | Confirmed | `app/adapters/src/storage.rs:80`–`:84`: the temporary name is `<name>.<key>.part`, written with `std::fs::write` (truncate), then `hard_link`ed; a racing retry rewrites the inode already published | T275 (new, harness) | TD-SEC-39: same-key same-bytes and different-bytes puts with a continuous reader: never partial, never changed | D-1836 |
| B06 Call end time used where the rule needs the start | Confirmed | `app/picks/src/unreachable_refund.rs:80` (`at: call.ended_at`); `app/payments/src/refund_rules.rs:49` ("When the call started"); `app/calls/src/handlers.rs:133`, `migrations/0116_calls_call_log.sql:22` store no start; nothing serves the status callbacks that log calls | T270 (new, harness), T271 (new, the callback route) | TD-CALL-15, TD-CALL-16: 19:59 to 20:01 and 07:59 to 08:01 calls, the 72-hour edge, spacing by start, repeated and late callbacks, unknown start | D-1831 |
| B07 An in-flight refund is treated as refunded | Confirmed | `app/picks/src/unreachable_refund.rs:297` maps `Unchanged` with `Refunded`; `app/payments/src/status.rs` answers `Nothing` for a `sending` or `failed` row with no matching retry and `app/payments/src/refunds.rs:273` makes it `Unchanged`; `fraud_warning` (`:456`) has the same reading. T258's ownership does not include `unreachable_refund.rs` | T270 (new), after T258 | TD-FEE-47: a held first request, a concurrent second decision sends nothing and closes nothing; the first fails, the retry succeeds, exactly one eventual close and email | D-1831 |
| B08 A fee-free introduction's no-show leaves the pick `free` | Confirmed | `app/picks/src/unreachable_refund.rs:417` updates only `status = 'paid'` and `:426` drops the row count; `app/picks/src/aftermath.rs:281` counts `paid` and `free` as connected | T270 (new) | TD-FEE-47: call and email contact for paid, first-win-free, repeat-free and fee-free-job-type picks; explicit result on mismatch; fraud path stays paid-only | D-1831 |

## Known build gaps listed by the audit

| Gap | Verdict | Evidence | Owner | Acceptance | Decision |
|---|---|---|---|---|---|
| Earlier audit fixes still queued (T253 to T266, T236, T196) | Already owned | `kit/tasks.json` on `main`; all QUEUED on `claude/kit-state` | unchanged | their own rows | D-1751 to D-1761 |
| T092 lists `app/messaging/src/purge.rs`, absent on `main` | Gap confirmed | `python3 scripts/plan_health.py --all` on 2e7439c: "T092: DONE but its owned file app/messaging/src/purge.rs is not on main", holding T093, T253, T257 and T100; D-1801 explains why it was not written | T092's record reconciled (path removed); T276 owns and creates the path for real work (the day-31 delete) | `plan_health.py --all` reports no T092 gap after this change; PRIV-027 leaves T092's requirements (never built there, D-1803; `check_code_docs.py` traces it once every owned file exists) and is carried by T275 and T276 | D-1830, D-1836 |
| Sealed-object and provider-body deletion unowned (D-1803) | Gap confirmed | `StorageAdapter` has no delete or list (`app/adapters/src/storage.rs:18`); `SmsAdapter` no body delete; `tests/integration/tests/retention_purge.rs:14` marks the cases pending | T275 (adapters, stand-in), T276 (purge consumers) | TD-PRIV-19, TD-PRIV-20 | D-1836 |
| Suspension-mode composition unowned (D-1807) | Gap confirmed | `app/messaging/src/mode.rs` takes everything as inputs; no stored state, no `ops.status_banner` table, no inbound composition | T279 (new); T108 and T101 consume it | TD-TXT-33 | D-1838 |
| Check-in integration incomplete (D-1811) | Gap confirmed | templates missing, sender lists without them, no `schedule_check_ins` caller, no reply dispatcher | T278 ((1), (2), with D-1818's texts), T277 ((3), with B04) | TD-JOB-13, TD-JOB-12 | D-1837, D-1835 |
| Strict context check fails | Confirmed | `python3 scripts/gen_prompts.py --check --strict` on 2e7439c: 20 problems (17 at the audit's snapshot), every one a stored `context_pct` more than 3 points from its measurement | this change: full `gen_prompts.py` run re-measures every stored value; the check is not relaxed | `gen_prompts.py --check --strict` 0 problems on this branch | D-1830 |
| D-1777's link-only `SignIn::new` needs a fail-closed serving composition | Gap confirmed | `app/portals/src/business_auth.rs:322` (`SignIn::new`), `with_device_codes` optional; no task serves the sign-in pages (D-1728) | T280 (new) | TD-SEC-40 | D-1839 |

## Found while revalidating (not in the audit's list)

| Item | Verdict | Evidence | Owner | Decision |
|---|---|---|---|---|
| Nothing runs T191's unreachable steps; missed-call send time, TXT-030 email copy and the 72-hour emailed question unowned; register row IC-25 named T191, now DONE | Gap confirmed | D-1814 ("the caller (the worker, not wired yet)"), D-1818, D-1819 | T281 (new); IC-25 moves to T270 and T281 | D-1840 |
| No route serves the dialed leg's status callbacks, so no call is ever logged in production | Gap confirmed | `calls::api::log_finished_call` has no caller outside tests (`grep`), T262 serves only the voice webhook | T271 (new) | D-1831 |
| T103's migration number would sit below the new stage-4 migrations in the runner's serial order | Plan fix | `validate_tasks.py` V9 | T103 renumbered 0169 to 0172 (QUEUED, nothing written) | D-1830 |

## Not changed, and why

- **T085's call-log view** keeps the end time it has; a start-time display is a later choice once T270 and T271 are on `main` (D-1831). The refund decision itself uses the start.
- **T089** does not wait for T272: they share no file, and T089's goal now states that the link and try caps are T272's (D-1832).
- **T230** went IN_PROGRESS during this change; it is only a dependency of T278 and is not edited.
- **T191** (DONE, merged in #323) is not edited; T270 takes over its test file as a harness task.

## Still unowned after this change

- D-990, D-1198: the AI writing calls and wiring `ai::api` into intake (IC-23, area `ai`, unchanged).
