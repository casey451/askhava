# askhava-jobs: the job lifecycle state machine

## Purpose

The layer-4 crate that owns every job's one status and the only code that changes it (`docs/architecture.md` section 6). A job moves only along the section 6 arrows; each move writes a history row and an audit row and queues the new status's timer in one transaction. T031 built the statuses, arrows, history and timers; T179 job numbers, T014 quote-page tokens; T032 the timeout texts, the every-window-ended entry point and the Connected reply; T230 adds feedback.

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the only public surface: `JobStatus`, `JobRow`, `JobsError`, `create`, `move_to`, `restart_timer`, `current`, `job_number`, `raise_number_floor`, `run_timer`, `TIMER_KIND`, `TEXT_KIND`, `text_due`, `all_windows_ended`, `connected_reply`, `api::quote_token` (`issue`, `revoke`, `resolve`, `TokenError`: `NoRandom`, `Busy`, `Database`), and `api::rules` (`check_move`, `timeout`, `Timeout`, `Wait`, `Refused`, `connected_move`, `Reply`, `CheckIn`, `Contact`, `after_windows`, `Windows`, `WindowsEnd`).
- `src/status.rs`: `JobStatus` (the twelve, `ALL`, `code`, `from_code`, `targets` = the arrows, `can_move_to`, `wait` = each status's timeout and target, `timeout_text` = the text its timeout sends, `awaits_pick`, `is_final`). Every `match` on a status lives here.
- `src/rules.rs`: pure rules, no I/O: `check_move`, `timeout` (backstop timer per waiting status, business or emergency clock) and the timer constants, `connected_move` (JOB-015) and `after_windows` (JOB-025).
- `src/handlers.rs` (T032, D-871, D-872): the `jobs.homeowner_text` event (`queue_text`, `TEXT_KIND`, `text_due`), `all_windows_ended` (called by `quotes`, T046) and `connected_reply`.
- `src/models.rs`: `JobsError`, `JobRow` and the queries (`insert_job`, `load`, `guarded_move`, `restart_wait`, `insert_history`).
- `src/service.rs`: `create` (numbers the job, `issue_number`), `job_number`, `raise_number_floor` (their queries sit here too, T179), `move_to`, the version-fenced `apply` (which queues a move's text event in its transaction), `restart_timer`, `queue_timer`, `run_timer`.
- `src/quote_token.rs` (T014, D-815): quote-page link tokens: `issue` (revokes the live token, inserts a new salt and hash, audits `quote_token_issued`), `revoke` (audits `quote_token_revoked`; stamped `GREATEST(at, issued_at)` so clock skew never blocks it, D-819), `resolve` (live token hash to job ID; `None` is the page's 404); a concurrent issue that loses the one-live-token race answers `TokenError::Busy`.
- Migration `0032_jobs_quote_tokens.sql`: `jobs.quote_token` (salt 16 bytes, `key_version`, unique `token_hash` 32 bytes, `issued_at`, `revoked_at`; partial unique index: one live token per job).
- Migration `0034_jobs_numbers.sql` (T179, D-851): `jobs.job_number` (number from 1001 as primary key, `job_id` unique, append-only for the app role) and the one-row counter `jobs.job_number_next`.
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
- JOB-007, JOB-013, JOB-014: `rules::timeout` counts business minutes 8 a.m. to 8 p.m. Arizona time (`core::api::add_business_minutes`) or 60 clock minutes on an emergency job (PICK-044).
- JOB-010 to JOB-012 (D-030, D-461): `JobStatus::timeout_text`; `run_timer` passes it to `apply`, which queues a `jobs.homeowner_text` event (`{job, version, template, values: {job: <number>}}`) in the move's transaction, at the timer's due time. Connected's close and the confirm and payment returns send none here (JOB-026).
- TXT-042, JOB-025 (D-140, D-872): `handlers::all_windows_ended(db, job, Windows {quoted, sent, choosable}, at)` acts only on Quotes ready: no choosable quote left closes it with `closed_no_quote_left` (fenced; reloaded and retried once if the row changed under it); otherwise it queues `all_quotes_in` with `{job, quoted, count}`. The dedupe key `jobs.homeowner_text.<job>.<template>` makes each text once per job. The sender (`intake`'s, not yet registered) fills the link, applies quiet hours and calls `text_due(job, version)` first, so a held text never goes after a pick or close.
- JOB-015 (D-143): `rules::connected_move` and `handlers::connected_reply` (DONE to Done, NO or PROBLEM to Closed, a day-3 NO under email contact to Quotes ready); parsing the reply is `intake`'s, the check-ins T097's, the 72-hour call check `calls`'.
- JOB-024: `service::issue_number` gives every job its number in `create`'s transaction; the number is the primary key of an append-only table, so it is never issued twice in one database; after a restore the replay (T110) calls `raise_number_floor` with the highest number in the vendors' sent-message records, and the counter never moves down (D-851).
- SEC-031: every `match` on `JobStatus` lives in `status.rs`, which denies `clippy::wildcard_enum_match_arm`; `scripts/check_rust_rules.py` fails anything else, and `tests/security/tests/status_enums.rs` proves the scan catches each planted shape (D-852).
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
- Each timeout lands on its business-clock or emergency-clock time: `src/tests/business_clock.rs`.
- A timeout sends its text with the move, the all-quotes-in text is queued once and is not due after a pick, and Connected closes silently: `tests/integration/tests/job_timers.rs`.
- A job number is never reused, a restore included: `tests/integration/tests/job_numbers.rs::td_job_06_a_texted_number_is_not_reused_after_a_restore`.
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
- `src/tests/business_clock.rs` (TD-JOB-03 pure cases: 7:30 p.m. pick to 11:30 a.m., emergency 60 minutes, 6 p.m. next day, 5, 3 and 21 days, which timeouts text, JOB-015 replies, JOB-025 rule): `cargo test -p askhava-jobs tests::business_clock`.
- `tests/integration/tests/job_timers.rs` (TD-JOB-03 database cases and TD-PICK-33's JOB-010 template: texts on timeout, the 3-day restart, all-quotes-in once, held to 8 a.m. and never after a pick or CLOSE, no-choosable close and its retry past a restarted wait, DONE and the silent close, a timer per waiting status): `cargo test -p askhava-integration --test job_timers`.
- `tests/integration/tests/job_numbers.rs` (TD-JOB-06: numbers from 1001 kept after cancel, concurrent creates, the app role cannot rewrite or re-issue one, a simulated restore and the raised floor; JOB-024).
- `tests/security/tests/status_enums.rs` (TD-SEC-12: the scan is clean, fails on each planted shape, every `status.rs` denies wildcard arms; SEC-031).
- `tests/integration/tests/webhook_dedupe.rs` (TD-SEC-16: re-issue, revoke, replay from the stored salt, one audit row each, a revoke stamped before the issue, the concurrent-issue race; SEC-037).
