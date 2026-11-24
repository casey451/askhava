# askhava-portals: portals and the shared web security

## Purpose
The layer-8 crate for the homeowner, business and owner portals (`docs/architecture.md` section 3).

## Never break
- No cookie without the `__Host-` attributes, and no cookie survives a login: `tests/security/tests/cookies_csrf.rs`. More: `(never 1)` in NOTES.md.
- Every response carries the SEC-012 headers, HSTS has no `preload`: `tests/security/tests/headers.rs`. More: `(never 2)` in NOTES.md.
- Security events carry no phone, name or path, 50 of one kind in an hour raise one alert, and a steady 50 an hour raises one per hour, not one per event: `cookies_csrf.rs`. More: `(never 3)` in NOTES.md.
- Quote numbers never move; Pick and "or reply <n> YES" from #1; no sort or filter control: `src/tests/prop_arrival_order.rs`, `tests/integration/tests/quote_page.rs`. More: `(never 4)` in NOTES.md.
- Only the Terms line beside the final Pick: `src/tests/pick_flow.rs`, `tests/e2e/pick_flow.spec.ts`. More: `(never 5)` in NOTES.md.
- The code step: labels before hints before inputs: `tests/a11y/pick_forms.spec.ts`, `pick_flow::code_tests`. More: `(never 6)` in NOTES.md.
- A sign-in link works once and for 15 minutes: `tests/security/tests/business_signin.rs`. More: `(never 7)` in NOTES.md.
- A notice link opens sign-in for its own person only, Guarded by `tests/integration/tests/notice_signin_link.rs`. More: `(never 8)` in NOTES.md.
- The feedback share box is unticked on first view, has JOB-017's label, and no page offers a reward or says shown feedback is all of it: `feedback::tests`, `tests/e2e/feedback.spec.ts`.
- A guarded change without a code from the last 5 minutes is refused and logged, at 4 minutes it passes, at 5:01 it is refused: `tests/security/tests/step_up.rs`.
- Billing and CSV equal the ledger's business lines with no PDF; Guarded by `business_portal::tests`. More: `(never 11)` in NOTES.md.
- The release page prints the first name once, ends the details with the tip, never holds a phone: `job_release::tests`; signed out, nothing: `tests/e2e/business_portal.spec.ts`. More: `(never 12)` in NOTES.md.
- `refuse_cross_site` is never layered over webhooks (their callers send no `Origin`).
- Owner sign-in needs both factors; Guarded by `tests/security/tests/owner_auth.rs`. More: `(never 14)` in NOTES.md.
- An action link opens only its own action and object, works once, ends at its window and dies with its member: `tests/security/tests/action_links.rs`. More: `(never 15)` in NOTES.md.
- The action links are served by `server::action_links` (T301): `app/server/tests/action_links_route.rs`. More: `(never 16)` in NOTES.md.
- SEC-004, SEC-027, D-3675: `confirm_link_session` signs in only the person a used Confirm link was emailed to, in the transaction that used it: `app/server/tests/action_links_route.rs`. More: `(never 17)` in NOTES.md.
- SEC-004, PICK-033, D-4425: only a token matched under the `action-text:` binding (`mint_text_confirm_link`, Confirm only) gives a `text_started` grant: `app/server/tests/action_links_route.rs` (TD-SEC-80). More: `(never 18)` in NOTES.md.
- `send_sign_in_link` sends only to a current member and never past `LINKS_PER_LIFE` in `LINK_LIFE`: `tests/integration/tests/signup_mail_paths.rs` (T468).
- `decide_at_confirm` gives no free first win to a business with no sign-up row or with no launch setting: `tests/integration/tests/confirm_waiver.rs` (T494). More: `(never 20)` in NOTES.md.
- The Confirm route charges or waives only the stored `pick_fee`: `app/server/tests/confirm_route.rs`. More: `(never 21)` in NOTES.md.
- The quote page's close button (JOB-019) posts to `CLOSE_ROUTE`, never the page's own path: `tests/integration/tests/quote_page_close.rs`. More: `(never 22)` in NOTES.md.
- BIZ-049, D-3676: the sign-in done screen adds its capability-step link only for an owner whose business has no saved capability row: `app/server/tests/business_signup_route.rs`. More: `(never 23)` in NOTES.md.
- SEC-014, SEC-020, D-2634: every pick code send (final Pick, "Send a new code") passes `admit_code_send` and every shaped "Finish my pick" try passes `admit_code_try` for the client IP before any Verify call: `tests/integration/tests/pick_code_limits.rs`. More: `(never 24)` in NOTES.md.
- BIZ-033, D-3081: `phone_approval_page` shows only a waiting phone's last 4 digits, never a whole number, and every form names the `number_set_at` shown: `phone_approval::tests`, `app/server/tests/phone_approval_route.rs`.
- JOB-016, PRIV-006, D-4423: `owner_problem_reports.html` shows no homeowner contact and preselects no kind; a counted report has no form: `owner_complaint::page_tests`, `app/server/tests/owner_problem_route.rs`.
- AI-008, SEC-043, D-5688: every `/owner/ai` change (both records and both switch directions) needs `OwnerAction::AiSwitch`'s fresh factor and records nothing without it: `app/server/tests/owner_ai_switch_route.rs` (TD-AI-32).
- BIZ-047, D-3155: every page served behind the business session extends `business_layout.html` and joins `BUSINESS_PAGES`; no other template fills `get_help` or links the form: `src/tests/get_help_link.rs` (TD-BIZ-23).

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/portals/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/portals/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
