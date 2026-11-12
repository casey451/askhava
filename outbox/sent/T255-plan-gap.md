---
id: T255-plan-gap
title: T255: plan gap reported, blocked for a plan fix
kind: blocked
---
T255's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: Releasable in app/payments/src/release_proof.rs:153 has private fields and no constructor; T255's owned reclear.rs cannot build one (lib.rs declares mod release_proof as a sibling; CLAUDE.md of payments forbids a public constructor). release_proof.rs is owned by T189, T337, T408, T332, T359, not T255. paid_release needs a VerifiedStripeEvent and an adapter, so it cannot serve a (conn, pick) re-clear without a new event (SEC-030). Smallest fix: give T255 release_proof.rs to add a pub(crate) constructor from the stored success, or declare reclear as a child module inside release_proof.rs. Reviewer confirmed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T255 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T255 --reason "..."` brings it back with the attempts it has left (D-2859).
