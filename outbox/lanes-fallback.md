---
id: lanes-fallback
title: Runner: back to one lane
kind: info
---
Lane 4 had 2 harness-type failures in a row, so the runner now works one task at a time (kit/runner.json still says more):
- T414 check_failed: Twin: failure
Twin failed at step: Build rules_bridge from this commit (T074)
##[error]Command failed: rustc -vV
- T414 check_failed: Ci: failure; Coverage: failure
Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly)
test td_call_23_a_second_unlinked_phone_on_the_line_stores_nothing ... ok
test td_call_23

Tasks already in flight finish normally. Once the cause is fixed, run `python3 -m runner --restore-lanes` to use every lane again (D-620).
