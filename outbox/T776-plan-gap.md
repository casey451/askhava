---
id: T776-plan-gap
title: T776: plan gap reported, blocked for a plan fix
kind: blocked
---
T776's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/quotes/src/card_pool.rs (also docs/catalog/categories.yaml)
Worker: D-5160's three trade-only rules (job_type: None) give candidate <trade>_other; card_pool.rs:150-163 marks it listed only if in the category's stored job types; categories.yaml lists no <trade>_other for local_moving, home_watch, turnover_cleaning (all license none); licensing/rules.rs:80-82 no_license returns RefusedJobType, so in-Arizona moving/turnover texts get not_covered and no card, contradicting T776's tests (Need movers to Phoenix, California king, Turnover cleaning after renters check out, EDIT cases). intake_refusals_wired: 1 passed, 3 failed.
Reviewer (Opus) confirmed. Smallest fix: amend D-5160 so the three rules name a listed job type (in-town move, weekly house check, turnover clean) in matching.rs, or add <trade>_other to the catalog for these trades.
Other points: "same text twice adds nothing" is not assertable as written (a repeat after close opens a new request); only first text and EDIT words are kept; routing::tell_coverage->not_covered always writes a recruiting-demand row (D-1008).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T776 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T776 --reason "..."` brings it back with the attempts it has left (D-2859).
