---
id: T460-plan-gap
title: T460: plan gap reported, blocked for a plan fix
kind: blocked
---
T460's worker reported that only a plan change can unblock it; the task is BLOCKED and is not retried on its unchanged prompt.

PLAN GAP: app/businesses/src/models.rs:269, :307 (pub(crate) SignupState, signup_state; owned by T041, T081, T083). D-3641 needs the server to read the sign-up's stored state (identity Step, code_done, email verified) before each identity step, but no public businesses::api reader gives it: signup_email_waits is not exported (lib.rs:49 private mod), RecordFound::masked() cannot tell "no step yet" from step 1, check_code (identity.rs:267-271) takes a try before it finds no step, and the server crate has no SQL (R8). T460 owns only app/server/src/business_signup.rs and app/server/tests/business_signup_route.rs; no queued task (T382, T468, T475-T484) adds the reader. Smallest fix: T460 also owns a new app/businesses/src/signup_progress.rs with one public reader businesses::api::signup_progress(conn, business) -> Result<SignupProgress { step: Option<Step>, code_done: bool, email_waits: bool }, BusinessesError>, exported by one pub use line in api.rs. Reviewer confirmed the claim with file:line evidence (PLAN-GAP: CONFIRMED). T461 inherits the same need.

Check the cited file, owner or contradiction. If the plan is wrong, change it (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T460 --plan-fix D-### --reason "..."`. If the report is mistaken, `python3 -m runner --requeue T460 --reason "..."` brings it back with the attempts it has left (D-2859).
