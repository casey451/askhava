# askhava-ops: audit log, alerts and pause switches

## Purpose

The layer-1 crate that keeps Ask Hava's operating records: the append-only audit log every state change writes in its own transaction, the alerts table and the owner pause switches with their change history (`docs/architecture.md` section 3). It records rows holding IDs and code names only; it never sends, calls or reads anything outside the `ops` schema (D-491). T015 built the audit log, alerts and switches; later tasks add safe mode, the status banner, alert acks and help requests.

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the only public surface.
- `src/models.rs`: `AuditEntry` (`status_change`, `check`), `Change`, `Actor`, `AuditRow` (`to_json`), `NewAlert`, `Severity`, `Switch`, `OpsError`, `is_code_name`.
- `src/service.rs`: `audit`, `export_audit` (cursor, `EXPORT_PAGE_MAX`), `raise_alert`, `is_paused`, `set_switch`.
- Migration `0021_ops_audit.sql`: the `ops` schema and its grants, `ops.audit_log` (append-only), `ops.alert`, `ops.pause_switch`, `ops.switch_history` (append-only).
- `sqlx.toml`: `'TIMESTAMPTZ'` reads as `jiff_sqlx::Timestamp`.

## Rules it enforces

- JOB-005: `service::audit` takes the caller's connection, so a module calls it with `&mut *tx` in the transaction that changes status; `AuditEntry::status_change` is the row for a status move.
- OPS-010: `ops.audit_log` revokes `UPDATE`, `DELETE`, `TRUNCATE` from `askhava_app`; `service::export_audit` reads it by `id` cursor, `AuditRow::to_json` is the line format.
- OPS-011: the off-database copy is T017's; it reads the log through `export_audit`'s cursor (D-729).
- PRIV-028: actors and subjects are UUIDs; every name is a `&'static str` written in code and checked by `models::is_code_name` and the table's `CHECK`s, so no phone, email or street address fits; a field edit logs the field name only (`Change::Field`).
- OPS-003: `service::set_switch` changes a switch only if it differs and logs the change with its actor in the append-only `ops.switch_history`; `service::is_paused` (a switch never set is off).

## Never break

- The app role cannot rewrite the audit log or the switch history: `tests/integration/tests/audit_log.rs::app_role_cannot_rewrite_the_audit_log_or_switch_history`.
- An audit row commits or rolls back with its change: `a_status_change_audit_row_shares_the_transaction`.
- No audit row holds a contact value; text columns are code names only (six of them): `an_audit_row_cannot_hold_a_contact_value`.
- The export returns every row exactly once in `id` order: `the_export_returns_every_row_once_in_order`.
- `OpsError` never carries a value or the database's message (PRIV-016).
- `ops` depends on no first-party crate but `core` (layer 1, `kit/layers.json`); it sends nothing itself.

## How to change

- Audit a new action: build an `AuditEntry` with a code-name `action` and `subject_kind` (`<module>.<table>`), call `audit(&mut *tx, &entry)` in the change's transaction. Never put a value in a name.
- Add a switch: a `Switch` variant, its name in `Switch::columns`, and the name in both `CHECK (name IN …)` lists through a new migration; then the switch test.
- Add a column or table: a new migration `migrations/0NNN_<slug>.sql` starting `-- ops (T###)`; an append-only table revokes `UPDATE, DELETE, TRUNCATE` right after `CREATE TABLE`; regenerate `.sqlx/`.
- Run `cargo test -p askhava-integration --test audit_log`, `cargo clippy --workspace --all-targets -- -D warnings` and `bash scripts/smoke.sh`.

## Tests that guard it

- `tests/integration/tests/audit_log.rs` (TD-OPS-03 append-only and export cases, TD-PRIV-13 audit-row case; JOB-005, OPS-003, PRIV-028). T017 adds the off-database stream cases of TD-OPS-03.
