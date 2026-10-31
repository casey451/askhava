# askhava-businesses: business accounts and the records matching reads

## Purpose

The layer-4 crate that holds each business account (`docs/architecture.md` section 3). T041 built the account status and its moves, the towns, the job-type checklist within the license, the overflow switch, the Spanish tick, the business-reported insurance answer and the after-hours tick with the night count (D-112). T220 added team members with their roles, the main and staff phones kept in `messaging`'s sender directory, and `notice_addresses`. T182 added the self-pause and the business's own quiet times. Sign-up and identity (T081) and the after-hours conditions (T150) come later.

## What it owns

- `src/api.rs`: the only public surface: `create`, `apply`, `pause_for_rule`, `pause_self`, `resume_self`, `set_ticks`, `set_towns`, `set_job_types`, `set_own_hours`, `own_hours`, `night_count`, `business`, `towns`, `job_types`, the types below, and `api::rules` (`next`, `checklist`, `towns`, `may_notify`, `may_resume_itself`, `APP_OPEN_MINUTE`, `APP_CLOSE_MINUTE`); from `team`: `Acting`, `add_owner`, `add_member`, `set_roles`, `remove_member`, `link_phone`, `change_phone`, `unlink_phone`, `members`, `notice_addresses`, `Roles`, `NoticePurpose`, `TeamMember`, `TeamError`, `SenderLink`, `MEMBER_REMOVED_KIND`.
- `src/status.rs`: `AccountStatus` (six statuses, `code`, `from_code`), `PauseReason`, `Refused`, `next`, `replaces_self_pause` (every match on a status lives here, SEC-031).
- `src/rules.rs` (pure): `Event`, `Checks`, `JobTypeChoice`, `checklist`, `towns`, `OwnHours`, `Widens`, `may_notify`, `may_resume_itself`.
- `src/models.rs`: `Business`, `Insurance`, `BusinessesError` and every query.
- `src/service.rs`: the transactions with their audit rows; `Ticks`; the own-hours and self-resume queries (T182).
- `src/team.rs`: team members, roles, staff phones and the notice-address lookup, with their own queries and `TeamError` (D-963, D-964).
- Migration `0055_businesses_records.sql`: the `businesses` schema, `businesses.business` (no `DELETE` for `askhava_app`), `businesses.town`, `businesses.job_type_choice`. No name, phone, email or address column.
- Migration `0056_businesses_team_members.sql`: `businesses.team_member` (owner row with `is_owner`, `mailbox_id` opaque email ID, role ticks, `removed_at`; no `DELETE` for `askhava_app`) and `businesses.staff_phone` (line ID, linking member, main mark; no number).
- Migration `0057_businesses_own_hours.sql`: `own_open_minute`, `own_close_minute` on `businesses.business` (Arizona minutes, both or neither, inside 480 to 1200 by a check).

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

## Never break

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

## How to change

- Add a status or a move: `AccountStatus` and `next` in `src/status.rs`, the migration check in a new migration, the oracle in `src/tests/account_status.rs`, BIZ-009 and D-953.
- Add a pause reason: `PauseReason` (code matches `^[a-z][a-z0-9_]{0,63}$`) and the oracle's events.
- Wire own quiet times into a notice sender: read `api::own_hours` and gate with `api::rules::may_notify` on the Arizona time, after the sender's QH-004 exceptions (D-968).
- Narrow the night count (T150): `models::night_count` and its test.
- Add a notice purpose or change who gets one: `NoticePurpose` and `Roles::fits` in `src/team.rs`, D-964, and `notice_purposes_fit_their_roles`.
- Implement `SenderLink` in the composition (when the portal pages land): copy the `Directory` impl in `tests/integration/tests/sender_directory_sync.rs` (D-963).
- After changing a query: `sqlx migrate run`, then `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `src/tests/account_status.rs`: TD-BIZ-02 (statuses, approval, checklist, towns, ticks, insurance, pause reason) and the BIZ-007 night count; run `cargo test -p askhava-businesses tests::account_status` with `DATABASE_URL` set.
- `src/tests/own_quiet_times.rs`: TD-QH-03 and T182's TD-BIZ-02 cases (self-pause and resume, own quiet times, no weekly card limit); run `cargo test -p askhava-businesses tests::own_quiet_times`.
- `src/tests/team.rs`: TD-BIZ-34 (roles, owner, audit rows, removal, who manages members and handles a phone, cross-business refusal, `notice_addresses`); run `cargo test -p askhava-businesses tests::team`.
- `tests/integration/tests/team_members.rs` and `sender_directory_sync.rs`: TD-BIZ-34 with the real sender directory; run `cargo test -p askhava-integration --test team_members --test sender_directory_sync`.
