---
id: T754-plan-gap
title: T754: plan gap reported, blocked for a plan fix
kind: blocked
---
T754's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/stripe_webhook_dispatch.rs:420-430 (done test td_fee_43_a_database_error_is_resent_and_handled, owned by T785 after T259/T331/T352) expects a verified payment_intent.succeeded naming pi_FakeNone for a pick stored with pi_Fake9 (refused WrongIntent, release_proof.rs:220-221) to change nothing, including ops.alert rows; kit/prompts/T754.md:9 (m3) requires one High alert for every WrongIntent refusal. T754 does not own that test file (T754.md:37-40, :53, :104). Reviewer confirmed (GAP-VERDICT: CONFIRMED). Smallest fix: (1) give T754 a one-time edit of stripe_webhook_dispatch.rs:430 to expect one alert payments.success_refused_wrong_intent, logged as a spec change; or (2) narrow m3 so WrongIntent alerts only when the event's intent is the pick's stored intent (store_success / missing pick_intent / refunded_or_disputed paths), no test edit.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T754 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T754 --reason "..."` brings it back with the attempts it has left (D-2859).
