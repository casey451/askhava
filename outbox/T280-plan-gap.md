---
id: T280-plan-gap
title: T280: plan gap reported, blocked for a plan fix
kind: blocked
---
T280's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/src/business_auth.rs:130 (SignInMail::addresses) and :176 (CodeAddress::code_to) have nothing to look up. No module stores a business member's sign-in email or texting phone in a readable/searchable form, and no task in kit/tasks.json builds one. Evidence: privacy.contact_index only ever written for HomeownerPhone (app/privacy/src/vault.rs:398,409); email_index (app/privacy/src/reader.rs:321) only computes a digest and nothing calls it; migrations/0056_businesses_team_members.sql:5-11 says email/phone is never stored there; Reader list (app/privacy/src/models.rs:89-102) has no business sign-in reader; D-1724 (docs/decisions.md:1243) names the prerequisite but assigns it to no task; T397 rests on the same missing lookup. Reviewer confirmed. Smallest fix: a task before T280 (privacy+businesses) storing each member's sign-in address and phone in the vault keyed by mailbox_id / staff phone line ID, with an address-to-mailboxes lookup and a read grant for business sign-in; T280 and T397 depend on it.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T280 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T280 --reason "..."` brings it back with the attempts it has left (D-2859).
