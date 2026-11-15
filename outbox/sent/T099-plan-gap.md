---
id: T099-plan-gap
title: T099: plan gap reported, blocked for a plan fix
kind: blocked
---
T099's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/e2e/webserver.ts:132 (T492/T227 file, forbidden to T099 by prompt line 126) gives the keyed binary a random Stripe webhook secret never exposed to specs; the stand-in signs with a different secret (stand_ins/stripe.py:82), so no spec can post a valid signed webhook and the post-payment closing checks (portal view, reload, one release row, no second fee, D-1739) cannot run. Reviewer confirmed the gap. Smallest fix: a harness task passing the keyed webhook secret to specs as E2E_KEYED_* . Reviewer round 2 also noted: free-win $0 Confirm release checks possible without webhook (test 3), and top-of-file comment in j2_business.spec.ts lines 10-11 wrongly cites FEE-010 for the $75 test.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T099 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T099 --reason "..."` brings it back with the attempts it has left (D-2859).
