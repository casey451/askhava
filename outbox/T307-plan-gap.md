---
id: T307-plan-gap
title: T307: plan gap reported, blocked for a plan fix
kind: blocked
---
T307's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/security/tests/vault_readers.rs:23 lets only `portals` call `read_own_contact` (line 19 lets only `messaging` and `calls` call `read_contact`), and app/portals/src/homeowner_portal.rs:97 keeps `OnFile::phone()` crate-private (T087's file, not T307's). So `server` has no allowed way to read her number and address for `HomeFacts`. The prompt (kit/prompts/T307.md:9, D-3877 (3)) says to read them through `privacy::api::read_contact` from the server, which those two lines (and the runtime check at app/privacy/src/api.rs:247) rule out. Smallest plan change: let T307 own app/portals/src/homeowner_portal.rs to add one `pub async fn home_facts(db, vault, &HomeownerSessionProof) -> HomeFacts` there, reading phone and address through `read_own_contact` (as `OnFile::read` already does) and `texts_on` through `messaging::api::texts_on`; `server::homeowner_portal` would call it. Messaging's `read_contact` with `Purpose::Send` was not taken: it would log a portal read as a send in the vault audit and skip SEC-046's homeowner proof.
Evidence: `cargo test -p askhava-security --test vault_readers` exit=101, panicked at tests/security/tests/vault_readers.rs:189:5: ["app/server/src/homeowner_portal.rs: `read_own_contact` named outside its readers"]; test result: FAILED. 4 passed; 1 failed.
Reviewer (Opus, fresh eyes) confirmed: T307 owns only app/server/src/homeowner_portal.rs, app/server/tests/homeowner_portal_route.rs, app/messaging/src/held_messages.rs, tests/e2e/homeowner_portal_served.spec.ts; no public API in portals/privacy/messaging/calls/intake returns her phone or address; HomeFacts.phone_last4 and address are required by app/portals/templates/homeowner_home.html:46-47 (T561's file). PLAN-GAP: CONFIRMED.
Worker's work (routes, tests, e2e spec) was written but not 

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T307 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T307 --reason "..."` brings it back with the attempts it has left (D-2859).
