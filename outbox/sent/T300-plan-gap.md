---
id: T300-plan-gap
title: T300: plan gap reported, blocked for a plan fix
kind: blocked
---
T300's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: tests/e2e/webserver.ts:52-70 (owned by T227 and T385) starts the e2e server with no ASKHAVA_TWILIO_ACCOUNT_SID, ASKHAVA_TWILIO_AUTH_TOKEN, ASKHAVA_TWILIO_VERIFY_SERVICE_SID, VAULT_KEY or VAULT_INDEX_KEY, so business_signin.rs:129 uses NoVerify (every code send/check fails) and lib.rs:321 vault_from gives no vault; the browser half of TD-BIZ-42 cannot pass. Also tests/a11y/fixtures.ts:126-130 (owner-only since T267) opens page states with a bare page.goto and a seed returns only a path, so no signed-in /b page state can be axe-checked. Reviewer confirmed all three claims. Smallest fix: a harness task before T300 adding the five fake settings to appEnv in webserver.ts, a way to seed a sealed fake vault contact, and a way for a page-state seed to sign the browser in. Router half (business_portal.rs, business_portal_route.rs) is doable meanwhile.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T300 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T300 --reason "..."` brings it back with the attempts it has left (D-2859).
