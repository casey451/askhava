---
id: T729-plan-gap
title: T729: plan gap reported, blocked for a plan fix
kind: blocked
---
T729's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/architecture_migrations.rs:288-294 (with :320-332) contradicts kit/prompts/T729.md:9 (D-4727 (1)). One migration 0215 must both create a privacy table and ALTER messaging.update_channel (add reverted_at); the protected migration test allows one module header per file and flags other-schema names, and CROSS_SCHEMA_CALLS covers only function calls. Fix: give T729 a second messaging-headed migration (0216_messaging_update_reverted.sql) or move reverted_at to T730. Related: the quote page needs a messaging reader for reverted_at (email.rs is read-only for T729). Reviewer confirmed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T729 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T729 --reason "..."` brings it back with the attempts it has left (D-2859).
