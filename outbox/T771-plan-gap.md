---
id: T771-plan-gap
title: T771: plan gap reported, blocked for a plan fix
kind: blocked
---
T771's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: kit/prompts/T771.md:9 (D-4970, docs/decisions.md:2604) contradicts itself: rule (1) checks the hidden Menu summary as before and rule (2) refuses links whose hidden container holds a non-a[href] control, so the DESIGN_SPEC.md:102 header at 1280 px (site.css:783 .menu display:none hides the whole details) still reports the Menu summary and its four links. Reviewer confirmed. Smallest fix: let rule (2) also cover the hidden Menu's own summary (boxless summary of a details inside or being the hidden container, when every other TARGETS control there is an a[href] with a passing same-href counterpart outside).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T771 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T771 --reason "..."` brings it back with the attempts it has left (D-2859).
