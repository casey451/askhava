# askhava-jobs: the job lifecycle state machine

## Purpose

The layer-4 crate that owns every job's one status and the only code that changes it (`docs/architecture.md` section 6). A job moves only along the section 6 arrows; each move writes a history row and an audit row and queues the new status's timer in one transaction. T031 built the statuses, arrows, history and timers; later tasks add job numbers (T179), quote-page tokens (T014) and feedback (T230).

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the only public surface: `JobStatus`, `JobRow`, `JobsError`, `create`, `move_to`, `restart_timer`, `current`, `run_timer`, `TIMER_KIND`, `api::quote_token` (`issue`, `revoke`, `resolve`, `TokenError`: `NoRandom`, `Busy`, `Database`), and `api::rules` (`check_move`, `timeout`, `Timeout`, `Wait`, `Refused`).
- `src/status.rs`: `JobStatus` (the twelve, `ALL`, `code`, `from_code`, `targets` = the arrows, `can_move_to`, `wait` = each status's timeout and target, `is_final`). Every `match` on a status lives here.
- `src/rules.rs`: pure rules, no I/O: `check_move`, `timeout` (backstop timer per waiting status) and the timer constants.
- `src/models.rs`: `JobsError`, `JobRow` and the queries (`insert_job`, `load`, `guarded_move`, `restart_wait`, `insert_history`).
- `src/service.rs`: `create`, `move_to`, the version-fenced `apply`, `restart_timer`, `queue_timer`, `run_timer`.
- `src/quote_token.rs` (T014, D-815): quote-page link tokens: `issue` (revokes the live token, inserts a new salt and hash, audits `quote_token_issued`), `revoke` (audits `quote_token_revoked`; stamped `GREATEST(at, issued_at)` so clock skew never blocks it, D-819), `resolve` (live token hash to job ID; `None` is the page's 404); a concurrent issue that loses the one-live-token race answers `TokenError::Busy`.
- Migration `0032_jobs_quote_tokens.sql`: `jobs.quote_token` (salt 16 bytes, `key_version`, unique `token_hash` 32 bytes, `issued_at`, `revoked_at`; partial unique index: one live token per job).
- Migration `0031_jobs_state_machine.sql`: the `jobs` schema and grants, `jobs.job` (status `CHECK` of the twelve codes, `version`, `emergency`, `status_at`, `timer_from`, `timer_version`), `jobs.status_history` (append-only).
- `sqlx.toml`: `'TIMESTAMPTZ'` reads as `jiff_sqlx::Timestamp`.

## Rules it enforces

- JOB-001: `status::JobStatus` and the `jobs.job.status` `CHECK`.
- JOB-002: `status::JobStatus::targets` (the arrows; every non-final status may go to Cancelled, JOB-018); `rules::check_move`; `service::apply` refuses any other move with `JobsError::RefusedMove`, logged once with `tracing::error!` as a bug, before anything is read or written.
- JOB-003: `status::JobStatus::wait` and `rules::timeout`: every non-final status has a timer, queued on entry by `service::queue_timer` (D-805).
- JOB-004 (the pattern; its race tests are TD-JOB-02's): `models::guarded_move` updates only `WHERE status = from`, returning `Guarded::MovedOn` otherwise.
- JOB-005: `service::apply` writes `jobs.status_history` and `ops::api::audit` (`status_changed`) in the move's transaction; `create` audits `created`.
- JOB-006: a timer is a `core.task` (`jobs.timeout`, payload `{job, status, version, timer}`, dedupe key `jobs.timeout.<job>.<version>.<timer>`); `service::run_timer` re-checks the version and timer version first and does nothing if the job moved on or its wait was restarted; the move it makes is fenced on status, version and timer version and stamped at the timer's due time (D-806).
- D-807: a wait the spec counts from a later event or extends or pauses (INT-019, JOB-014, JOB-015, FEE-018, FEE-038, PICK-043, BIZ-034) is re-timed by the owning task with `restart_timer(db, id, (status, from))`; it never shortens a wait (`from` must not be before `timer_from`); a pause is a restart at `timer_from` plus the paused time.
- SEC-037, PICK-028: `quote_token` stores only the token's SHA256 and its salt; a re-issued or revoked token resolves to nothing. The salt's copy to the off-database stream waits on an `ops` stream event (D-817).

## Never break

- The arrows equal the hand-written oracle of section 6: `src/tests/prop_job_machine.rs::the_arrows_are_the_architectures`.
- A refused or stale move writes nothing: `src/tests/transitions.rs::a_refused_or_stale_move_changes_nothing`.
- One history row and one audit row per move: `every_move_writes_one_history_and_one_audit_row`.
- A timer for a job that moved on, or run twice, does nothing more: `a_timer_for_a_job_that_moved_on_does_nothing`.
- A restarted wait supersedes the pending timer: `a_restarted_wait_supersedes_the_pending_timer`.
- `jobs.status_history` is append-only for `askhava_app`: `the_app_role_cannot_rewrite_the_history`.
- Every waiting status times out along an arrow; finals never: `prop_job_machine.rs::every_waiting_status_times_out`.
- Nothing outside `status.rs` matches on or compares a `JobStatus` (`scripts/check_rust_rules.py`, SEC-031).
- A token is never stored, logged or audited; only its hash and salt: `tests/integration/tests/webhook_dedupe.rs::td_sec_16_reissue_and_revoke_and_replay`.
- `JobsError` never carries a value or the database's message (PRIV-016).

## How to change

- Add or change an arrow: architecture section 6 first (a locked section, so a plan change with a decision), then `JobStatus::targets`, the `ORACLE` in `src/tests/prop_job_machine.rs`, and run both test files.
- Change a timeout: the spec line and architecture section 6 table, then the constant in `src/rules.rs` (and `JobStatus::wait` for a new kind of wait); run `cargo test -p askhava-jobs`.
- Add a status: JOB-001 first; a `JobStatus` variant, its code in `code`, `ALL`, `targets`, `wait`, and a new migration widening the `CHECK`.
- Change a table: a new migration `migrations/0NNN_<slug>.sql` starting `-- jobs (T###)`, then `cargo sqlx prepare --workspace -- --all-targets --all-features`.
- Run `cargo test -p askhava-jobs`, `cargo clippy --workspace --all-targets -- -D warnings` and `bash scripts/smoke.sh`.

## Tests that guard it

- `src/tests/prop_job_machine.rs` (TD-JOB-01 property cases: arrows, one status, timeouts, and random runs through `move_to` on the database with one history and one audit row per change; JOB-001, JOB-002, JOB-003, JOB-005, JOB-006).
- `src/tests/transitions.rs` (TD-JOB-01 database cases: history and audit rows, refused (logged once) and stale moves, timers surviving a restart and doing nothing after a move, restarted waits, the append-only history; JOB-001 to JOB-003, JOB-005, JOB-006). The per-status timer values and texts are TD-JOB-03's.
- `tests/integration/tests/webhook_dedupe.rs` (TD-SEC-16: re-issue, revoke, replay from the stored salt, one audit row each, a revoke stamped before the issue, the concurrent-issue race; SEC-037).
