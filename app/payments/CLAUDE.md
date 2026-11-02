# askhava-payments: Stripe and the money ledger

## Purpose

The layer-3 crate through which all money moves (FEE-032, `docs/architecture.md` section 3). T225 built its first part: the append-only ledger `payments.ledger_entry` and its one writer, `ledger_append` (D-1383, D-1384). Not here yet: Stripe customers and cards, the Confirm-and-pay charge, refunds, disputes and evidence, free first wins, the webhook and the nightly reconcile (T067).

## What it owns

- Migration `0090_payments_ledger.sql`: the `payments` schema and grants; `payments.ledger_entry` (append-only: `kind`, `business_id`, `job_id`, `amount_cents` never negative, `stripe_id`, unique `idempotency_key`, `seq`, `recorded_at`; `askhava_app` may only insert and select).
- `src/ledger.rs`: `EntryKind` (the eleven FEE-028 kinds; `ALL`, `code`, `is_zero`), `NewEntry` (raw fields), `LedgerEntry` (checked; `new`, `kind`, `amount_cents`), `LedgerError`, `Appended` (`New`, `Already`), `ledger_append`.
- `src/api.rs`: the public surface; everything above is re-exported there.

## Rules it enforces

- FEE-028, D-092: `payments.ledger_entry` is append-only for the app role (grants in the migration); `LedgerEntry::new` and the table's checks agree: $0 kinds (free first win, repeat customer FEE-013, fee-free job type FEE-045) are exactly 0 with no Stripe ID, every other kind is above 0; payout has no business or job; every kind but payout and credit has a job.
- FEE-028, FEE-050, FEE-051: the countering-fee, countering-fee-return and processing-fee kinds exist for the dispute and reconcile tasks.
- FEE-032: `ledger_append` is the only code that writes the ledger; other crates reach it through `api`.
- JOB-006, SEC-018: the caller's derived idempotency key (no personal data) makes a repeated task append once; a reused key with different content is `LedgerError::KeyReused`.

## Never break

- No `UPDATE`, `DELETE` or `TRUNCATE` on `payments.ledger_entry` for `askhava_app`; a wrong entry is corrected by a new entry (D-092): `src/tests/ledger_append.rs` `the_app_role_cannot_change_a_row`.
- `EntryKind::code` matches the migration's `kind` check exactly: `every_entry_type_appends`.
- Amounts are unsigned whole cents; direction comes from the kind (D-1383). Do not store negative amounts.
- `ledger_append` runs on the caller's connection, so the entry commits or rolls back with the caller's state change (rust brain R11); never call Stripe inside that transaction.

## How to change

- Add an entry kind: a spec change to FEE-028 in `docs/spec/fees.md` first; then a new forward migration replacing the `kind` check (and the amount/party checks if needed), a variant in `EntryKind` with its `code` and `ALL` entry, and run `cargo test -p askhava-payments tests::ledger_append`.
- Write a ledger row from a new task: build `NewEntry`, call `LedgerEntry::new`, then `api::ledger_append` inside your transaction with a key derived from IDs only.
- After any query change: apply migrations, regenerate `.sqlx/` and run `cargo sqlx prepare --workspace --check -- --all-targets --all-features`.

## Tests that guard it

- `src/tests/ledger_append.rs` (TD-FEE-16 ledger cases: every kind, the repeated key, the constructor's refusals, the app role's append-only grants): `cargo test -p askhava-payments tests::ledger_append`.
- TD-FEE-16's Stripe match and reconcile cases are T067's; its architecture case (only `payments` imports the payments adapter) is T175's.
