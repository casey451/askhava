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

## More for phase-2 planning and the runner-fix batch (Casey: "Add in what you think is best")

These come from Matt Pocock's open agent-skills material (aihero.dev). They are chosen only where they answer a problem this build actually has.

Nothing here applies now, during the freeze. Each item names where it belongs.

1. **A turn cap that ends with the retro note (runner fix; the cheapest win).**
   - **Problem:** T222's attempt 3 started with a small context but ran 230 turns. Every turn re-reads the growing context, so it cost about 48M tokens.
   - **Fix:** when a worker passes a turn limit, it stops and writes the retro note above, rather than running on.
   - **Setting the limit:** take it from the runner's own usage data, for example the 95th-percentile turn count of DONE tasks of the same type. Do not use a borrowed number.
2. **Review findings must cite a rule or spec line, and review does not loop until clean (template plus runner).**
   - **Problem:** T222 and T122 ran out of attempts while the reviewer kept finding new wording points after the fix round.
   - **Blocking findings:** a finding blocks hand-in only if it cites a requirement ID, a decision, a `BUILD_BRIEF.md` rule or a failing check.
   - **Everything else:** it becomes a follow-up line in `docs/for-casey-review.md`, not a fail.
   - **Fix round:** the reviewer checks only what it flagged, not a fresh full review.
   - This keeps every money, privacy, consent and security rule enforced, because those all have IDs.
3. **Phase-2 tasks as vertical slices, not crate layers (planning).**
   - **Problem:** this week's plan gaps (T305, T801, T820, T119) were mostly a crate-shaped task needing a template, test or reader that it did not own.
   - **Fix:** plan each phase-2 task around one behaviour you can demo, and draw its ownership from that path through every layer. Ask of each task: "what can I demo when this is done?"
4. **Acceptance must fail at the start (pre-flight).**
   - Each pre-flight, outside or Opus, also checks that the task's named tests or acceptance would fail on today's `main`, and can be written inside the task's owned files.
   - That catches tasks that are already green and tests that can't be placed.
5. **Size tasks by expected total spend, not only by starting context (planning plus pacer).**
   - Starting context is already capped (about 15–50% of the window).
   - The long journey tasks (T222, T119, T532) are the ones that run away.
   - Split any task the pacer estimates above a few million tokens: one journey, or one step group, per task.
   - Let a worker hand broad, many-file searches to a cheap read-only helper and keep only its conclusion. Use this for searches only, not single lookups.

**Not adopted:**
- installing his skills;
- his doc layout (GLOSSARY.md, docs/agents/);
- his parallel-worktree build flow.
The runner, the crate guides and `docs/spec/wording.md` already cover those, with stricter gates.
