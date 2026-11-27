---
id: T856-plan-gap
title: T856: plan gap reported, blocked for a plan fix
kind: blocked
---
T856's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/intake/src/replies.rs:367 (and replies.rs:204). The chat raw path web_chat::raw_turn -> replies::receive_raw_reply -> answer_name_address -> receive_answer (replies.rs:367) and reply_in's receive_text call (replies.rs:204) reach models::record_consent; T856 does not own replies.rs, TextIn has ~100 unowned builders, so only a runtime flag (set_chat_turn, D-5891) is possible in owned files. Reviewer (agent aec58822cf2c18f01) confirmed the gap with evidence at head c3beeeb4ca4cae1ff578a5f1aa7ea50d5c8e35e3; its only finding is that D-5891 should also cite replies.rs:204.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T856 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T856 --reason "..."` brings it back with the attempts it has left (D-2859).
