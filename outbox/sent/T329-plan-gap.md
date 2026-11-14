---
id: T329-plan-gap
title: T329: plan gap reported, blocked for a plan fix
kind: blocked
---
T329's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: kit/prompts/T329.md:166 and :172 (task owns no customer copy; stop if writing words a business reads) contradict kit/prompts/T329.md:9 and D-2287 (docs/decisions.md:1487), which say each kind is sent with words businesses already has. Four kinds lack words: notice_address_changed (email_verification.rs:37, no words, SEC-024), after_hours_off (emergency_switch.rs:30, no email words, BIZ-007), notice_address_returned (email_verification.rs:39, returned_line :549 English only, no subject/Spanish, BIZ-040), phone_linked (complaints.rs:44, phone_linked_line :556 has no subject and no Approve wording, BIZ-033). Reviewer confirmed: no other source supplies them; T268 precedent (D-3185, kit/prompts/T268.md:144,:150) gave copy ownership. Fix: give T329 copy ownership with copy checks, or add a copy task first. Worker committed nothing.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T329 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T329 --reason "..."` brings it back with the attempts it has left (D-2859).
