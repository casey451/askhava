---
id: T251-plan-gap
title: T251: plan gap reported, blocked for a plan fix
kind: blocked
---
T251's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/intake/src/summary_step.rs:415 and app/intake/src/service.rs:672 (T208/T039 files, not T251's). The card sender has nowhere to read the approved card text, category, remaining candidate job types or scrubbed text. Approval payload is only {job, conversation, card digest} (summary_step.rs:415); approved_card (summary_step.rs:345) needs the full JobCard, which only quotes::api::build_card makes. Emergency payload is only {conversation} (service.rs:672). No table stores card text/category/candidates/scrubbed text (quotes.job_card 0067, jobs.job 0031, intake tables 0053; jobs.job_facts 0164 is written by the sender itself, D-2330). intake_answer holds codes only (0053:68). matching::candidates is unwired (D-999, intake/CLAUDE.md:5). T162 depends on T251 in kit/tasks.json, so the in-memory holder comes later. Job ID on the emergency path is recoverable from intake rows (conversation.rs:143, models.rs:792). Reviewer confirmed. Fix options: (a) an earlier task stores card text/category/candidates/scrubbed text per job (privacy decision), or (b) move the two handlers to T162 and keep T251 to ZIP step, card pool and a sender function taking these as arguments. Also businesses::api has no list-businesses-serving-a-town function.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T251 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T251 --reason "..."` brings it back with the attempts it has left (D-2859).
