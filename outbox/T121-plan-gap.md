---
id: T121-plan-gap
title: T121: plan gap reported, blocked for a plan fix
kind: blocked
dispatched: 2026-11-24
---
T121's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/portals/src/update_channel.rs
T729 (owner) merged UpdatesForm/ConfirmForm without deny_unknown_fields and confirm_query ignores unknown keys; SEC-010 scan fails on them (F1 x2, F5 x1). Also: REFUSING_TRY_FROM lacks JobTypesForm (business_portal.rs), StepUpForm (business_step_up.rs), Posted<F> (address_changes.rs, business_guarded_changes.rs); CLASSIFIED lacks twilio_answer/vendor_error (adapters base.rs), run_help_notice, spanish_switch latest, photo_asks load; business_signup_route.rs posts roc_number/base_zip/radius/code names the templates lack, so per-route name lists cannot be strictly template-closed; 550-line budget too small (~1000). Reviewer confirmed all three claims on main bd3e4e9cd.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T121 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T121 --reason "..."` brings it back with the attempts it has left (D-2859).
