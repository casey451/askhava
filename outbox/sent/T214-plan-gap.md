---
id: T214-plan-gap
title: T214: plan gap reported, blocked for a plan fix
kind: blocked
---
T214's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/picks/src/handlers.rs:73-76 (T063's) and the pick_notice / pick_notice_emergency keys in app/messaging/templates/en.json:322 and es.json build the pick notice's words and values; T214 owns neither, and confirm.rs only exports template ID constants (confirm.rs:41-44). business_notices.rs:63 hardcodes PICK_TEMPLATES so a renamed template would break the Accept-by window. Smallest plan change: give T214 handlers.rs and let it add a {cant_take} value to the pick_notice and pick_notice_emergency templates in both languages, or move the notice-link part of PICK-050 / TD-PICK-43 to a task that owns handlers.rs. Reviewer confirmed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T214 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T214 --reason "..."` brings it back with the attempts it has left (D-2859).
