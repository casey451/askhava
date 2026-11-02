# askhava-payments: Stripe and the money ledger

## Purpose

The layer-3 crate through which all money moves (FEE-032, `docs/architecture.md` section 3). T225 built its first part: the append-only ledger `payments.ledger_entry` and its one writer, `ledger_append` (D-1383, D-1384). T058 added the introduction fee by job size of a chosen one-time quote, `fee_for` (D-1399, D-1400). Not here yet: the 4-month rule and its $75 cap (T188), the fee fixed at the pick and the fee-mode switch (T060), Stripe customers and cards, the Confirm-and-pay charge, refunds, disputes and evidence, free first wins, the webhook and the nightly reconcile (T067).

## What it owns

- Migration `0090_payments_ledger.sql`: the `payments` schema and grants; `payments.ledger_entry` (append-only: `kind`, `business_id`, `job_id`, `amount_cents` never negative, `stripe_id`, unique `idempotency_key`, `seq`, `recorded_at`; `askhava_app` may only insert and select).
- `src/ledger.rs`: `EntryKind` (the eleven FEE-028 kinds; `ALL`, `code`, `is_zero`), `NewEntry` (raw fields), `LedgerEntry` (checked; `new`, `kind`, `amount_cents`), `LedgerError`, `Appended` (`New`, `Already`), `ledger_append`.
- `src/rules.rs` (pure): `Tier` (`Small`, `Medium`, `Large`; `cents`, `for_basis`), `VisitTier` (`DEFAULT`, `from_dollars`, `tier`), `FeeQuote` (`Fixed`, `Range`, `Hourly`, `DiagnosticVisit`), `FeeJob`, `Fee` (`Charged`, `FeeFree`; `cents`, `basis`), `FeeError`, `fee_for`.
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
- FEE-044: `FeeQuote` has no setup amount, so a setup charge never reaches a basis; recurring quotes are T188's (D-1400).

## Never break

- No `UPDATE`, `DELETE` or `TRUNCATE` on `payments.ledger_entry` for `askhava_app`; a wrong entry is corrected by a new entry (D-092): `src/tests/ledger_append.rs` `the_app_role_cannot_change_a_row`.
- `EntryKind::code` matches the migration's `kind` check exactly: `every_entry_type_appends`.
- Amounts are unsigned whole cents; direction comes from the kind (D-1383). Do not store negative amounts.
- `LedgerError` carries no database message (D-698, D-1384); do not add a `sqlx::Error` source.
- `ledger_append` runs on the caller's connection, so the entry commits or rolls back with the caller's state change (rust brain R11); never call Stripe inside that transaction.
- The fee is always $25, $75 or $199 and never falls as the basis rises; a fee-free job type pays $0: `src/tests/prop_fee_props.rs`.
- Every one-time worked example matches to the cent and the recurring and refused rows are pinned: `src/tests/worked_examples.rs`.

## How to change

- Add an entry kind: a spec change to FEE-028 in `docs/spec/fees.md` first; then a new forward migration replacing the `kind` check (and the amount/party checks if needed), a variant in `EntryKind` with its `code` and `ALL` entry, and run `cargo test -p askhava-payments tests::ledger_append`.
- Write a ledger row from a new task: build `NewEntry`, call `LedgerEntry::new`, then `api::ledger_append` inside your transaction with a key derived from IDs only.
- After any query change: apply migrations, regenerate `.sqlx/` and run `cargo sqlx prepare --workspace --check -- --all-targets --all-features`.
- Change a fee tier or edge: `docs/spec/fees.md` FEE-001 and `docs/catalog/worked_examples.yaml` first, then `MEDIUM_FROM_CENTS`, `MEDIUM_UP_TO_CENTS` and `Tier::cents` in `src/rules.rs`; run `cargo test -p askhava-payments -- tests::fee_tiers tests::worked_examples tests::prop_fee_props`.
- Add a worked example: add the row to the YAML; a recurring or refused row also goes in the pinned list in `src/tests/worked_examples.rs`, and the checked count changes.

## Tests that guard it

- `src/tests/ledger_append.rs` (TD-FEE-16 ledger cases: every kind, the repeated key, the constructor's refusals, the table's own checks, the `Database` error branches, the app role's append-only grants with SQLSTATE 42501): `cargo test -p askhava-payments tests::ledger_append`.
- TD-FEE-16's Stripe match and reconcile cases are T067's; its architecture case (only `payments` imports the payments adapter) is T175's.
- `src/tests/fee_tiers.rs`, `src/tests/worked_examples.rs`, `src/tests/prop_fee_props.rs`: TD-FEE-01, TD-FEE-02, TD-FEE-27 (fee cases), TD-FEE-28, TD-LIC-17 (the $0 fee): `cargo test -p askhava-payments tests::fee_tiers`, `tests::worked_examples`, `tests::prop_fee_props`. The recurring cases of TD-FEE-02 and TD-FEE-27 (W18, W30, W37) are T188's (D-1400).
