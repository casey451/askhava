---
id: T754-plan-gap
title: T754: plan gap reported, blocked for a plan fix
kind: blocked
---
T754's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/adapters/src/payments.rs, stand_ins/stripe.py, stand_ins/server.py
T754 requires cancelling the PaymentIntent at Stripe after a $0 press, but PaymentsAdapter/IntentStripe (app/adapters/src/payments.rs:343-397, app/payments/src/charge.rs:67) have no cancel method and stand_ins/stripe.py and stand_ins/server.py (route at server.py:457) have no cancel route. None is owned by T754 (owns confirm_pay.rs, release_proof.rs, routes.rs, zero_fee_after_intent.rs); stand_ins/ is protected. Existing callers of end_intent_unless_charging (confirm_moves.rs:137, cant_take.rs, confirm_pay.rs:1033) only write ended_at in the database, so no retry precedent exists. Reviewer confirmed. Smallest fix: drop the Stripe cancel from T754 (DB end only), or add a harness task first adding cancel_payment_intent and the stand-in route.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T754 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T754 --reason "..."` brings it back with the attempts it has left (D-2859).
