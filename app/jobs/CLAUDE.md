# askhava-jobs: the job lifecycle state machine

## Purpose
The layer-4 crate that owns every job's one status and the only code that changes it (`docs/architecture.md` section 6). A job moves only along the section 6 arrows; each move writes a history row and an audit row and queues the new status's timer in one transaction. T031 built the statuses, arrows, history and timers; T179 job numbers, T014 quote-page tokens;

## Never break
- The arrows equal the hand-written oracle of section 6: `src/tests/prop_job_machine.rs::the_arrows_are_the_architectures`.
- A refused or stale move writes nothing: `src/tests/transitions.rs::a_refused_or_stale_move_changes_nothing`.
- Of two events racing one job, exactly one applies, in either order: `tests/integration/tests/job_races.rs`.
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
- Only ticked feedback is shown, a negative one too; a removal only for the four reasons, logged; never the owner's, a relative's or a business's: `app/portals/src/feedback.rs` `tests` (TD-JOB-10).
- `jobs.job_facts` is append-only and only ever narrows; two narrowings at once apply in turn: `tests/integration/tests/job_facts.rs` (TD-JOB-17).
- A purged feedback body is never restored or changed, the app role changes no other feedback column, a hold keeps the words until released, and "Delete my data" blanks them with the vault delete: `tests/integration/tests/feedback_purge.rs` (TD-PRIV-22).
- `JobsError` never carries a value or the database's message (PRIV-016).
- A dead or missing timer of a non-final job is revived, a final job's never, and the third death alerts and stays dead: `tests/integration/tests/timer_backstop.rs` (TD-OPS-38).
- A check-in or feedback text never goes after the job left Connected or for an earlier connection, a PROBLEM is listed once, and OTHER never records a no-show: `tests/integration/tests/followups.rs`.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/jobs/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/jobs/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
