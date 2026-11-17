---
id: T203-plan-gap
title: T203: plan gap reported, blocked for a plan fix
kind: blocked
---
T203's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: `app/portals/templates/quote_page.html:101` (owned by DONE tasks T052, T095, T294, T308, T384, T388, T393, T440 and T441, and by QUEUED T608). WORD-002 (`docs/spec/wording.md:6`) and TD-WORD-05 (`docs/test-design.md:683`) say the long not-a-contractor statement must be "at the top of the quote page". The template has only the first sentence, and only at the bottom (line 101 of 103); it extends `app/site/templates/base.html`, which has no contractor line. T203 owns only tests/docs/test_wording_statements.py and tests/docs/test_introduction_fee_wording.py and may not write copy (kit/prompts/T203.md:94), so a correct TD-WORD-05 test fails on main. Smallest plan change: let T203 own that one line of quote_page.html, or add a task (or extend T608) that puts the full long statement (brand from brand()) at the top of the page.
Second gap (reviewer confirmed): WORD-007 (`docs/spec/wording.md:11`, via TD-WORD-02 in T203's prompt line 66) wants "Your number is never shared" with "Your first name and address go only to the business you pick." `app/site/templates/home.html:36` has only the second sentence ("Your number stays with us."); no app .html has "never shared". home.html owners: T102 (DONE), T593, T216, T608 (QUEUED). Either add the sentence via an owning task or state that WORD-007 covers only the privacy policy.
Reviewer (fresh-eyes, Opus) confirmed both gaps independently. No commit made; branch unchanged at b1fe64c6f.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T203 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T203 --reason "..."` brings it back with the attempts it has left (D-2859).
