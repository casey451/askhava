---
id: T361-plan-gap
title: T361: plan gap reported, blocked for a plan fix
kind: blocked
---
T361's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/intake/src/job_texts.rs:473 (`(1..).zip(page)`) and :494 (`.filter(|(n, _)| others.contains(n))`). kit/tasks.json lists T240, T246, T247, T278, T295, T328, T402, T446 and T471 as owners of this file; T441 owns only portals files; app/quotes/src/notify.rs (list_lines) is owned by T047, T293, T328, T384. Evidence: T361 makes list_rows/list_lines number by shown_number (required by kit/prompts/T361.md:9) while page_quotes must stay in arrival order until T441, so lapsed_body pairs page places with shown-number LIST lines. Counterexample: A AI-read at t0, B by form at t0+5m; B shown #1, A confirmed later shown #2; page order A,B. B's pick lapses -> others = page place {1} = A, but list line 1 = B, so the lapse text offers B (who just lapsed) and omits A; "1 YES" then picks B again. Reviewer aa0b52b726d31bee2 confirmed ownership and the counterexample (PLAN GAP CONFIRMED). Smallest plan change: a task owning job_texts.rs (T402, T446, T471 or new) builds pickable/others from quotes::api::shown_numbers, merged with or before T361. Also: branch ~717 lines outside .sqlx vs 550 budget (D-3744), recorded in D-3980. Same page-place pattern in app/picks/src/email_code.rs:198-211 (owners T095, T382), consistent with the page until T441.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T361 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T361 --reason "..."` brings it back with the attempts it has left (D-2859).
