---
id: T302-plan-gap
title: T302: plan gap reported, blocked for a plan fix
kind: blocked
---
T302's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/src/business_signup.rs:226-242 (Screen enum) has no screen to type the identity code or the emailed sign-up code; nothing in app/portals calls businesses::api::check_code (identity.rs:260), start_signup_check (email_verification.rs:134), verified_by_link (:178) or confirm (:363); templates/business_signup.html has no code input and no texts choice or text phone field (Entered.text_chosen at :344 and SignupConsent.text_phone at :373 can never come from a post). T302 (kit/prompts/T302.md:34-38, :158, :164) owns no templates or customer copy, yet TD-SEC-45 (T302.md:73) needs the email code step and the e2e needs the identity-code step. No task in kit/tasks.json owns these screens (T202 depends on T302 and does not cover them). The form also posts to /b/signup/start, not SIGNUP_ROUTE. Proposed: a portals task before T302 adding the code-entry screens and the text choice/phone fields, T302 depending on it. Reviewer confirmed all claims.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T302 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T302 --reason "..."` brings it back with the attempts it has left (D-2859).
