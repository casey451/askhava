---
id: T461-plan-gap
title: T461: plan gap reported, blocked for a plan fix
kind: blocked
---
T461's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: T461's acceptance says a flagged (NeedsReview) sign-up's done screen says it waits for review, but the wording lives in app/portals/src/business_signup.rs (DoneFacts / done_lines, lines 234-243, owned by T450's line), which T461 does not own (it owns only app/server/src/business_signup.rs and app/server/tests/business_signup_route.rs). Evidence: app/server/src/business_signup.rs:1149 Form::finished gives a NeedsReview sign-up the same Screen::Done with done_facts(..., (false,false)), showing "We'll tell you once, by email, when requests in your trade start." Worker logged it in D-3806 and docs/for-casey-review.md; reviewer (head 181df2bf12944491b3ad8df7392291b88cbefd71) confirmed the ownership contradiction. Everything else in T461 was built and committed; smoke and prehandin green. Fix needs a portals copy task adding a review field/line to Screen::Done, or a narrower T461 acceptance.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T461 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T461 --reason "..."` brings it back with the attempts it has left (D-2859).
