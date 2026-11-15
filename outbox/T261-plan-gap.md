---
id: T261-plan-gap
title: T261: plan gap reported, blocked for a plan fix
kind: blocked
---
T261's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/e2e/confirm_served.spec.ts can't be built in T261. The fix belongs to tests/e2e/webserver.ts, which T227, T385, T452 and T469 own (not T261). Its appEnv (:89) and keyedEnv (:107) set no ASKHAVA_STRIPE_WEBHOOK_SECRET, Stripe account, Stripe stand-in URL, publishable key or launch date, so release_sink::stripe_webhook (app/server/src/release_sink.rs:298-300) returns None, confirm_route returns None, and neither e2e binary serves /b/picks/{pick}/confirm. Also the 800-line budget does not hold: +1936 lines across 18 files (owned files about 1680). Reviewer confirmed the ownership contradiction and budget overrun. Needed: a harness task adding the Stripe stand-in settings, publishable key and launch date to the keyed e2e binary plus a PAGE_STATES entry, with T261's served spec after it. Other reviewer findings (paused-pick test, FEE-041 over-limit and lost-answer tests, "Nothing was charged" race in press paused branch) are fixable inside T261 once re-queued. Work committed at 705f96a54 on claude/task/T261-served-confirm-route (not pushed).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T261 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T261 --reason "..."` brings it back with the attempts it has left (D-2859).
