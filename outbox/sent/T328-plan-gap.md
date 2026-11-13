---
id: T328-plan-gap
title: T328: plan gap reported, blocked for a plan fix
kind: blocked
---
T328's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/intake/src/job_texts.rs is outside T328's scope (owned by T240, T247, T278, T402). The PICK-046 text cannot be sent from picks: picks may not depend on intake (kit/layers.json), and picks.pick stores no homeowner contact (migrations/0079_picks_code.sql:18-32). payments.pick_ended fires when no picks.release row exists, so release.contact_id is unavailable. The jobs.homeowner_text path (app/intake/src/job_texts.rs:56-74, 128-136, 358) has no PICK-046 template, no quote-list link, no EmergencyJob exemption, and is keyed once per job. Reviewer confirmed. Fix: a task that adds a pick_lapsed template to intake, or a job-to-contact lookup picks may use; then T328 queues it.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T328 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T328 --reason "..."` brings it back with the attempts it has left (D-2859).
