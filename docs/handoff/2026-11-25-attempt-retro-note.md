# Note for the after-phase-1 runner-fix batch: a short "retro" on every failed attempt

**Date:** Nov 25, 2026. **Casey's go:** "Yes", on the overseer's suggestion to note this for the runner-fix batch (PF-R2, `docs/handoff/PF-R2_runner_fix_design.md`).

## Why

- **T222's last attempt left no record of why it failed.** It spent about 48M tokens over 2.4 hours and ended BLOCKED.
  - `last_failure` still held attempt 2's text: that is the known `record_failure` BLOCKED-branch gap.
  - The job-log API does not return the worker step's output.
  - The work in progress was not pushed, because it changed `.github`.
- The overseer could not tell whether the worker hit attempt 2's test-clock gap, ran out of turns, or failed on something new.

## Idea

The idea comes from the `/retro` pattern in the open-source agent skills at aihero.dev. Only the idea is taken: no files, nothing installed.

- **What the worker writes:** at the end of every attempt that does not hand in green, a few lines saying:
  - what stopped it;
  - which file or decision would have saved the attempt;
  - whether it is a plan gap, a flake, a budget overrun or a code defect.
- **What the runner does with it:**
  - stores the note, capped, as the attempt's history row `failure_reason` (PF-R2 problem 2's optional field);
  - shows it under BLOCKED in `runner/status.py`.
- **Fallback:** when the worker ends without a report (turn or time limit), the runner records "no report: <session-end reason>", so the row is never empty.

## Scope

- This folds into PF-R2's second harness task, the per-attempt failure reason, as one more source for that field.
- It is not a new task under the freeze.
- Check it against the template's existing `PLAN GAP:`, `ALLY RED:` and `MAIN RED:` report lines, so the note never duplicates or contradicts them.
