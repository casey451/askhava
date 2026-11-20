---
id: T617-plan-gap
title: T617: plan gap reported, blocked for a plan fix
kind: blocked
---
T617's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/a11y/design_tokens.spec.ts:221 and :242 (protected, not owned by T617) contradict the prompt's single DM Sans @font-face with font-weight: 100 1000 (kit/prompts/T617.md:9). loadedFonts() (:170-175) labels faces as family + declared weight; a range face reports "DM Sans 100 1000", so the expected "DM Sans 400"/"DM Sans 700" labels never appear. Worker's Chromium probe with one range block printed 'DM Sans 100 1000 loaded'; three blocks (400, 600, 700) all pointing at the one variable file printed 400/600/700 loaded with 1 font request. Smallest plan change: one @font-face per used weight (400, 500, 600, 700) each pointing at the variable file. License, integrity, SHA-256 and size checks all passed. Reviewer confirmed the ownership/contradiction.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T617 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T617 --reason "..."` brings it back with the attempts it has left (D-2859).
