# askhava-businesses: business accounts and the records matching reads

## Purpose

The layer-4 crate that holds each business account (`docs/architecture.md` section 3). T041 built the account status and its moves, the towns, the job-type checklist within the license, the overflow switch, the Spanish tick, the business-reported insurance answer and the after-hours tick with the night count (D-112). T220 added team members with their roles, the main and staff phones kept in `messaging`'s sender directory, and `notice_addresses`. T182 added the self-pause and the business's own quiet times. T081 added sign-up, the identity chain with automatic activation, the About us screen and the business terms acceptance. T082 added the E-SIGN consent records, the welcome request and gate, the business STOP email request, the auto-reply rule and the recruiting-email rules. The after-hours conditions (T150) and the verified-email check before activation (BIZ-040, T196) come later.

## What it owns

- `src/api.rs`: the only public surface: `create`, `apply`, `pause_for_rule`, `pause_self`, `resume_self`, `set_ticks`, `set_towns`, `set_job_types`, `set_own_hours`, `own_hours`, `night_count`, `business`, `towns`, `job_types`, the types below, and `api::rules` (`next`, `checklist`, `towns`, `may_notify`, `may_resume_itself`, `APP_OPEN_MINUTE`, `APP_CLOSE_MINUTE`); from `team`: `Acting`, `add_owner`, `add_member`, `set_roles`, `remove_member`, `link_phone`, `change_phone`, `unlink_phone`, `members`, `notice_addresses`, `Roles`, `NoticePurpose`, `TeamMember`, `TeamError`, `SenderLink`, `MEMBER_REMOVED_KIND`.
- `src/signup.rs` (T081): `sign_up` (business, sign-up row and audit row in one transaction, then `payments::api::ensure_customer` outside it, BIZ-001), `waitlisted`, `category_list` ("Open now" or "Coming later" from `licensing`'s `ClassMap::opening`, BIZ-021), `WAITLIST_LINE`, `HeardFrom` and the `g` code (BIZ-053), `hand_entity_keys` (FEE-011 digests to `payments`), `accept_terms` (TERMS-033), `review_due` and `review_told` (BIZ-020), `SignupError`. T193: `SOMEONE_ELSE_LINE` (BIZ-024); `MaskedContact` (kind and masked value only, `for_step` from `RecordContacts`, `phone` for a landline), `fix_record_lines`, `NUMBER_OLD_BUTTON`, `STOP_FIX_BUTTON`, `RecordSeen` (record contacts plus the full record address), `number_is_old`, `resume_on_new_record`, `waiting_on_record`, `RECORD_UPDATED_TEMPLATE` (BIZ-026); `SignupSettings` with `REVIEW_TARGET_KEY` and `LIST_DELAY_KEY`, `review_target`, `review_target_line` (BIZ-027); `FeeNumbers`, `fee_summary`, `BUSINESS_STATEMENT`, `REPEAT_FREE_DAYS` (BIZ-030); `trade_count`, `claim_launch_notice` (BIZ-031).
- `src/identity.rs` (T081): `RecordContacts`, `Step`, `next_step` (text, email, letter; no voice), `send_code` and `check_code` through `VerifyAdapter`, `letter_code` (keyed, never stored), `Flag`, `record_match`, `settle`, `approve` (BIZ-002, BIZ-003, BIZ-016).
- `src/about_us.rs` (T081): `screen`, `screen_public_name`, `set_about_us`, `AboutUsRefusal` with its shown reason (BIZ-022, PICK-007, PICK-041, WORD-003); its own word lists, since `businesses` cannot call `quotes` or `ai`.
- `src/status.rs`: `AccountStatus` (six statuses, `code`, `from_code`, `receives_cards`: only `Active`, read by `quotes` for CARD-001 and CARD-016, T044), `PauseReason`, `Refused`, `next`, `replaces_self_pause` (every match on a status lives here, SEC-031).
- `src/consent.rs` (T082): `CONSENT_VERSION`, `NO_CALLS_LINE`, `ConsentChannel` (email, text; no call), `ConsentBox`, `consent_wording`, `consent_box`, `record_consent`, `may_text` (CON-019); `WELCOME_KIND`, `request_welcome`, `welcome_sent`, `may_get_cards`, `welcome_lines` (CON-020); `PHONE_STOPPED_KIND`, `phone_stopped`, `stopped_email_line` (BIZ-017); `AUTO_REPLY_TEMPLATE`, `auto_reply_due` (BIZ-032); `ADVERTISEMENT_LINE`, `RecruitingEmail`, `recruiting_email`, `stop_recruiting`, `may_recruit` (BIZ-048); `ConsentError`.
- `src/rules.rs` (pure): `Event`, `Checks`, `JobTypeChoice`, `checklist`, `towns`, `OwnHours`, `Widens`, `may_notify`, `may_resume_itself`.
- `src/models.rs`: `Business`, `Insurance`, `BusinessesError` and every query.
- `src/service.rs`: the transactions with their audit rows; `Ticks`; the own-hours and self-resume queries (T182).
- `src/team.rs`: team members, roles, staff phones and the notice-address lookup, with their own queries and `TeamError` (D-963, D-964).
- Migration `0055_businesses_records.sql`: the `businesses` schema, `businesses.business` (no `DELETE` for `askhava_app`), `businesses.town`, `businesses.job_type_choice`. No name, phone, email or address column.
- Migration `0056_businesses_team_members.sql`: `businesses.team_member` (owner row with `is_owner`, `mailbox_id` opaque email ID, role ticks, `removed_at`; no `DELETE` for `askhava_app`) and `businesses.staff_phone` (line ID, linking member, main mark; no number).
- Migration `0057_businesses_own_hours.sql`: `own_open_minute`, `own_close_minute` on `businesses.business` (Arizona minutes, both or neither, inside 480 to 1200 by a check).
- Migration `0120_businesses_signup_resume.sql` (T193): `waiting_on_record_since` with `old_text_key`, `old_mail_key`, `old_letter_key` (all or none), `trades` (sign-up category codes) and `launch_notice_sent_at` on `businesses.signup`.
- Migration `0121_businesses_consent.sql` (T082): `businesses.esign_consent` (append-only; member, channel `email` or `text` only, `phone_id` on text rows only, wording version, time; unique per member, channel, phone and version), `businesses.welcome` (requested and sent times; only `sent_at` is updatable), `businesses.recruit_stop` (keyed address digest, append-only).
- Migration `0119_businesses_signup_identity.sql`: `businesses.signup` (one row per business: `mailbox_id`, source answer, `g` code, identity step, code and match ticks, flag, `review_since`, `review_told_at`, `about_us`; no `DELETE`) and `businesses.terms_acceptance` (append-only TERMS-033 rows, channel `business_signup`).

## Rules it enforces

- BIZ-009, BIZ-003, BIZ-016, BIZ-021: `status::next` (pending to active only with the code, the match and no flag; a flag to needs review; needs review to active only on Casey's approval with the code and the match; waitlist to pending when a category opens; only active pauses, only paused resumes; removed is final; D-953). `service::apply` moves only if still in the status read (JOB-004) and writes the status audit row in the same transaction (JOB-005).
- BIZ-004, BIZ-049, LIC-043: `rules::checklist` and `service::set_job_types` (only job types in the caller's `allowed` set; D-954).
- BIZ-005, BIZ-006: `service::set_ticks` (`overflow_on`, `speaks_spanish`).
- BIZ-008: `Insurance` is stored as reported; no event of `next` reads it, so it never pauses or activates (D-061).
- BIZ-007: the `after_hours` column and `service::night_count` (active, ticked, in the town and category); its conditions are T150's (D-955, D-491).
- BIZ-010: `rules::towns` and `service::set_towns` (catalog town codes); `service::pause_self` and `resume_self` (only from a `self` pause, row locked; D-969); `service::pause_for_rule` (a rule's pause replaces a `self` reason, never another rule's; D-970); `set_own_hours` never changes a removed business (D-971); `rules::OwnHours`, `service::set_own_hours`, `own_hours` and `rules::may_notify` (own quiet times only narrow 8 a.m. to 8 p.m. Arizona time; D-085, D-968). No weekly card limit exists (D-028).
- BIZ-023: `team::add_owner`, `add_member`, `set_roles`, `remove_member` (one audit row each; only the owner, as the `Acting` member, adds, changes or removes; the owner holds both roles and is never removed, also by a check constraint); `Roles::new` refuses neither role.
- CALL-012: `team::link_phone`, `change_phone`, `unlink_phone` (owner or "Confirm and billing" only; the main phone owner only; `may_handle_phone`); `remove_member` unlinks every phone the member linked and enqueues one `businesses.member_removed` task that `portals` handles (TD-BIZ-06).
- D-491, D-963: every phone add or change goes through `SenderLink` (the composition's `messaging::api::sender_add` with the vault) and every unlink through `messaging::api::sender_remove`, in the phone change's transaction.
- D-492, D-964: `team::notice_addresses` (owner first; cards to "Cards and quotes", picks and release and money to "Confirm and billing", account to the owner alone).
- BIZ-001, BIZ-019, BIZ-021, BIZ-053: `signup::sign_up` refuses a form with no email, no category, a switched-off category or a malformed `g` code; only coming-later choices waitlist.
- BIZ-002, BIZ-003, BIZ-016, D-059, D-281: `identity::next_step`, `send_code`, `check_code`, `record_match`, `settle`, `approve`; no record contact at all flags `no_record_contact` (needs review). `send_code` and `check_code` refuse any status but pending and needs review (`AccountStatus::takes_identity_code`); the letter code is compared in constant time (`subtle`, D-1703).
- BIZ-020: `signup::review_due` (never told, or told 7 days ago or more).
- BIZ-022: `about_us::screen` and `screen_public_name`; word lists match whole words, `@`, `.com` and `#1` anywhere; spelled-out numbers count as a phone number (D-1703).
- BIZ-026, D-178: `number_is_old` stores only keyed SHA256 digests of the record phone, email and address; `resume_on_new_record` ends the wait only when a present record contact differs and sets `identity_step` to the step before the first changed one, so the next `send_code` goes to the changed contact, never the old one (D-1712). Known gap: `send_code`'s idempotency key is the business and step only, so a resumed send on a step already used reuses its key (D-1712).
- BIZ-027: `review_target` is calendar days (default 4) and gates nothing. BIZ-030: `fee_summary` takes every number from `FeeNumbers` (the Confirm window in minutes, shown as hours and any leftover minutes). BIZ-031: `trade_count` counts active businesses with a job type in the trade (waitlisted ones by `signup.trades`) plus the business itself; `claim_launch_notice` is true once (D-1709).
- CON-019, D-191, D-344: `consent::record_consent` refuses an unticked box; one row per channel with version and time (a repeat adds none); a text box needs a linked phone of the business; no call consent exists; `may_text(business, phone)` only after a current member ticked the text box for that phone (D-1717). CON-020: `request_welcome` once, active only; `may_get_cards` false until `welcome_sent`. BIZ-017: `phone_stopped` never changes the status. BIZ-032: `auto_reply_due` (not for expected texts, HELP, opt-outs or a text that is wholly a command word with at most one number; "Yes, why was I charged?" and an empty text get it, D-1717). BIZ-048: no recruiting text or call path; `recruiting_email` opens with `ADVERTISEMENT_LINE`; `may_recruit` honors a stop at once (D-1714).
- FEE-011: `signup::hand_entity_keys`. TERMS-016, TERMS-033: `signup::accept_terms`.

## Never break

- Every identity code goes to a record contact the caller read from `licensing`, never a typed one, and never by voice: `RecordContacts` has no typed field and `send_code` uses only `CodeChannel::Sms` and `Email` (`tests/integration/tests/identity_chain.rs::td_biz_01_code_and_match_activate_without_approval`).
- Activation goes only through `identity::settle` or `approve`, both via `service::apply` and `status::next`; the letter path is not a flag (D-281): `td_biz_12_letter_code_activates_with_no_flag`.
- No Stripe call inside the sign-up transaction (R11); a failed customer create returns `customer_ready: false` for a retry of `ensure_customer`, never a second business.
- `businesses.terms_acceptance` is append-only for `askhava_app`: `td_terms_08_business_terms_acceptance_is_stored`.
- A status moves only through `status::next`: `src/tests/account_status.rs::td_biz_02_each_status_reachable_only_by_its_rules`.
- Casey's approval alone never activates (BIZ-003): `src/tests/account_status.rs::td_biz_02_owner_approval_alone_never_activates`.
- Insurance No never pauses: `src/tests/account_status.rs::td_biz_02_ticks_insurance_and_pause_are_stored`.
- A paused or unticked business never counts at night: `src/tests/account_status.rs::night_count_counts_active_after_hours_businesses`.
- Own quiet times never widen the window, in code and in the table: `src/tests/own_quiet_times.rs::td_qh_03_own_quiet_times_only_narrow_the_window`, `td_qh_03_the_table_refuses_a_wider_window`.
- A business never resumes itself from a pause it did not choose, also after a rule paused it on top of its own pause: `src/tests/own_quiet_times.rs::td_biz_02_business_pauses_and_resumes_itself`, `td_biz_02_a_rule_pause_replaces_a_self_pause`.
- Every rule pause (complaints, license, no covered job type, Casey) must call `pause_for_rule`, not `apply`, or a self-paused business keeps its `self` reason (D-970). A business paused for two rules keeps only the first reason; ending it must re-check the others (D-198).
- A business row is never deleted by the app: `src/tests/account_status.rs::the_app_role_cannot_delete_a_business`.
- LIC-043 holds only as far as the caller's `allowed` set is right: `checklist` and `set_job_types` refuse anything outside it but cannot check the set against the license (D-954); every caller must build it from the license map plus the no-license job types.
- The owner always holds both roles and is never removed: `src/tests/team.rs::td_biz_34_the_owner_always_holds_both_roles`.
- Removing a member unlinks its phones and queues one end-sessions event: `src/tests/team.rs::td_biz_34_removal_unlinks_both_phones`, `tests/integration/tests/team_members.rs`.
- The sender directory follows every phone change at once, and a refused number leaves no row: `tests/integration/tests/sender_directory_sync.rs`.
- Every team and phone query names the business as well as the row ID, and every change but `add_owner` checks the `Acting` member: `src/tests/team.rs::biz_023_only_the_owner_manages_members`, `one_business_cannot_touch_anothers_team`, `call_012_who_may_change_or_unlink_a_phone`.
- Known gaps (D-963): a non-owner's phone works before the owner approves it (T148); a member who loses "Confirm and billing" keeps the phones it linked; audit rows name the business, not the acting member; no task sends BIZ-023's owner email yet.
- A team member row is never deleted by the app: `src/tests/team.rs::the_app_role_cannot_delete_a_team_member`.
- No contact value in any table, audit row or error (PRIV-001, PRIV-028); contacts live in `privacy`.
- The consent box is never pre-ticked and no call consent can be stored: `tests/integration/tests/business_consent.rs::td_con_08_box_starts_unticked_and_names_each_channel`, `td_con_08_consent_is_stored_with_version_and_time`.
- A business STOP never pauses the account: `td_con_08_business_stop_stops_texts_only`.
- Known gaps (D-1715): the card sender does not yet check `may_get_cards`, business text sends do not yet check `may_text`, and the welcome and STOP email handlers are not built.

## How to change

- Send the BIZ-020 status email: a worker task reads `api::review_due`, queues the email through `messaging` to the record contact and the sign-up mailbox, then calls `review_told` (D-1701).
- Build the sign-up route's code entry: the route MUST limit code attempts per business before calling `identity::check_code`, which keeps no attempt count; the six-digit letter code never changes (D-1703).
- Resume a waiting sign-up: on each daily list, for each `waiting_on_record` sign-up call `resume_on_new_record` with the new list's `RecordSeen` (its `RecordContacts` and the full record address); on `true` send `RECORD_UPDATED_TEMPLATE` to the sign-up email (and the new record email), never the code (D-1709).
- Wire T082 (D-1715): the form shows `consent_box` and posts to `record_consent`; call `request_welcome` after activation; the card sender checks `may_get_cards`; every business text checks `may_text` for the phone's `staff_phone` ID; the inbound route calls `phone_stopped` after an opt-out from a `sender_business` number and sends `AUTO_REPLY_TEMPLATE` when `auto_reply_due`; handle `WELCOME_KIND` and `PHONE_STOPPED_KIND` once the email request exists (D-1380). Change the consent wording: bump `CONSENT_VERSION`.
- Mail the identity letter: on a `Step::Letter`, the composition sends a Lob letter to the record address printing `letter_code` (D-1699).

- Add a status or a move: `AccountStatus` and `next` in `src/status.rs`, the migration check in a new migration, the oracle in `src/tests/account_status.rs`, BIZ-009 and D-953.
- Add a pause reason: `PauseReason` (code matches `^[a-z][a-z0-9_]{0,63}$`) and the oracle's events.
- Wire own quiet times into a notice sender: read `api::own_hours` and gate with `api::rules::may_notify` on the Arizona time, after the sender's QH-004 exceptions (D-968).
- Narrow the night count (T150): `models::night_count` and its test.
- Add a notice purpose or change who gets one: `NoticePurpose` and `Roles::fits` in `src/team.rs`, D-964, and `notice_purposes_fit_their_roles`.
- Implement `SenderLink` in the composition (when the portal pages land): copy the `Directory` impl in `tests/integration/tests/sender_directory_sync.rs` (D-963).
- After changing a query: `sqlx migrate run`, then `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `tests/integration/tests/signup_screens.rs`: TD-BIZ-07, TD-BIZ-10, TD-BIZ-27 and the sign-up-screen cases of TD-BIZ-11, TD-BIZ-12, TD-PICK-33; run `cargo test -p askhava-integration --test signup_screens`.
- `tests/integration/tests/business_consent.rs`: TD-CON-08, TD-BIZ-17, TD-TXT-13, TD-BIZ-35 and BIZ-032's case of TD-BIZ-12; run `cargo test -p askhava-integration --test business_consent`.
- `tests/integration/tests/identity_chain.rs`: TD-BIZ-01, TD-BIZ-05, TD-BIZ-12 (About us, letter, public name), TD-TERMS-08, the sign-up parts of TD-LIC-17 and TD-LIC-21, and FEE-011's hand-over; run `cargo test -p askhava-integration --test identity_chain`.

- `src/tests/account_status.rs`: TD-BIZ-02 (statuses, approval, checklist, towns, ticks, insurance, pause reason) and the BIZ-007 night count; run `cargo test -p askhava-businesses tests::account_status` with `DATABASE_URL` set.
- `src/tests/own_quiet_times.rs`: TD-QH-03 and T182's TD-BIZ-02 cases (self-pause and resume, own quiet times, no weekly card limit); run `cargo test -p askhava-businesses tests::own_quiet_times`.
- `src/tests/team.rs`: TD-BIZ-34 (roles, owner, audit rows, removal, who manages members and handles a phone, cross-business refusal, `notice_addresses`); run `cargo test -p askhava-businesses tests::team`.
- `tests/integration/tests/team_members.rs` and `sender_directory_sync.rs`: TD-BIZ-34 with the real sender directory; run `cargo test -p askhava-integration --test team_members --test sender_directory_sync`.
