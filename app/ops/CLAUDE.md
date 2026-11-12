# askhava-ops: audit log, alerts and pause switches

## Purpose

The layer-1 crate that keeps Ask Hava's operating records: the append-only audit log every state change writes in its own transaction, the alerts table and the owner pause switches with their change history (`docs/architecture.md` section 3). It records rows holding IDs and code names only; it never sends, calls or reads anything outside the `ops` schema (D-491). T015 built the audit log, alerts and switches; of the switch rules it provides only the OPS-003 store (`set_switch`, `is_paused`): nothing reads a switch yet, and later tasks add the checks that honour them, safe mode, the status banner, alert acks and help requests.

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file.
- `src/api.rs`: the only public surface; also holds the off-database stream entry points (T017, D-734): `StreamEvent` (`Audit`, `Request`, `AiReading`), `stream_event` (enqueues an `ops.offdb_stream` `core.task`, `STREAM_KIND`), and the `audit` and `set_switch` wrappers over `service` that enqueue the audit row's copy in the same transaction, and `previous_audit_id` (the `max(id) < n` lookup the chain writer links to, D-735); `acknowledge_alert` and `ALERT_ACKNOWLEDGED_ACTION` (T080, OPS-028, D-1706); `audit_once` (T080, D-1705: writes an audit row only when none with its action and subject exists, under an advisory lock, in one transaction); `audited(action, subject)` (T336, D-3140: true when an audit row with that action and subject exists, the read side of an `audit_once` mark; the inbound retry's handled mark); `alert_open(kind, subject)` (T327, D-2850: true while an alert of that kind about that subject is unresolved, used by `payments` and `calls`); `raise_alert_once(kind, severity, subject)` (T327, D-2852: the open check and the raise in one transaction under an advisory lock on kind and subject, so checks at once raise one alert; the server's no-handler check).
- `src/models.rs`: `AuditEntry` (`status_change`, `check`), `Change`, `Actor`, `AuditRow` (`to_json`), `NewAlert`, `Severity`, `Switch`, `OpsError`, `is_code_name`.
- `src/service.rs`: `audit`, `export_audit` (cursor, `EXPORT_PAGE_MAX`), `raise_alert`, `is_paused`, `set_switch`.
- Migration `0021_ops_audit.sql`: the `ops` schema and its grants, `ops.audit_log` (append-only), `ops.alert`, `ops.pause_switch`, `ops.switch_history` (append-only).
- `src/status_banner.rs` (T279, D-2187): the WEB-007 banner: `BannerBy` (`System(kind, id)`, `Owner`), `show_banner`, `clear_banner`, `banner_shown` (T101's layout reads it). Migration `0170_ops_status_banner.sql`: `ops.status_banner` (append-only; `shown`, `actor_kind` system/owner, `actor_id`, `cause_kind`, `cause_id`, `changed_at`).
- `sqlx.toml`: `'TIMESTAMPTZ'` reads as `jiff_sqlx::Timestamp`.

## Rules it enforces

- JOB-005: `service::audit` takes the caller's connection, so a module calls it with `&mut *tx` in the transaction that changes status; `AuditEntry::status_change` is the row for a status move.
- OPS-010: `ops.audit_log` revokes `UPDATE`, `DELETE`, `TRUNCATE` from `askhava_app`; `service::export_audit` is a paged export by `id` cursor, `AuditRow::to_json` is the line format.
- OPS-011: every audit row is copied off the database through a per-row `core.task` inserted in its own transaction (`api::audit`, `api::set_switch` call `api::stream_event`), never through `export_audit`'s cursor, since identity IDs can commit out of order (D-729, D-734). `ops` only enqueues; `app/server/src/offdb_stream.rs` writes to storage. Call `api::audit`, never `service::audit` directly, or the row is not streamed.
- PRIV-029: a stream event holds IDs, code names, a route template, a status code and a time only; an AI reading's fields must each be a code name and value (`api::code_fields`), so no AI-written text enters the stream.
- PRIV-028: actors and subjects are UUIDs; every name is a `&'static str` written in code and checked by `models::is_code_name` and the table's `CHECK`s, so no phone, email or street address fits; a field edit logs the field name only (`Change::Field`).
- OPS-003, SEC-006: `service::set_switch` changes a switch only if it differs and, in one transaction, logs the change with its actor in the append-only `ops.switch_history` and as an `ops.audit_log` row (`switch_on`/`switch_off`, subject `ops.pause_switch` by its UUID); `service::is_paused` (a switch never set is off).

- OPS-028, D-1706: `acknowledge_alert` resolves only an open critical alert, with one `ops.alert_acknowledged` system audit row, in one transaction; a repeat writes nothing (`tests/integration/tests/voice_calls.rs::ops_028_*`). JOB-006, D-1705: `audit_once` writes one row per action and subject however often or concurrently it runs; `calls` uses it as the claim before a safety call (`voice_calls.rs::td_call_09_each_safety_call_is_claimed_once_*`).

- WEB-007, TXT-024, D-144 (T279): the banner is the latest `ops.status_banner` row (none: hidden); a change takes the banner's advisory lock and writes nothing when it is already in that state; the system turns it on for an emergency while texting is paused, and only Casey turns it off (the table refuses a hidden row by the system).

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

## How to change

- Audit a new action: build an `AuditEntry` with a code-name `action` and `subject_kind` (`<module>.<table>`), call `audit(&mut *tx, &entry)` in the change's transaction. Never put a value in a name.
- Add a switch: a `Switch` variant, its name in `Switch::columns`, and the name in both `CHECK (name IN …)` lists through a new migration; then the switch test.
- Add a column or table: a new migration `migrations/0NNN_<slug>.sql` starting `-- ops (T###)`; an append-only table revokes `UPDATE, DELETE, TRUNCATE` right after `CREATE TABLE`; regenerate `.sqlx/`.
- Run `cargo test -p askhava-integration --test audit_log`, `cargo clippy --workspace --all-targets -- -D warnings` and `bash scripts/smoke.sh`.

## Tests that guard it

- `tests/integration/tests/audit_log.rs` (TD-OPS-03 append-only and export cases, TD-PRIV-13 audit-row case; JOB-005, OPS-003, PRIV-028, SEC-006). `tests/integration/tests/offdb_stream.rs` (TD-OPS-03 stream cases, TD-OPS-19 chain and purge cases; OPS-010, OPS-011, PRIV-029).
