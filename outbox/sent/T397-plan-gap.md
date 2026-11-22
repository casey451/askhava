---
id: T397-plan-gap
title: T397: plan gap reported, blocked for a plan fix
kind: blocked
---
T397's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/server/tests/door_matrix.rs:1182. owner_jar signs the owner in at key.now + 30s*k per probed Owner row; past ~60 rows expires_at < created_at, violating portals.session CHECK (expires_at > created_at), so POST /owner/help/close as Owner returns 500. Marking the three /b/phones rows yes in docs/architecture.md section 13 turns td_sec_61_every_served_door_row_over_http red (reproduced by worker and by reviewer: exit 101, door_matrix.rs:1533). Smallest fix: a harness task that bases owner_jar sign-in times in the past (e.g. key.now - OWNER_IDLE_LIMIT) or spaces TOTP steps differently. Prompt forbids editing door_matrix.rs and forbids working around an unreachable route. Worker work is uncommitted in the tree.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T397 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T397 --reason "..."` brings it back with the attempts it has left (D-2859).
