---
id: T161-plan-gap
title: T161: plan gap reported, blocked for a plan fix
kind: blocked
---
T161's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: `tests/docs/test_site_copy_blocks.py` must check that the home page and footer show the blocks in `docs/copy/site-copy.md:7` to `:79` (D-3890 (3)). On `main` the home page does not match that file, and I can't fix either side. `app/site/templates/home.html` belongs to T102, T203, T216, T593 and T608. `docs/copy/site-copy.md` belongs to T429, T593 and T615. Smallest plan change: move `test_site_copy_blocks.py` to T593, which already owns both files and rewrites both to D-3331's wording, or make T161 depend on T593. T161 keeps its other seven files.
Evidence: site-copy.md:32 "It goes to Havasu businesses" vs home.html:30 "Your request goes to"; site-copy.md:35 "or you can pick none" vs home.html:31 "you don't have to pick anyone"; fair 3 (:47), coverage title/line (:50, :53), business text (:61), WEB-024 business line (:68) and texting consent line (:23) are absent from home.html and site_layout.html. T593's goal (D-4098) says "the home page still renders the October 28 draft" and rewrites both files; T161 depends on T203, T202, T429, T488, not T593; no cycle either way.
Reviewer (opus, ab595414359dfc49d) confirmed (a)-(d) with file:line evidence: PLAN-GAP: CONFIRMED.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T161 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T161 --reason "..."` brings it back with the attempts it has left (D-2859).
