---
id: T663-plan-gap
title: T663: plan gap reported, blocked for a plan fix
kind: blocked
---
T663's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/test_coverage_floor.py:125 asserts floors["future"] == [], contradicting T663 goal (5), which requires a `future` entry for app/portals/src/confirm_moves.rs. Also the test at :183-196 requires per-file floors and files_measured to hold the same paths, on disk, and named in D-3974, so the new connect.rs and confirm_moves.rs floors fail it. T663 does not own tests/test_coverage_floor.py (T264/T324 do). Reviewer confirmed. Smallest fix: relax the floor tests to accept reasoned future entries, or drop goal (5) from T663. Reviewer also found TD-JOB-18 cases untested (paid settle, held_success run, safe-mode, late success) and connect.rs coverage unmeasured.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T663 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T663 --reason "..."` brings it back with the attempts it has left (D-2859).
