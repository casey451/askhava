---
id: T609-plan-gap
title: T609: plan gap reported, blocked for a plan fix
kind: blocked
---
T609's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/e2e/site_pages.spec.ts:26-28 (owned by T102) requires exactly one footer h2 "Homeowners" and exactly four footer links (toEqual), but T609's goal (DESIGN_SPEC.md:117, D-4252(a)) requires a second footer column (For businesses, Español, Terms, Privacy, Accessibility), and the prompt says that test must pass unchanged and T609 does not own it. Reviewer confirmed at HEAD 907c2ff0ac1a: current footer has one h2 and four links, so adding the second column fails lines 26-28. Smallest plan change: logged change to site_pages.spec.ts:26-28 to allow both columns, or drop the second column from T609.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T609 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T609 --reason "..."` brings it back with the attempts it has left (D-2859).
