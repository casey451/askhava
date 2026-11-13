---
id: T280-plan-gap
title: T280: plan gap reported, blocked for a plan fix
kind: blocked
---
T280's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/portals/src/business_auth.rs:176-178 CodeAddress::code_to(db, member) gets only a team-member row ID; no public API in any crate maps a member ID to business, mailbox or own staff-phone lines (businesses::api and team.rs lookups need the business or mailbox first: team.rs:613 members, api.rs:178 businesses_of_mailbox, channel_preference.rs:132 notice_recipients; locked_member team.rs:253 is private). server runs no SQL (0 files with sqlx::query) and T280 owns no businesses file (kit/prompts/T280.md:34-39, :49), T415 adds only contact_ids_in_use (bulk). Reviewer confirmed. Smallest fix: a businesses task before T280 adding businesses::api::member_contact(conn, member) -> Option<(business, mailbox, Vec<own staff_phone line IDs>)>, current members only. Other problems for the re-plan: (1) tests/e2e/webserver.ts:51-70 starts the app with no vault keys or Twilio/Verify settings, so refusing to start breaks Ally/e2e; (2) admit_code_try needs the link's business and models::open_link is pub(crate); (3) queue_account_email stores the rendered link body in the outbox in plain text against SEC-026; (4) goal asks for a business_signin_link template in en.json/es.json but the prompt says the task owns no customer copy.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T280 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T280 --reason "..."` brings it back with the attempts it has left (D-2859).
