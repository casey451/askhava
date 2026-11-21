---
id: T776-plan-gap
title: T776: plan gap reported, blocked for a plan fix
kind: blocked
---
T776's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/intake/src/matching.rs
Worker: matching.rs:52 keyword RULES cover only plumbing, HVAC, electrical, handyman, pool, so no text reaches `tell` as local_moving, home_watch or turnover_cleaning; the T776 refusals can't be reached or tested end to end. matching.rs is owned by T042/T183/T716, not T776. Reviewer (Opus) confirmed: integration test intake_refusals_wired 0 passed, 3 failed (no trade read, nothing sent). Smallest fix: add keyword rules for those three trades (from docs/catalog/categories.yaml) in matching.rs, by a prior task or by adding matching.rs to T776 ownership. Second gap: order::store_given keeps trade answers as code only, so LIC-018 "answers" can't be rechecked on later answers without more order_dispatch.rs change. Also routing::tell_coverage->not_covered always writes a recruiting-demand row, against D-1008 for second-job refusals.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T776 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T776 --reason "..."` brings it back with the attempts it has left (D-2859).
