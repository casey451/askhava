# askhava-recovery: the nightly encrypted export and the backup checks

## Purpose
The layer-8 crate that keeps Ask Hava recoverable: the nightly `pg_dump -Fc` export, encrypted to an offline `age` recipient and stored under `exports/` (OPS-013, OPS-051, PRIV-018, PRIV-024, PRIV-029), each result recorded in `recovery.backup_check` and the `ops` audit log; later tasks add the monthly test restore (T648) and replay (`docs/architecture.md` sections 3 and 10).

## Never break
- No private key, identity or passphrase is ever a setting or sits on the host; the recipient is X25519 only and an export opens only with the test's own identity: `src/backup.rs::td_priv_09_the_recipient_is_x25519_only_and_no_private_key_is_a_setting`, `td_priv_14_an_export_is_unreadable_without_the_offline_key`.
- The export runs as `askhava_backup` (read-only, NOINHERIT, `pg_read_all_data` only, owns nothing, never granted to the app), its password only in `PGPASSWORD`, never on the command line or in a log: `td_ops_19_the_backup_role_dumps_what_the_app_role_cannot`.
- `pg_tool` takes only a PostgreSQL 16 binary found by name on `PATH`, never a newer major and never a fixed folder; its error names the tool and the `PATH` searched (D-4708): `the_path_helper_takes_only_postgresql_16_and_names_what_it_searched`.
- `recovery.backup_check` is append-only for the app role; each row commits with its audit entry.
- `deny.toml`'s RUSTSEC-2026-0173 ignore holds only while `age` alone reaches `proc-macro-error2`: `tests/test_age_lock_guard.py` (D-4707).

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/recovery/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'OPS-013' app/recovery/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
