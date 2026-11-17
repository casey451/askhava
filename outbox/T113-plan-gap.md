---
id: T113-plan-gap
title: T113: plan gap reported, blocked for a plan fix
kind: blocked
---
T113's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/docs/test_terms_clauses.py:494 is T111's test (DONE, kit/tasks.json:18231) and asserts `report.checked == len(check_legal_docs.DOCUMENTS) == 10`. T113 must list `twilio-campaign.md` in `DOCUMENTS` (prompt Goal, line 9), which makes the count 11, and T113 does not own that file. Smallest plan change: let T113 own that one line and change `== 10` to `== 11` (or drop the literal). Evidence: bash scripts/smoke.sh red with exactly one failure: tests/docs/test_terms_clauses.py::test_spanish_summary_is_listed_as_a_business_document - AssertionError 11 == 10 (1 failed, 3154 passed, 11 skipped). Reviewer a3dde0d36e597189a confirmed the ownership and contradiction independently (PLAN-GAP: CONFIRMED). Worker's draft (twilio-campaign.md, test_campaign.py, DOCUMENTS entry, count 10->11, plan.lock, D-4147) is uncommitted on the local branch.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T113 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T113 --reason "..."` brings it back with the attempts it has left (D-2859).
