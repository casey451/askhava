---
id: T515-plan-gap
title: T515: plan gap reported, blocked for a plan fix
kind: blocked
---
T515's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/adapters/src/tests/ssrf_guard.rs:488 is owned by T176 and asserts the stand-in PaymentIntent has no livemode (None). D-3862 says _event adds livemode False and nothing else, but release (app/payments/src/release_proof.rs:359 check_intent) refuses a re-fetched intent without livemode (WrongMode), so the acceptance (one paid release) cannot pass without giving the stand-in PaymentIntent livemode=False, which turns that test red. Smallest plan change: T515 also owns that test (None -> Some(false)), update the stale comment at app/adapters/src/payments.rs:54, and log both in docs/decisions.md. Reviewer confirmed the gap.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T515 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T515 --reason "..."` brings it back with the attempts it has left (D-2859).
