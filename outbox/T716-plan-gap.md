---
id: T716-plan-gap
title: T716: plan gap reported, blocked for a plan fix
kind: blocked
---
T716's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/intake/src/conversation.rs (purge_child, ~348-362), app/intake/src/models.rs (purge_request). first_text (order_dispatch.rs:272) holds a short trade-naming first text's scrubbed words in quotes.job_description under the request's own ID (no job). If she then says "I'm 12" before the ZIP, stop_if_under_18 runs before order_dispatch, request_jobs is empty, purge_child purges nothing, purge_request deletes only code-only intake tables, so an under-13 sender's words survive INT-038/PRIV-008; the row also escapes the 3-year text purge (no job, no purge task). D-5400's "re-read the first inbox message" option is impossible: messaging.inbox stores no body (migrations/0043_messaging_inbox_delivery.sql:7, PRIV-002). Smallest plan change: purge_child also calls askhava_quotes::api::purge_job_text(tx, conversation), plus a test (two-word first text, then "I'm 12", leaves no job_description row) and a purge for an adult who never sends a ZIP; reword D-5581 and docs/for-casey-review.md. Confirmed by reviewer a1a2363fa41dcc7a8 at head 453a46fe0d47b68ba4e727ce1693faa9f1ef8d33. Separate accepted gap D-5582 (count/send disagreement for handyman+breaker) already logged.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T716 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T716 --reason "..."` brings it back with the attempts it has left (D-2859).
