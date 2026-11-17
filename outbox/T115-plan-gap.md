---
id: T115-plan-gap
title: T115: plan gap reported, blocked for a plan fix
kind: blocked
---
T115's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: `package-lock.json` (unchanged from main) fails `npm audit --audit-level=low` on GHSA-fc5q-39xc-vjpf; nothing in `package.json`/`package-lock.json` can fix it; the smallest plan change is a harness task that adds a reviewed audit exception for this advisory to `scripts/prehandin.sh` and Ci's audit step.
Evidence (confirmed by reviewer a72d87631599620e5): advisory covers picomatch <=4.0.7, latest published is 4.0.7, no patched version, so no override works. Path: typescript-eslint@8.71.2 -> @typescript-eslint/typescript-estree@8.71.2 -> tinyglobby@0.2.17 -> picomatch@4.0.7 (also via fdir); newest typescript-estree 8.73.0 still needs tinyglobby, newest tinyglobby needs picomatch ^4.0.4. npm's suggested typescript-eslint@8.47.0 needs eslint ^8.57||^9 and typescript <6, repo pins eslint 10.12.0 and TS 6.0.3. origin/main's unchanged lockfile fails the same way (9 high). scripts/prehandin.sh:143-147 and .github/workflows/ci.yml:309-316 run the audit whenever an npm path changes (tests/a11y/*.ts), so any task touching TypeScript is blocked; only protected harness paths can fix it.
Work on branch (heads 970e88f79, fix 5cde1fda2, not pushed): fix round's prehandin red only at npm audit; fmt/clippy clean, cargo test -p askhava-site 31 passed, home_numbers 3 passed, test_email_accessibility.py 22 passed, statement.spec.ts 6 passed, tsc/eslint 0. prehandin --ally: 2-3 failures in tests/a11y/j2_keyboard.spec.ts:149 on reused local DB (reviewer: nothing in the diff can affect J2).
Remaining review findings for the retry (second review FINDINGS after the one fix round): (1) tests/a11y/statement.spec.ts:26-27 CALL_WORD matches whole words only; misses "Request a callback.", "Talking to us is easy.", "Speaking with us by voice works.", "Telephoning us works too.", "Contact us 

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T115 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T115 --reason "..."` brings it back with the attempts it has left (D-2859).
