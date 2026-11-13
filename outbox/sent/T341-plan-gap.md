---
id: T341-plan-gap
title: T341: plan gap reported, blocked for a plan fix
kind: blocked
---
T341's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: test-design.md:203 (TD-FEE-63) and D-2332 require a text pick of a tiers quote to store nothing and reply with the quote-page link, but kit/prompts/T341.md:139 and :145 say T341 owns no customer copy. The reply needs a new en/es template with a link placeholder: existing text_pick_* templates have no {link}, and the first_quote, all_quotes_in and quote_update templates that do say something false here. Reviewer confirmed. Correction: no intake change is needed, because Out::say in app/picks/src/service.rs:183-205 renders the reply. TextPickIn carries no quote-page link, which is unsettled. Smallest fix: let T341 own the new template key (copy review WORD-028/029), or move this reply to T440 or a copy task and change TD-FEE-63 and D-2332 to match. Nothing was changed or committed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T341 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T341 --reason "..."` brings it back with the attempts it has left (D-2859).
