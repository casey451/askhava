---
id: T489-plan-gap
title: T489: plan gap reported, blocked for a plan fix
kind: blocked
---
T489's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/security/tests/auth_matrix.rs:722. The `POST /request` route this task must serve (D-3878, D-3882) makes `sec_048_every_refusal_is_a_security_event` panic at auth_matrix.rs:181:9 ("/request has no door in auth_matrix.rs: add its matrix rule (SEC-019)"). sec_048 (:720-:723) builds its POST list from route_table() and calls door(path, false), ignoring the served-by-site flag that td_sec_01 (:637) and :693 pass; so any site POST outside the listed prefixes panics. POST /request is the first site POST. The file is owned by T348 (harness; also T088, T231, T302, T308); T489 does not own it and its prompt says it edits neither door test. Smallest plan change: a harness task changes sec_048's filter to `routes().into_iter().filter(|(path, (methods, site))| methods.contains("post") && matches!(door(path, *site), Door::Link))`, as td_sec_01 does at :637. Reviewer reproduced (cargo test -p askhava-security --test auth_matrix: 5 passed, 1 failed) and confirmed no legitimate alternative within owned files (serving from server still panics; moving the path breaks D-3878/WEB-003; dropping the POST breaks D-3882/D-3189). Also noted: drafted diff already ~805 lines across 10 files before the e2e spec, against a 550-line budget; the retry likely needs a bigger budget. Draft saved at /tmp/T489-wip.patch (uncommitted).

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T489 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T489 --reason "..."` brings it back with the attempts it has left (D-2859).
