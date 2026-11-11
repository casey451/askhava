---
id: T337-plan-gap
title: T337: plan gap reported, blocked for a plan fix
kind: blocked
---
T337's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/server/tests/release_recovery.rs:572-576 (td_pick_50_a_zero_fee_releases_free_and_a_cancelled_pick_nothing) is owned by T254/T287/T343/T352, not T337 (kit/tasks.json; kit/prompts/T337.md and D-2832 cover only confirm_and_pay.rs). Quote order is by business_id (random uuid), so gone's pick_fee row names min(first, other); the test passes (first, replaced_job) and with TD-FEE-57's binding no_fee_release refuses about half the runs (panic at release_recovery.rs:344). Fix: let T337 own release_recovery.rs for a fixture-only change (use the pick's stored business via payments::api::pick_fee). Reviewer confirmed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T337 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T337 --reason "..."` brings it back with the attempts it has left (D-2859).
