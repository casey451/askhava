# askhava-businesses: business accounts and the records matching reads

## Purpose

The layer-4 crate that holds each business account (`docs/architecture.md` section 3). T041 built the account status and its moves, the towns, the job-type checklist within the license, the overflow switch, the Spanish tick, the business-reported insurance answer and the after-hours tick with the night count (D-112). Sign-up and identity (T081), team members (T220), pause rules and quiet times (T182) and the after-hours conditions (T150) come later.

## What it owns

- `src/api.rs`: the only public surface: `create`, `apply`, `set_ticks`, `set_towns`, `set_job_types`, `night_count`, `business`, `towns`, `job_types`, the types below, and `api::rules` (`next`, `checklist`, `towns`).
- `src/status.rs`: `AccountStatus` (six statuses, `code`, `from_code`), `PauseReason`, `Refused`, `next` (every match on a status lives here, SEC-031).
- `src/rules.rs` (pure): `Event`, `Checks`, `JobTypeChoice`, `checklist`, `towns`.
- `src/models.rs`: `Business`, `Insurance`, `BusinessesError` and every query.
- `src/service.rs`: the transactions with their audit rows; `Ticks`.
- Migration `0055_businesses_records.sql`: the `businesses` schema, `businesses.business` (no `DELETE` for `askhava_app`), `businesses.town`, `businesses.job_type_choice`. No name, phone, email or address column.

## Rules it enforces

- BIZ-009, BIZ-003, BIZ-016, BIZ-021: `status::next` (pending to active only with the code, the match and no flag; a flag to needs review; needs review to active only on Casey's approval with the code and the match; waitlist to pending when a category opens; only active pauses, only paused resumes; removed is final; D-953). `service::apply` moves only if still in the status read (JOB-004) and writes the status audit row in the same transaction (JOB-005).
- BIZ-004, BIZ-049, LIC-043: `rules::checklist` and `service::set_job_types` (only job types in the caller's `allowed` set; D-954).
- BIZ-005, BIZ-006: `service::set_ticks` (`overflow_on`, `speaks_spanish`).
- BIZ-008: `Insurance` is stored as reported; no event of `next` reads it, so it never pauses or activates (D-061).
- BIZ-007: the `after_hours` column and `service::night_count` (active, ticked, in the town and category); its conditions are T150's (D-955, D-491).
- BIZ-010: `rules::towns` and `service::set_towns` (catalog town codes).

## Never break

- A status moves only through `status::next`: `src/tests/account_status.rs::td_biz_02_each_status_reachable_only_by_its_rules`.
- Casey's approval alone never activates (BIZ-003): `src/tests/account_status.rs::td_biz_02_owner_approval_alone_never_activates`.
- Insurance No never pauses: `src/tests/account_status.rs::td_biz_02_ticks_insurance_and_pause_are_stored`.
- A paused or unticked business never counts at night: `src/tests/account_status.rs::night_count_counts_active_after_hours_businesses`.
- A business row is never deleted by the app: `src/tests/account_status.rs::the_app_role_cannot_delete_a_business`.
- LIC-043 holds only as far as the caller's `allowed` set is right: `checklist` and `set_job_types` refuse anything outside it but cannot check the set against the license (D-954); every caller must build it from the license map plus the no-license job types.
- No contact value in any table, audit row or error (PRIV-001, PRIV-028); contacts live in `privacy`.

## How to change

- Add a status or a move: `AccountStatus` and `next` in `src/status.rs`, the migration check in a new migration, the oracle in `src/tests/account_status.rs`, BIZ-009 and D-953.
- Add a pause reason: `PauseReason` (code matches `^[a-z][a-z0-9_]{0,63}$`) and the oracle's events.
- Narrow the night count (T150): `models::night_count` and its test.
- After changing a query: `sqlx migrate run`, then `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `src/tests/account_status.rs`: TD-BIZ-02 (statuses, approval, checklist, towns, ticks, insurance, pause reason) and the BIZ-007 night count; run `cargo test -p askhava-businesses tests::account_status` with `DATABASE_URL` set.
