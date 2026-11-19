---
id: T715-plan-gap
title: T715: plan gap reported, blocked for a plan fix
kind: blocked
---
T715's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: one TD-INT-52 sentence can't be met by intake as planned: "a two-word first text naming a trade opens its job at the ZIP's step 5 and gets the coverage line". D-4441 gives the job-type question before the count to T716, and without that question the coverage line never comes. Today a homeowner who texts "sticky door" gets INT-017's "not covered" reply and the request is closed, even though two covering handyman businesses exist. Amend TD-INT-52 or move this case to T716; the test's assertion then changes to the coverage line.
Evidence: `trade_check` passes no words when the first text opened no job, because the words are never kept (INT-007, INT-049), so `facts` reads `<trade>_other` (app/intake/src/matching.rs:400-402). docs/catalog/categories.yaml lists no `*_other` job type, so quotes::card_pool::candidate_jobs marks it unlisted (app/quotes/src/card_pool.rs:150). licensing::rules::no_license refuses an unlisted job type before it reads any ticked job type (app/licensing/src/rules.rs:81-82; handyman the same at :99). A job type at step 5 can come only from the "Which fits best?" question or stored candidate-code rows, both T716's (its prompt parts (1) and (2); D-4441 (3)). TD-INT-52's sentence also contradicts TD-INT-53, where "Which fits best?" comes before the coverage line.
Reviewer (agent a66dbff8ebfea7810) confirmed the plan gap independently and approved head 6230f48ff986a7cc97f7999936d2914d3957db27 as otherwise correct: all other Goal parts (1)-(6) built and tested (intake_order_corrections 5 passed, intake_order_area_trade 9, intake_order_two_jobs 6, name_address_answer 5; smoke and prehandin green per the workers). The branch's test now asserts the current not-covered outcome exactly, gap logged in D-4530; the branch can be reused on the requeue onc

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T715 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T715 --reason "..."` brings it back with the attempts it has left (D-2859).
