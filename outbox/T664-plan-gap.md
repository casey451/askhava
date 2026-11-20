---
id: T664-plan-gap
title: T664: plan gap reported, blocked for a plan fix
kind: blocked
---
T664's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/picks/src/handlers.rs, app/picks/src/confirm.rs
The pick's window deadline (picks.pick.confirm_due) is read only by private code (handlers.rs store_due :263, hold_row :323); no public reader exists, and portals may not read picks tables. check_at_confirm needs it. Also the refused-at-confirm tail (job back to Quotes ready, PICK-046/047 text: end_waiting, queue_backup_text, waiting_pick) is crate-private in picks. A picks-side task must export a deadline reader and a refused-at-confirm tail first. Reviewer confirmed (GAP CONFIRMED). End/bond dates are also not stored anywhere.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T664 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T664 --reason "..."` brings it back with the attempts it has left (D-2859).
