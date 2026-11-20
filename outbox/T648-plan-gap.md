---
id: T648-plan-gap
title: T648: plan gap reported, blocked for a plan fix
kind: blocked
---
T648's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/recovery/src/restore.rs:44 include_str!("../../../scripts/bootstrap_roles.sql") is required by the goal, but scripts/check_include_paths.py (D-1867, run in the smoke by tests/test_check_include_paths.py::test_td_ops_32_the_real_repository_passes) fails: scripts/bootstrap_roles.sql is not matched by the Checks and Coverage rules in .github/workflows/ci.yml (protected, harness-only) nor by RUST_PATHS in scripts/prehandin.sh:77 (locked in kit/plan.lock). T648 owns neither. Fix: a harness task adds scripts/bootstrap_roles\.sql to those three rules and rewrites kit/plan.lock. Reviewer a3d22f0629d9ff35a confirmed (GAP-VERDICT: CONFIRMED). Also: owned files are 551 + 348 lines vs 450 budget. Work uncommitted in tree; smoke 1 failed (that test only).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T648 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T648 --reason "..."` brings it back with the attempts it has left (D-2859).
