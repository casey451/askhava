---
id: T674-plan-gap
title: T674: plan gap reported, blocked for a plan fix
kind: blocked
---
T674's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: tests/a11y/fixtures.ts — GET /photos/{token} needs one NOT_PAGES line there (A11Y-002; a PAGE_STATES entry cannot cover a JPEG or 404). The file is owner-only since T267 moved the registry (page_states.ts:7-8); T674's Scope shares it only until then, and only for PAGE_STATES rows. Reviewer confirmed: no task grants that line (T766 does not). plan_pages.py gives a false negative for T674 because its goal mentions page_states.ts. Smallest plan change: grant T674 the one NOT_PAGES line in fixtures.ts. Worker's uncommitted work was not committed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T674 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T674 --reason "..."` brings it back with the attempts it has left (D-2859).
