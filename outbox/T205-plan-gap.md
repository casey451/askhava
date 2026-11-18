---
id: T205-plan-gap
title: T205: plan gap reported, blocked for a plan fix
kind: blocked
---
T205's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: `payments` has no public reader that ties a dispute to its pick (or its charge, business or job). Without one, T205 cannot narrow `waiting_disputes` or refuse a press on another pick's dispute, so this has to go back as a `payments` follow-up.
Evidence (confirmed by a second reviewer): list_disputes (app/payments/src/disputes.rs:362-388) and DisputeView (app/payments/src/models.rs:621-634) carry no pick, payment-intent, business or job; refund_state (app/payments/src/refund_state.rs:39-45) gives only `disputed`, not the dp_ id; EvidencePack fields are pub(crate) (models.rs:571-580); record_owner_submission (disputes.rs:231-240) takes only the dispute id, returns bool and never checks a caller-named pick. T205 owns only app/portals/src/owner_portal.rs, the two templates, tests/e2e/owner_refund.spec.ts and tests/integration/tests/owner_refund.rs (kit/tasks.json T205 entry); docs/architecture.md:267 (T175, SEC-029) forbids portals querying payments.dispute directly.
Effect at head 015d53a2b: the review step lists every waiting dispute system-wide (owner_portal.rs:677, :713) and a press on another pick's dispute records it under this pick's audit subject (owner_portal.rs:643), breaking SEC-006 / D-229 ("this dispute's evidence").
Fix needed: a payments task adds a pick-scoped dispute reader (e.g. disputes_of(conn, pick)) and has record_owner_submission take the pick and refuse a dispute not its own; then T205 narrows waiting_disputes, refuses a foreign dispute press with no audit row, adds the integration test, and corrects D-4481.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T205 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T205 --reason "..."` brings it back with the attempts it has left (D-2859).
