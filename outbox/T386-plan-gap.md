---
id: T386-plan-gap
title: T386: plan gap reported, blocked for a plan fix
kind: blocked
---
T386's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/voice_calls.rs:229-251 (td_call_06_no_route_lets_a_business_text_a_homeowner) keeps a fixed placeholder allowlist (slots, lines 231-241) for en.json/es.json that lacks {cant_take}, which T386 must add to pick_notice and pick_notice_emergency. The file is owned by T080/T271/T378/T383/T425, not T386. Evidence: cargo test -p askhava-integration --test voice_calls td_call_06 exit=101, panicked at voice_calls.rs:250:13: {cant_take} in en. Reviewer confirmed (kit/tasks.json ownership, prompt line 9 wording). Smallest plan change: let T386 own voice_calls.rs to add "{cant_take}" to slots, or have an owner task add it first.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T386 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T386 --reason "..."` brings it back with the attempts it has left (D-2859).
