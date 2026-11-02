# askhava-payments: Stripe and the money ledger

## Purpose

The layer-3 crate through which all money moves (FEE-032, `docs/architecture.md` section 3). T225 built its first part: the append-only ledger `payments.ledger_entry` and its one writer, `ledger_append` (D-1383, D-1384). T058 added the introduction fee by job size of a chosen one-time quote, `fee_for` (D-1399, D-1400). T188 added quotes with a period, `period_fee_for` (the 4-month rule and the recurring cap), and the early joiner's price lock (D-1404 to D-1407). T059 added Stripe customers made at sign-up with no card and saved cards with the FEE-015 authorization, cards only (D-1409, D-1410). T060 added the fee fixed at the pick, its ledger entry at Confirm and the fee-mode switch with its new authorization (D-1414, D-1415). Not here yet: the SetupIntent adapter call (T132), the Confirm-and-pay charge, refunds, disputes and evidence, free first wins, the webhook and the nightly reconcile (T067).

## What it owns

- Migration `0090_payments_ledger.sql`: the `payments` schema and grants; `payments.ledger_entry` (append-only: `kind`, `business_id`, `job_id`, `amount_cents` never negative, `stripe_id`, unique `idempotency_key`, `seq`, `recorded_at`; `askhava_app` may only insert and select).
- `src/ledger.rs`: `EntryKind` (the eleven FEE-028 kinds; `ALL`, `code`, `is_zero`), `NewEntry` (raw fields), `LedgerEntry` (checked; `new`, `kind`, `amount_cents`), `LedgerError`, `Appended` (`New`, `Already`), `ledger_append`.
- `src/rules.rs` (pure): `Tier` (`Small`, `Medium`, `Large`; `cents`, `for_basis`), `VisitTier` (`DEFAULT`, `from_dollars`, `tier`), `FeeQuote` (`Fixed`, `Range`, `Hourly`, `DiagnosticVisit`), `FeeJob`, `Fee` (`Charged`, `FeeFree`; `cents`, `basis`), `FeeError`, `fee_for`; T188: `RECURRING_MONTHS`, `Period`, `Service` (`of`), `RecurringCap` (`DEFAULT`, `from_dollars`), `PeriodJob`, `period_fee_for`, `LockDates`, `price_lock_until`, `locked_cents`; T060: `FeeMode` (`code`), `FeeTerms` (`new`, `mode`, `authorization_version`, `switched`), `flat_tier`, `PickedFee` (`fee`, `mode`, `cents`), `fee_at_pick`.
- Migration `0093_payments_customers_cards.sql`: `payments.customer` (key `business_id`, `stripe_customer_id`; insert and select only), `payments.card_authorization` (append-only: `version`, `text`, `accepted_at`), `payments.payment_method` (`pm_…`, `method_type` = `card`, `usage` = `off_session`, `brand`, `last4`, `authorization_id`, `saved_at`, `removed_at`).
- `src/models.rs` (T059): `AUTHORIZATION_TEXT`, `AUTHORIZATION_VERSION`, `CardError`, `BusinessId`, `CustomerId` (`parse`, `as_str`), `SavedCard` (`new`, `payment_method`, `brand`, `last4`), `SetupIntentRequest` (`for_customer`, `form`).
- `src/service.rs` (T059): `CUSTOMER_ACTION`, `customer_of`, `ensure_customer`, `Saved` (`New`, `Already`), `save_card`; T060: `FEE_MODE_ACTION`, `FEE_MODE_SUBJECT_KIND`, `FEE_MODE_ID`, `CONFIRM_FEE_ACTION`, `ConfirmError`, `switch_fee_mode`, `accept_authorization`, `may_confirm`, `Confirmed`, `record_fee_at_confirm`.
- `src/api.rs`: the public surface; everything above is re-exported there.

## Rules it enforces

- FEE-028, D-092: `payments.ledger_entry` is append-only for the app role (grants in the migration); `LedgerEntry::new` and the table's checks agree: $0 kinds (free first win, repeat customer FEE-013, fee-free job type FEE-045) are exactly 0 with no Stripe ID, every other kind is above 0; payout has no business or job; every kind but payout and credit has a job.
- FEE-028, FEE-050, FEE-051: the countering-fee, countering-fee-return and processing-fee kinds exist for the dispute and reconcile tasks.
- FEE-032: `ledger_append` is the only code that writes the ledger; other crates reach it through `api`.
- JOB-006, SEC-018: the caller's derived idempotency key (no personal data) makes a repeated task append once; a reused key with different content is `LedgerError::KeyReused`.
- FEE-001, FEE-002, FEE-008, D-009: `Tier::for_basis`: under $500.00 is $25, $500.00 to $1,500.00 inclusive is $75, over is $199; `Fee::cents` is exactly the tier, no tax, no percentage.
- FEE-003, FEE-043, D-033: `fee_for` prices a range, and an hourly quote with hours (rate times low to rate times high), at the midpoint rounded down to the cent.
- FEE-004, PICK-053, D-418: a diagnostic-visit quote, and an hourly quote with no hours, pays the category's `VisitTier` ($25 or $75 only) in a `diagnostic_visit` job; elsewhere `NotDiagnosticVisit` or `NeedsHours`.
- FEE-045, D-403: a fee-free job type gives `Fee::FeeFree`, $0, for every quote.
- FEE-044: no input has a setup amount, so a setup charge never reaches a basis (D-1400, D-1404).
- REC-002, REC-004, REC-005, REC-006, D-010: `period_fee_for` takes 4 months (`RECURRING_MONTHS`, never another number) of the monthly amount (`Period::per_month`; only the five periods, any other visit schedule comes in converted to `Monthly`, D-1407), a range at its exact midpoint, rounded down to the cent once at the end.
- REC-001, REC-010, D-037, D-159: `Service::of(recurring, one_time_job_type)`; a recurring job type with no period is `NeedsPeriod`, a period outside a recurring category `PeriodNotRecurring`, a period on an hourly or diagnostic-visit quote `NotRecurringPrice`; a one-time job with no period is `fee_for`'s face value.
- FEE-039, D-196, D-265: a recurring job type's tier is never above `RecurringCap` ($75 default, always a tier); a one-time job type is never capped, whatever period is typed.
- FEE-040, D-465: `price_lock_until` is none for a join 90 days or more after launch, else 6 calendar months (Arizona) from the latest of join, first trade on and activation; `locked_cents` charges at most the join-date amount strictly before that end.
- FEE-009: a Stripe customer is keyed by `BusinessId`; no `payments` table, type or function names a homeowner (`tests/security/tests/no_homeowner_charge.rs`).
- FEE-036, D-089: `ensure_customer` makes the customer at sign-up with no card, key derived from the business ID only (SEC-018), no transaction across the Stripe call.
- FEE-014, D-073: `SavedCard::new` refuses every method type but `card` (`CardsOnly`) and the table checks `method_type = 'card'`; only the `pm_…` ID, brand and last four are stored.
- FEE-015: `save_card` stores `AUTHORIZATION_TEXT` with `AUTHORIZATION_VERSION` and the acceptance time in the same transaction as the card; it charges nothing.
- FEE-005, D-009, D-1415: `fee_at_pick` takes the chosen quote, its period, the job and the fee mode only; no AI flag or other quote is an input, and only it makes a `PickedFee`.
- FEE-006, JOB-016, D-071: `record_fee_at_confirm` writes exactly `PickedFee::cents` at Confirm (a `charge` with its `ch_…` ID, or `fee-free-job-type` at $0), key from job and business IDs; nothing in `payments` takes a final bill or a check-in reply.
- FEE-007, D-072, D-1414: `switch_fee_mode` logs an `ops` audit row (owner actor, `tiers`/`flat`) and returns `FeeTerms` with the next authorization version; `may_confirm` refuses until the business accepted that version (`accept_authorization`); flat mode is `flat_tier`: the owner's edit, else the category's set tier; fee-free stays $0.
- FEE-048, D-490: `SetupIntentRequest::form` saves with `usage=off_session`, cards only; saved cards are stored with `usage = 'off_session'`.

## Never break

- No `UPDATE`, `DELETE` or `TRUNCATE` on `payments.ledger_entry` for `askhava_app`; a wrong entry is corrected by a new entry (D-092): `src/tests/ledger_append.rs` `the_app_role_cannot_change_a_row`.
- `EntryKind::code` matches the migration's `kind` check exactly: `every_entry_type_appends`.
- Amounts are unsigned whole cents; direction comes from the kind (D-1383). Do not store negative amounts.
- `LedgerError` carries no database message (D-698, D-1384); do not add a `sqlx::Error` source.
- `ledger_append` runs on the caller's connection, so the entry commits or rolls back with the caller's state change (rust brain R11); never call Stripe inside that transaction.
- The fee is always $25, $75 or $199 and never falls as the basis rises; a fee-free job type pays $0: `src/tests/prop_fee_props.rs`.
- Every one-time worked example matches to the cent and the recurring and refused rows are pinned: `src/tests/worked_examples.rs`; every recurring row (and W32) matches through `period_fee_for`, and W20 and W33 need the cap: `src/tests/recurring.rs`.
- Nothing in `models.rs` or `service.rs` creates a PaymentIntent or charge; saving a card charges nothing (FEE-036): `tests/integration/tests/setup_intent.rs` (its fake Stripe panics on any charge).
- `payments.card_authorization` is append-only for the app role, and `payments` code never names a homeowner (D-1409): `setup_intent.rs` `td_fee_10_authorizations_are_append_only`, `no_homeowner_charge.rs`.

- The fee charged at Confirm is the one fixed at the pick; a later quote edit, AI flag or PROBLEM reply changes no ledger row: `tests/integration/tests/fee_at_pick.rs`.
- No Confirm, and no fee entry, without the authorization version the fee terms need: `tests/integration/tests/fee_mode_switch.rs`.

## How to change

- Add an entry kind: a spec change to FEE-028 in `docs/spec/fees.md` first; then a new forward migration replacing the `kind` check (and the amount/party checks if needed), a variant in `EntryKind` with its `code` and `ALL` entry, and run `cargo test -p askhava-payments tests::ledger_append`.
- Write a ledger row from a new task: build `NewEntry`, call `LedgerEntry::new`, then `api::ledger_append` inside your transaction with a key derived from IDs only.
- After any query change: apply migrations, regenerate `.sqlx/` and run `cargo sqlx prepare --workspace --check -- --all-targets --all-features`.
- Change a fee tier or edge: `docs/spec/fees.md` FEE-001 and `docs/catalog/worked_examples.yaml` first, then `MEDIUM_FROM_CENTS`, `MEDIUM_UP_TO_CENTS` and `Tier::cents` in `src/rules.rs`; run `cargo test -p askhava-payments -- tests::fee_tiers tests::worked_examples tests::prop_fee_props`.
- Add a worked example: add the row to the YAML; a recurring or refused row also goes in the pinned list in `src/tests/worked_examples.rs`, and the checked count changes there (and, for a recurring row, in `src/tests/recurring.rs`).
- Change what flat mode charges: FEE-007 in `docs/spec/fees.md` and D-072 first, then `flat_tier` and `fee_at_pick` in `src/rules.rs`; run `cargo test -p askhava-payments tests::fee_at_pick` and `cargo test -p askhava-integration --test fee_mode_switch`.
- Change the FEE-015 wording: FEE-015 in `docs/spec/fees.md` first, then `AUTHORIZATION_TEXT` and a higher `AUTHORIZATION_VERSION` in `src/models.rs` (businesses accept again before the next charge, FEE-007); run `cargo test -p askhava-integration --test setup_intent`.

## Tests that guard it

- `src/tests/ledger_append.rs` (TD-FEE-16 ledger cases: every kind, the repeated key, the constructor's refusals, the table's own checks, the `Database` error branches, the app role's append-only grants with SQLSTATE 42501): `cargo test -p askhava-payments tests::ledger_append`.
- TD-FEE-16's Stripe match and reconcile cases are T067's; its architecture case (only `payments` imports the payments adapter) is T175's.
- `src/tests/fee_tiers.rs`, `src/tests/worked_examples.rs`, `src/tests/prop_fee_props.rs`: TD-FEE-01, TD-FEE-02, TD-FEE-27 (fee cases), TD-FEE-28, TD-LIC-17 (the $0 fee): `cargo test -p askhava-payments tests::fee_tiers`, `tests::worked_examples`, `tests::prop_fee_props`.
- `src/tests/recurring.rs`: TD-FEE-03, TD-FEE-21 (fee and lock cases), TD-FEE-24 (activation lock case), TD-FEE-35, TD-FEE-02 (W18, W30): `cargo test -p askhava-payments tests::recurring`. TD-FEE-21's monthly-limit and TD-FEE-24's refund and decline cases are T062's.
- `src/tests/prop_no_homeowner_charge.rs`, `tests/integration/tests/setup_intent.rs`, `tests/security/tests/no_homeowner_charge.rs`: TD-FEE-07, TD-FEE-10, TD-FEE-32 (saved-card case): `cargo test -p askhava-payments tests::prop_no_homeowner_charge`, `cargo test -p askhava-integration --test setup_intent`, `cargo test -p askhava-security --test no_homeowner_charge`. TD-FEE-10's Confirm-page cases are TD-FEE-40's; the stripe-mock SetupIntent round trip is T132's (D-1410).
- `src/tests/fee_at_pick.rs`, `tests/integration/tests/fee_at_pick.rs`, `tests/integration/tests/fee_mode_switch.rs`: TD-FEE-04, TD-FEE-05, TD-FEE-06: `cargo test -p askhava-payments tests::fee_at_pick`, `cargo test -p askhava-integration --test fee_at_pick`, `--test fee_mode_switch`.
