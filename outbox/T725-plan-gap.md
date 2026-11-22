---
id: T725-plan-gap
title: T725: plan gap reported, blocked for a plan fix
kind: blocked
---
T725's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/docs/test_required_lines.py:139 (WORD-013, docs/spec/wording.md:17) contradicts kit/prompts/T725.md:9 (Spanish home page unchanged, shows neither). The consent line word for word in home.html contains "Reply STOP"; WORD-013 reads raw template text ignoring {% if %} and requires PARAR on es_home.html. Smoke fails: test_td_word_02_word_013_spanish_page_matches_its_english_page[es_home.html] assert ['PARAR'] == []. Task does not own es_home.html or the test. Reviewer confirmed. Smallest fix: harness plan change so WORD-013 exempts a STOP only inside the post-Gate C texting block, or let T725 place a reviewed Spanish line with PARAR on es_home.html.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T725 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T725 --reason "..."` brings it back with the attempts it has left (D-2859).
