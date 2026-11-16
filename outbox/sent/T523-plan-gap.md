---
id: T523-plan-gap
title: T523: plan gap reported, blocked for a plan fix
kind: blocked
---
T523's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/ops/src/audit_subjects.rs:13-21 (owned by T477, not T523) — the audit-subjects reader returns only `subject_kind`, `subject_id` and `at` for an action, never the row's `field`, so the jobs reader cannot pick out `day3_no` / `day3_other` rows through it as the goal requires. Smallest plan change: let T523 own `app/ops/src/audit_subjects.rs` (or a harness/T477 follow-up) to add `field: Option<String>` to `AuditSubject` and to its `SELECT`; or have the plan let the jobs reader take the field through a new `ops::api` reader.
Evidence: AuditSubject has only subject_kind, subject_id, at (audit_subjects.rs:13-21); its query (:35-36) selects only those columns filtered by action and window. kit/tasks.json lists app/ops/src/audit_subjects.rs under T477; T523 owns only the four flag_inputs.rs files, tests/integration/tests/flag_inputs.rs and app/jobs/src/followups.rs. Every answer shares the one action jobs.check_in_answer (goal, D-3881(3) at docs/decisions.md:2206), so the reader cannot tell day 3 from day 14 or NO/OTHER from the rest. jobs.status_history moves are ambiguous (followups.rs:108-111, rules.rs:121-126); ops::api::audited (api.rs:229) returns only existence; ops::api::field_changed_last (api.rs:253) has no window and is defeated by later moves; export_audit pages the whole log; modules may not query ops.audit_log themselves (audit_subjects.rs:1-3).
Reviewer afaa2d05071c1f268 confirmed the ownership and contradiction. No commits made.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T523 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T523 --reason "..."` brings it back with the attempts it has left (D-2859).
