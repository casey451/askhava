---
id: T302-plan-gap
title: T302: plan gap reported, blocked for a plan fix
kind: blocked
---
T302's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/security/tests/auth_matrix.rs:146 and app/server/tests/door_matrix.rs:161 treat only /b/signup itself as the open sign-in door (exact match); every other /b/signup/... path (T450's /b/signup/start, /send, /old, /letter, /identity-code, /email-code, /email-code/resend, /service-area at app/portals/src/business_signup.rs:92-104,303) falls to the Business door (signed-in business only, auth_matrix.rs:162,177; door_matrix.rs:173-174,186). T302 must serve them without a session (TD-SEC-45). T302 owns neither file (kit/prompts/T302.md:34-38,50,146), nor tests/security/door_cases.txt:10, nor tests/a11y/page_states.ts (PAGE_STATES entry required by TD-SEC-45). route_wiring.rs:253 and door_matrix.rs:647 build the router via build_router without sign-up. Smallest fix: a harness task opens every /b/signup... path except /b/signup/capability like a sign-in page in auth_matrix.rs, door_matrix.rs and door_cases.txt, has route_wiring.rs and door_matrix.rs build the router with sign-up merged in, and adds the PAGE_STATES entry. Reviewer confirmed each point. Work is uncommitted in the T302 tree.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T302 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T302 --reason "..."` brings it back with the attempts it has left (D-2859).
