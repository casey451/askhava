---
id: T829-plan-gap
title: T829: plan gap reported, blocked for a plan fix
kind: blocked
---
T829's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/architecture_migrations.rs:791 (TD-SEC-10, not owned by T829) refuses the fix D-5411 prescribes for app/server/tests/door_matrix.rs:1180: plain sqlx::query is "SQL outside the query macros" (check_sql_forms :660/:677) and sqlx::query! naming `portals.` fails schema_refs/foreign() (:287/:264). Exemptions (:67, :76) cover migrations only. No portals::api helper clears owner_factor.totp_last_step (use_totp_step models.rs:470 is pub(crate), raise-only). Smoke failure: left ["app/server/tests/door_matrix.rs: SQL outside the query macros: `query`"]. Fix works otherwise (door_matrix 13 passed). Plan fix options: portals api helper owned by a portals task; or architecture-test exemption via harness task; or D-5411 changed. Reviewer confirmed: PLAN-GAP: CONFIRMED.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T829 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T829 --reason "..."` brings it back with the attempts it has left (D-2859).
