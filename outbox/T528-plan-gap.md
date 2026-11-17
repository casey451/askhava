---
id: T528-plan-gap
title: T528: plan gap reported, blocked for a plan fix
kind: blocked
---
T528's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/business_leaving.rs:463 (owned by T228, also listed by T343) asserts that businesses/src/leaving.rs is the only file in app/ naming `leave_reason`; D-3882 puts the BIZ-054 leaving-answers reader in app/businesses/src/digest_counts.rs, which must name the table. No public API returns the answers (leaving.rs's only public reader, reason_asked, returns a bool). Smallest plan change: let T528 add "businesses/src/digest_counts.rs" to the allowed list at business_leaving.rs:465, or give T528 leaving.rs so a crate-private leaving_answers query lives there.
Evidence: cargo test -p askhava-integration --test business_leaving -> exit 101; td_biz_32_a_long_pause_asks_once_and_changes_nothing panicked at business_leaving.rs:463:5 "no routing or fee code reads it", left ["businesses/src/leaving.rs", "businesses/src/digest_counts.rs"], right ["businesses/src/leaving.rs"]. Reviewer reproduced it and confirmed the ownership and that no API exposes the answers.
Also: the built work is about 826 lines (5 new files: 246+82+49+60+372 test) against the 550-line budget; a re-plan should split or raise the budget. digest_rows tests passed (7/7) and clippy was clean in the worker's run; nothing was committed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T528 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T528 --reason "..."` brings it back with the attempts it has left (D-2859).
