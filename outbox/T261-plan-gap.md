---
id: T261-plan-gap
title: T261: plan gap reported, blocked for a plan fix
kind: blocked
---
T261's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/payments/src/free_wins.rs:101-112,142 waive_at_confirm needs JoinDates (launch, trades_opened, joined). No reader for businesses.signup.submitted_at (written app/businesses/src/models.rs:296, never read); catalog.category has no switch-on timestamp (migrations/0027_catalog_module.sql:15-26); no task in kit/tasks.json (T475-T484 included) builds them; T261 owns no businesses or catalog file. Reviewer confirmed. same_customer_jobs (intake::api::job_homeowner + jobs_of_contact) and the launch-date/Stripe-key settings (shared core settings.rs) are sourceable by T261. Needed: a task adding a businesses join-date reader and a catalog switch-on time (migration plus reader), with T261 depending on it.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T261 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T261 --reason "..."` brings it back with the attempts it has left (D-2859).
