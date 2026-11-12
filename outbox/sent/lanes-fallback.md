---
id: lanes-fallback
title: Runner: back to one lane
kind: info
---
Lane 5 had 2 harness-type failures in a row, so the runner now works one task at a time (kit/runner.json still says more):
- lane 5: 3 lost races in a row in one run
- lane 5: 3 lost races in a row in one run

Tasks already in flight finish normally. Once the cause is fixed, run `python3 -m runner --restore-lanes` to use every lane again (D-620).
