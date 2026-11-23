---
id: T730-plan-gap
title: T730: plan gap reported, blocked for a plan fix
kind: blocked
---
T730's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/messaging/src/quote_text_sent.rs
Evidence: last_quote_text (quote_text_sent.rs:45-52) reads only messaging.outbox; with the Email choice T730 stores no text row (kit/prompts/T730.md:9), so intake previous() (quote_updates.rs:166-172) is None, notice() returns Notice::First (quotes/src/notify.rs:100-105) and later quote updates end as Nothing. T730 owns only update_emails.rs, email.rs, service.rs and update_email_sends.rs (T730.md:35-40); the file is owned by merged T582 only. Fix: add the file to T730 owns so last_quote_text also reads update emails in messaging.email_outbox, plus a test driving intake's update handler for Email-only and for a bounce-reset homeowner. Reviewer confirmed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T730 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T730 --reason "..."` brings it back with the attempts it has left (D-2859).
