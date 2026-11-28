---
id: T849-plan-gap
title: T849: plan gap reported, blocked for a plan fix
kind: blocked
---
T849's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/e2e/j1_homeowner.spec.ts:302 (not owned by T849; the per-IP limit is in app/portals/src/pick_flow.rs:846-858, also not owned) has J1 step 6 failing in the late project. D-5955 moved j1_homeowner.spec.ts into late, so it runs after every main spec in the same real hour; the pick page counts per-IP code sends at real_now() (D-5836), CODE_SENDS_PER_IP allows 10 an hour (app/portals/src/limits.rs:34), all e2e requests come from 127.0.0.1, so the budget is gone by step 6 and the send is refused (pick_flow.rs:825); #pick-result never appears and j1_homeowner.spec.ts:321 toBeFocused fails. Passes alone (11/11); fails in full runs (2 local, 1 Ci). Smallest plan change: T849 owns j1_homeowner.spec.ts and clears this IP's portals.code_sends_per_ip and code_tries_per_ip buckets in core.rate_limit before step 6, or per-spec client IPs. Reviewer confirmed. D-5978 fix (keyed.ts, j3.ts, D-5905) is committed at 5c0500c3d and approved.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T849 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T849 --reason "..."` brings it back with the attempts it has left (D-2859).
