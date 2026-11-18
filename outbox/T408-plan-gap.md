---
id: T408-plan-gap
title: T408: plan gap reported, blocked for a plan fix
kind: blocked
---
T408's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: kit/prompts/T408.md:54 "Size budget: at most 6 files, 800 changed lines" cannot be met. The prompt's own plan changes (D-3144, D-3879, D-4026, D-4326) require edits to at least 13 files (release_sink.rs, new release_hold.rs, release_proof.rs, server lib.rs, payments api.rs, server tests/safe_mode_release.rs, integration safe_mode.rs, ops safe_mode.rs, three NOTES.md, architecture.md, decisions.md), and tests for every held_release money branch (Stripe fetch failure, pick ended while held, no fee row) plus address_in through its registered handler, which the first review required (95% branch floor on payments).
Evidence (run here): `git diff --shortstat origin/main...HEAD` = 14 files changed, 1106 insertions(+), 30 deletions(-) (1,136 changed lines; the first commit alone was 949+29). Production code ~290 lines; tests ~830 (server TD-OPS-40 test 624, payments unit test ~130, integration safe_mode.rs 79). Server tests may not write payments rows (architecture_migrations check_query_schemas), so the ended-pick branch needed a payments unit test (D-4021).
Reviewer (Opus, fresh) confirmed: PLAN GAP CONFIRMED; the code itself approved at 56a5f21e77fe6da7363db5abceb60648e88b6d22 (cargo test -p askhava-payments --lib release_proof 2 passed; -p askhava-server --test safe_mode_release 5 passed).
Worker's checks at that head: fmt, clippy -D warnings clean; smoke.sh green; prehandin.sh green incl. "ok coverage floor" for app/payments and release_sink.rs (PREHANDIN_COVERAGE_TIMEOUT=1500); prehandin --ally red on j2_keyboard.spec.ts:149 (Stripe card mock timeout / heading not found / toHaveText, 2-3 failed of 807), which the reviewer judged unrelated to this branch (no page, template or route change).
Smallest plan change: raise T408's budget to about 1,150 lines and 14 fil

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T408 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T408 --reason "..."` brings it back with the attempts it has left (D-2859).
