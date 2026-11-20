# askhava-recovery: the nightly encrypted export and the backup checks

## Purpose
The layer-8 crate that keeps Ask Hava recoverable: the nightly `pg_dump -Fc` export, encrypted to an offline `age` recipient and stored under `exports/` (OPS-013, OPS-051, PRIV-018, PRIV-024, PRIV-029), each result recorded in `recovery.backup_check` and the `ops` audit log; with the monthly test restore into a throwaway database (T648) and the replay (`docs/architecture.md` sections 3 and 10).

## Never break
- No private key, identity or passphrase is ever a setting or sits on the host; the recipient is X25519 only and an export opens only with the test's own identity: `src/backup.rs::td_priv_09_the_recipient_is_x25519_only_and_no_private_key_is_a_setting`, `td_priv_14_an_export_is_unreadable_without_the_offline_key`.
- The export runs as `askhava_backup` (read-only, NOINHERIT, `pg_read_all_data` only, owns nothing, never granted to the app), its password only in `PGPASSWORD`, never on the command line or in a log: `td_ops_19_the_backup_role_dumps_what_the_app_role_cannot`.
- `pg_tool` takes only a PostgreSQL 16 binary found by name on `PATH`, never a newer major and never a fixed folder; its error names the tool and the `PATH` searched (D-4708): `the_path_helper_takes_only_postgresql_16_and_names_what_it_searched`.
- `recovery.backup_check` is append-only for the app role; each row commits with its audit entry.
- The replay never resends a text Twilio's sent log already holds, never matches by a stored key, and moves no money (D-2764, D-3889): `src/tests/replay_ids.rs::td_ops_04_a_text_sent_before_the_restore_is_found_and_not_resent`, `td_ops_04_stripe_is_retried_and_unmatched_charges_are_flagged`.
- The test restore reads the app database only through a `pg_dump`, writes only to a database it creates on the throwaway service and always drops, runs the role bootstrap before `pg_restore --single-transaction`, and fails on a foreign owner or an app-role `UPDATE`/`DELETE` grant on an append-only table: `tests/recovery/tests/export_restores.rs::td_ops_26_a_restore_needs_the_roles_first_and_checks_owners_and_grants`.
- A failed test restore raises one High alert that never starts safe mode: `src/restore.rs::td_ops_22_a_failed_restore_raises_one_high_alert_and_no_safe_mode`.
- The restore drill never reads storage and never takes an `age` identity (the operator hands it a plain dump, D-4932), and a vendor unreachable through every round fails the drill with no replay run written, never a guess (OPS-018): `src/tests/service.rs::td_ops_04_stripe_down_every_round_fails_the_drill_and_guesses_nothing`.
- `deny.toml`'s RUSTSEC-2026-0173 ignore holds only while `age` alone reaches `proc-macro-error2`: `tests/test_age_lock_guard.py` (D-4707).

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/recovery/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'OPS-013' app/recovery/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
