---
id: T652-plan-gap
title: T652: plan gap reported, blocked for a plan fix
kind: blocked
---
T652's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/integration/tests/safe_mode.rs:246,281,320,378,407 (owned by T487 and T408; T652's prompt :11 and :47 say it must pass unchanged) plant Severity::High alerts of the made-up kind "ops.ledger_difference" and expect safe mode to start. D-4310 (docs/decisions.md:2366) lets a High alert start safe mode only for ops.vendor_errors, ops.health_check_failed, payments.ledger_mismatch or messaging.number_suspended. The worker made a trial edit (not committed) and `cargo test -p askhava-integration --test safe_mode` gave 3 passed, 4 failed: td_ops_01_safe_mode_at_24_00_00_not_at_23_59_59, td_fee_20_safe_mode_stops_and_resumes_both_payment_timers, td_ops_06_safe_mode_ends_when_no_other_high_alert_is_open, td_ops_06_a_press_in_safe_mode_is_refused_before_any_stripe_call. "ops.ledger_difference" appears nowhere in app/ or migrations, so no alias exists. The reviewer (Opus) confirmed the ownership and the contradiction. Fix: let T652 own tests/integration/tests/safe_mode.rs, or add a harness task, and swap the kind for askhava_ops::api::LEDGER_ALERT ("payments.ledger_mismatch"), with the assertions unchanged; check ordering against T408. Nothing was committed.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T652 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T652 --reason "..."` brings it back with the attempts it has left (D-2859).
