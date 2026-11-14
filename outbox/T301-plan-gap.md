---
id: T301-plan-gap
title: T301: plan gap reported, blocked for a plan fix
kind: blocked
---
T301's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/server/tests/door_matrix.rs:1078 and :1127 (T348's harness file, not owned by T301). judge_opened requires 200 for the fixture's own Quote link GET and for Y's Quote link on X's job. T301's goal (D-3189, CARD-023) requires the one not-found answer for a job with no sent card. Nothing in door_matrix.rs or door_matrix/released_pick.rs calls keep_card/mark_card_sent/sent_card (quoted_job only send_cards + submit_quote); job_page returns None without holds_card (app/quotes/src/job_page.rs:155). Y holds no card on X's job, so even a carded fixture does not fix case (b). Marking section 13 rows yes fails TD-SEC-61; leaving them T301 fails route_wiring.rs TD-SEC-41. T301 owns only app/server/src/action_links.rs and app/server/tests/action_links_route.rs. Reviewer confirmed. Fix: a harness task before T301 giving door_matrix's quoted jobs a kept and sent card and settling what Y's Quote link on X's job answers (not-found, or probe a Hold link); log in docs/decisions.md.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T301 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T301 --reason "..."` brings it back with the attempts it has left (D-2859).
