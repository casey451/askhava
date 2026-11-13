# askhava-ops: audit log, alerts and pause switches

## Purpose
The layer-1 crate that keeps Ask Hava's operating records: the append-only audit log every state change writes in its own transaction, the alerts table and the owner pause switches with their change history (`docs/architecture.md` section 3). It records rows holding IDs and code names only; it never sends, calls or reads anything outside the `ops` schema (D-491).

## Never break
- The app role cannot rewrite the audit log or the switch history: `tests/integration/tests/audit_log.rs::app_role_cannot_rewrite_the_audit_log_or_switch_history`.
- An audit row commits or rolls back with its change: `a_status_change_audit_row_shares_the_transaction`.
- No audit row holds a contact value; text columns are code names only (six of them): `an_audit_row_cannot_hold_a_contact_value`.
- The export returns every row exactly once in `id` order: `the_export_returns_every_row_once_in_order`.
- A switch change writes history and audit together, a repeat writes nothing: `a_pause_switch_logs_each_change_once`, `a_repeat_switch_change_writes_nothing`.
- `OpsError` never carries a value or the database's message (PRIV-016).
- `ops` depends on `core` only (for `enqueue`; layer 1, `kit/layers.json`); it sends nothing itself.
- A stream event commits or rolls back with its row: `tests/integration/tests/offdb_stream.rs::every_stream_event_reaches_storage_as_it_happens`.
- The banner clears only by Casey and its history cannot be rewritten: `tests/integration/tests/suspension_mode.rs::td_txt_33_a_restore_keeps_texting_off_and_only_casey_clears_the_banner`.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/ops/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/ops/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
