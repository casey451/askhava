---
id: T810-plan-gap
title: T810: plan gap reported, blocked for a plan fix
kind: blocked
---
T810's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: outside ownership: app/server/tests/confirm_press_code.rs. TD-SEC-82 requires a direct portals.shared_inbox insert (never set_inbox_shared/record_step_up, D-5319), but TD-SEC-10 (tests/integration/tests/architecture_migrations.rs check_app :737, check_sql_forms :660-677, schema_refs :295) scans every .rs under app/: macro form fails "`portals.` names another module's schema", plain sqlx::query fails "SQL outside the query macros: `query`". Reviewer confirmed. Smallest plan change: T810 owns tests/integration/tests/confirm_press_code.rs (not scanned; confirm_terms.rs:238 does the plain insert there); update docs/test-design.md:939 and kit/tasks.json TD-SEC-82 paths and run command to `cargo test -p askhava-integration --test confirm_press_code`.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T810 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T810 --reason "..."` brings it back with the attempts it has left (D-2859).
