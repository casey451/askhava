---
id: T573-plan-gap
title: T573: plan gap reported, blocked for a plan fix
kind: blocked
---
T573's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/privacy/src/business_contacts.rs:124 (owned by T412, not T573). Item (2) needs the outbox sender to read a business line's phone number, which lives only in privacy.staff_phone; privacy::api::read_staff_phone refuses every purpose except BusinessPurpose::SignInCode (opens_phone at :124, refusal at :398). Messaging keeps no copy (sender directory holds blind-index digests only), and tests/security business_contact_readers.rs lets only privacy and server name read_staff_phone. Reusing SignInCode would falsify the SEC-047 audit. Smallest plan change: give T573 business_contacts.rs so it adds a BusinessPurpose::BusinessText purpose (own audit action and SEC-047 label, BusinessPurpose::ALL entry, update the one-purpose test and TD-PRIV-28), with the read going through the server composition. Reviewer confirmed: PLAN-GAP: CONFIRMED.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T573 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T573 --reason "..."` brings it back with the attempts it has left (D-2859).
