---
id: T408-plan-gap
title: T408: plan gap reported, blocked for a plan fix
kind: blocked
---
T408's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/safe_mode.rs:480-484 (owned by T487) fails on any `safe_mode_on(` call outside `payments/src/handlers.rs` and `app/ops/src/` ("only charges read safe mode"). This task's goal (D-3144, D-3879; OPS-001 at `docs/spec/ops-recovery.md:5`; TD-OPS-01 at `docs/test-design.md:811`, "in safe mode no charge and no release happen") requires `PickReleaser` and `address_in` in `app/server/src` to call `ops::api::safe_mode_on`. Smallest plan change: a harness task lets that scan also allow `server/src/release_hold.rs`, or the scan checks only that text paths never read safe mode.
Evidence (run here): `cargo test -p askhava-integration --test safe_mode td_ops_01_every_action_checks` panicked at tests/integration/tests/safe_mode.rs:482:13 "only charges read safe mode: app/server/src/release_hold.rs"; 0 passed, 1 failed.
Reviewer confirmed: kit/tasks.json:16428 gives safe_mode.rs to T487; T408 owns neither it nor app/payments/src/handlers.rs; no T408 prompt line or decision foresees the scan (D-4000 at docs/decisions.md:2241, app/ops/src/safe_mode.rs:14 say only charges_paused reads it). Routing through charges_paused would also pause releases when the Charging/All switch is off, a behaviour change. Other notes for the plan fix: no ops::api writes ops.alert_push (T204 builds it, not a dependency), so tests need an injected safe-mode reader; the WIP diff was about 1,040 lines against the 450 budget, and it also touches app/payments/src/api.rs and app/server/src/lib.rs. Nothing was committed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T408 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T408 --reason "..."` brings it back with the attempts it has left (D-2859).
