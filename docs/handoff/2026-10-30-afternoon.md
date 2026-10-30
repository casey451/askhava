# Session hand-off, October 30, 2026 (afternoon)

For the next Claude session that looks after the build. Read CLAUDE.md, then docs/handoff/2026-10-29-evening.md, docs/handoff/2026-10-30-day.md and this note. This one wins where they differ.

## Where the build is (19:40Z)
- 33 of 228 tasks are done: the 25 in the morning note plus T176, T017, T020, T021, T177, T022, T026, T027. Stage 2 is running on two lanes; T223 is MERGE_PENDING.
- Count only `T###` ids when telling Casey the number; `state.json` also lists the kit tasks (K0 to K9, FIX1, GATE-P) as DONE.

## Fixed this afternoon
- **D-794 (PR #137):** since D-723 (#121) the backup-runner step "Start the next cycle at once when this one cleared the way (D-500)" failed on every job with a bash syntax error: an apostrophe in a comment inside its single-quoted `python3 -c '...'` snippet ended the quote. The refill loop never ran, so a lane freed by another lane's merge waited for an event or the hourly tick, and every backup job showed failure. The comment is reworded and `tests/test_workflow_shell.py` now runs `bash -n` over every bash `run:` step in `.github/workflows`. If #137 has not merged yet, see it through (auto-merge is on); afterwards check that the next backup job's "Start the next cycle" step is green.
- Never put an apostrophe or other single quote inside a single-quoted snippet in a workflow; the new test catches it.
- Run 41108679328 (18:38Z) died during "Smoke-test prerequisites" with no error line; it looks like a one-off GitHub problem. Watch for a repeat.

## Taking over from the old session
- The old session's next 29-minute update is scheduled task `trig_01XBuPKniixCVWS7XrKw6JF6` (fires 20:05Z). Delete it with `delete_trigger`, then schedule your own update chain with `send_later` (one chain only). Each update: check state, the runner log tail, open task PRs and backup-runner runs; dispatch an idle lane if needed; send Casey 2 or 3 plain lines with the done count.
- Merging main into a branch that adds a decision: the merge can place the new entry ahead of main's new entries. Move it to the end and run `python3 scripts/check_pr_change.py --base origin/main` before pushing.
- In this container the git fetch refspec may cover only one branch; after `git push -u`, add a fetch refspec for the new branch if `git status -sb` shows no upstream.
