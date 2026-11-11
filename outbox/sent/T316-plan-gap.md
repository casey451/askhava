---
id: T316-plan-gap
title: T316: plan gap reported, blocked for a plan fix
kind: blocked
---
T316's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/businesses/src/holding.rs:8 and migrations/0050_licensing_imports.sql:22-55: nothing links a business to a stored record that holds a name. businesses.business and businesses.signup hold no name or license number; licensing.agency_record has no names (ROC import header is license_number,class,status,issued_on); licensing.city_license_entry has names but no business_id and city_standing does not store the match; licensing::api returns no name for a business ID. D-2206 assumed a link that D-2741 documents does not exist, and T316's no-storage rule forbids adding one. Reviewer confirmed. A plan change must decide where the link comes from (e.g. store the matched record or names at sign-up in T302/T165) or have callers pass names in.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T316 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T316 --reason "..."` brings it back with the attempts it has left (D-2859).
