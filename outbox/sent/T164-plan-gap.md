---
id: T164-plan-gap
title: T164: plan gap reported, blocked for a plan fix
kind: blocked
---
T164's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/src/tests/prop_arrival_order.rs:22 (T052) and app/quotes/src/notify.rs:179 / app/quotes/src/models.rs:573 (T047). The homeowner side of T164 (quote page "Updated <time>"/"Withdrawn by the business", LIST reply) needs api.rs (page_quotes filters withdrawn_at, api.rs:116), PageQuote fields (api.rs:93, built field-by-field in the T052 test), notify.rs list_line/Listed and models.rs list_rows, none owned by T164. Reviewer confirmed (kit/tasks.json owns vs. those files; T361 covers part but depends on T164).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T164 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T164 --reason "..."` brings it back with the attempts it has left (D-2859).
