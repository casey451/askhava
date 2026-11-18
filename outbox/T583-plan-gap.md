---
id: T583-plan-gap
title: T583: plan gap reported, blocked for a plan fix
kind: blocked
---
T583's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: kit/prompts/T583.md:51 (budget of 450 changed lines in 3 files) The smallest plan change: raise T583's budget to 650 changed lines, or move `tests/integration/tests/quote_update_texts.rs` into a test-only follow-up task with its own budget.
Evidence: `git diff --numstat origin/main...HEAD` on owned files at head a3034e2b1: app/intake/src/job_texts.rs 5/5, app/intake/src/quote_updates.rs 292/0, tests/integration/tests/quote_update_texts.rs 377/0 (679 lines). Fix-round worker's honest trim reached about 273 + 8 + ~350 = ~630 lines (test still failing clippy: a 73-line function needing a split), without dropping required cases, the worker-registry path or required docs; tests/integration/src/lib.rs (no shared helpers) is not owned by this task. The confirming reviewer measured a realistic minimum of about 590-615 lines (~30% over 450, and above the 600-line build ceiling MAX_LINES, scripts/validate_tasks.py:199; cf. D-4202's size split). Code at head a3034e2b1 had no correctness finding; all six required cases present; quote_update_texts 19 passed, smoke and prehandin green per the first worker.
Also noted by both reviewers as a plan item (not drift): a second quote arriving while the first-quote text is still held overnight is dropped (notice_for answers First), so no update tells "2" unless a third quote arrives; suggest holding the event to the first-quote text's send time.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T583 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T583 --reason "..."` brings it back with the attempts it has left (D-2859).
