---
id: T536-plan-gap
title: T536: plan gap reported, blocked for a plan fix
kind: blocked
---
T536's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/e2e/j2.ts:150 (owned by T099, `e2e-j2-business`; T644 also lists it). Smallest plan change: make the `notices` count in `closing` leave out the quiet-hours re-queue (rows whose `dedupe_key` starts with `messaging.business_notice.held.`), or count notices by `template`. Without that, the asserts at `tests/e2e/j2_business.spec.ts:57`–`:59` and `:78`–`:80` fail.
Evidence: with the worker now started on keyedEnv, the business-notice handler registers (app/server/src/lib.rs:1840-1868); a non-emergency notice handled during Arizona quiet hours re-queues a second `messaging.business_notice` task (app/messaging/src/business_notices.rs:501-506, dedupe key `messaging.business_notice.held.<task_id>`), and `closing` (tests/e2e/j2.ts:150) counts every task row of that kind, so it finds 2 per pick. Fresh DB: main 91 passed; branch 1 failed, 90 passed (`j2_business.spec.ts:42` got "notices": 2, expected 1). Reviewer a94ab22e50df34128 CONFIRMED the ownership (kit/tasks.json) and the cited lines; approved head 8f7225ce27f9100e6d8db068a19ad36762e6e118. Smoke, prehandin and prehandin --ally green on the branch.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T536 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T536 --reason "..."` brings it back with the attempts it has left (D-2859).
