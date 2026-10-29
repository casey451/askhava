# Brain: PostgreSQL 16 operations for Ask Hava

> **Purpose:** the rules an AI session needs to write migrations, size pools, protect append-only records and plan recovery on managed PostgreSQL 16, each with its source.
> **Who reads it:** T012 (`core`, migrations, `core.task`), every task that writes a `migrations/0NNN_*.sql`, the `privacy`, `payments`, `ops` and `recovery` tasks, the account-checkpoint and Gate C work, and every PR reviewer touching SQL.
> **How to use it:** read it before writing SQL or a migration; cite the principle number (for example "P4") in the PR body; run the checklist in section 4 before hand-in. Items marked **verify with host** are claims to check in the host dashboard or docs at the account checkpoint, never assumptions to build on.
> **Last verified:** 2026-10-29 (sources fetched that day).
> **Never overrides** `docs/spec/` or `docs/architecture.md`. If this file disagrees with either, they win; report the disagreement in your PR and `docs/for-casey-review.md`.

Marks: **[inferred]** = reasoned practice no fetched source states in these words. "Arch" = `docs/architecture.md`.

## 1. Who this draws on

- **PostgreSQL documentation** (PostgreSQL Global Development Group) — privileges, default privileges, date/time, sequences, autovacuum, release notes: https://www.postgresql.org/docs/18/ddl-priv.html and others in section 5.
- **PostgreSQL wiki "Don't Do This"** (community-maintained list of PostgreSQL mistakes): https://wiki.postgresql.org/wiki/Don't_Do_This
- **Brandur Leach** (Heroku and Stripe engineer; wrote the River queue) — queue failure under long transactions: https://www.brandur.org/postgres-queues
- **Simeon Griggs, PlanetScale (2026)** — MVCC horizon and queue health: https://www.planetscale.com/blog/keeping-a-postgres-queue-healthy
- **Nikolay Samokhvalov** (founder of PostgresAI, co-host of Postgres.FM) — `lock_timeout` and retries for DDL: https://postgres.ai/blog/20210923-zero-downtime-postgres-schema-migrations-lock-timeout-and-retries
- **Andrew Kane, strong_migrations** (widely used catalogue of unsafe Postgres migrations): https://raw.githubusercontent.com/ankane/strong_migrations/master/README.md
- **Christophe Pettus** (PostgreSQL consultant, PGX) — always log lock waits and temp files: https://thebuild.com/blog/always-do-this-3-log-lock-waits-and-temp-files/
- **Brett Wooldridge, HikariCP "About Pool Sizing"** — small pools beat big ones: https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing
- **Andrew Kane, blind_index** — what a keyed-hash index leaks: https://github.com/ankane/blind_index
- **Railway and Render docs** — backups, PITR, connection limits: https://docs.railway.com/volumes/point-in-time-recovery , https://docs.railway.com/guides/postgres-backups-restores , https://render.com/docs/postgresql-backups , https://render.com/tutorials/postgres-on-render/connection-limits-and-pooling

## 2. Principles

**P1. Migrations are forward-only, numbered, and each names its module.** No down scripts in production; a mistake is fixed by a new migration. sqlx supports reversible pairs only when "you use the `-r` flag when creating the first migration" (sqlx-cli docs, https://docs.rs/crate/sqlx-cli); this repo does not. Every file opens with a `--` comment naming its module, what changes, why and the requirement IDs (`docs/code-docs.md` section 6). Serves Arch 2 (migrations), D-453, D-481.

**P2. Migrations change schema in lock-safe steps.** An `ALTER` waiting for a lock "is blocking even read-only transactions" queued behind it; protect with a low `lock_timeout` and retry (Samokhvalov). strong_migrations: build indexes `CONCURRENTLY`; add foreign keys and check constraints `NOT VALID` then `VALIDATE` in a later migration; never change a column type or rename in place (expand, backfill, contract); no volatile default on a new column on a large table. `CREATE INDEX CONCURRENTLY cannot run inside a transaction block` and a failed build leaves an `INVALID` index to drop (https://www.bytebase.com/blog/postgres-create-index-concurrently/). sqlx's `Migration` has a `no_tx` flag for this (source: https://pages.pvv.ntnu.no/Projects/mysqladm-rs/main/docs/src/sqlx_core/migrate/migration.rs.html); the file directive that sets it (believed to be a first line `-- no-transaction`) is **[inferred; check sqlx 0.9 source before use]**. At launch tables are small, so a plain `CREATE INDEX` in a new table's migration is fine [inferred]. Serves OPS-022 (release step must not stall live traffic), Arch 9.

**P3. Two roles; the app role never owns anything.** Roles are created outside migrations by the host or a bootstrap script (D-481). The migration role owns every schema and table; the app role (`askhava_app`) used by `web` and `worker` gets only `USAGE` and table privileges. Why: "The right to modify or destroy an object is inherent in being the object's owner, and cannot be granted or revoked in itself" (PostgreSQL privileges docs), so an app role that owned a table could always rewrite it. Serves D-092, threat T6, SEC-029.

**P4. Grants live in migrations, idempotent, with default privileges per schema.** `ALTER DEFAULT PRIVILEGES` sets privileges "applied to objects created in the future. (It does not affect privileges assigned to already-existing objects.)", and only for objects created by the target role: "new object permissions are only affected by the default privileges of the current role, and are not inherited" (docs). So: run migrations *as* the migration role (not via membership), use `FOR ROLE askhava_migrator IN SCHEMA <module>`, and also `GRANT` explicitly on tables that already exist. Serves D-481, D-092.

**P5. Append-only is a missing privilege, tested as the app role.** Append-only tables (`ops.audit_log`, `payments.ledger_entry`, `payments.charge_evidence`, the consent and opt-out event tables) get `SELECT, INSERT` only. `TRUNCATE` is its own privilege, separate from `DELETE` (docs), so never grant it to the app role. Default privileges grant `UPDATE, DELETE` broadly, so the creating migration must `REVOKE UPDATE, DELETE ON <table> FROM askhava_app` right after `CREATE TABLE` [inferred from P4]. Tests `SET ROLE askhava_app` and expect the `UPDATE` to fail (D-481). The off-database audit stream is the second copy (OPS-010, OPS-011). Serves D-092, OPS-010, FEE-029.

**P6. Lock down `public`.** PostgreSQL 15 removed `PUBLIC` create permission on the `public` schema for new databases, but "Upgrading a cluster or restoring a database dump will preserve `public`'s existing permissions" (PG 15 release notes). A bootstrap or first migration should `REVOKE CREATE ON SCHEMA public FROM PUBLIC` anyway; it is idempotent. Serves SEC-029 (one schema per module, D-091).

**P7. Every instant is `timestamptz`; zone logic lives in Rust.** "Don't use the `timestamp` type to store timestamps, use `timestamptz`" (wiki). PostgreSQL stores `timestamptz` "internally as UTC. The originally stated or assumed time zone is not retained and *cannot* be retrieved later" (date/time docs). So store instants only; compute Arizona business hours and recipient quiet hours in `core::clock` with jiff (Arch 5, D-003, D-004). Ranges use `>=` and `<`, never `BETWEEN` (wiki). Read and write through `jiff-sqlx` (D-481). Set the app role's `timezone` to `UTC` so ad hoc output is unambiguous [inferred]. Serves JOB-007, QH-*, D-004.

**P8. Size pools from the host's hard limit, counting deploy overlap.** Render caps connections by memory (100 under 8 GB, about 10 reserved; legacy plans 97), the cap is strict ("FATAL: sorry, too many clients already"), pools are per process, and "deploys … briefly double connection counts" (Render tutorial). Railway's default `max_connections` is not documented in any page fetched: **verify with host** (`SHOW max_connections;`). HikariCP: "You want a small pool, saturated with threads waiting for connections"; start near `(cores × 2) + spindles` and load-test. sqlx: "Be mindful of the connection limits for your database as well as other applications" (PoolOptions docs). Budget: `2 × (web_replicas × web_pool + worker_pool) + migration (1) + admin (2) ≤ max_connections − reserved` [inferred from the Render rule]. Make pool size a setting. Serves Arch 5 (pool sizing), OPS-007.

**P9. The queue table is hot: index it narrowly, keep transactions short, vacuum it harder.** PostgreSQL: skipping locked rows "provides an inconsistent view of the data, so this is not suitable for general purpose work, but can be used to avoid lock contention with multiple consumers accessing a queue-like table" (select.sgml text, commit https://forger.sitiv.fr/open-dsi/postgres/commit/df630b0dd5ea2de52972d456f5978a012436115e.diff). Dead tuples pile up when a long transaction holds back vacuum, and lock queries slow down scanning them (Brandur 2015); "A single transaction that takes 2 minutes to complete pins the horizon for the full 2 minutes" (PlanetScale). Autovacuum's defaults are threshold 50 and scale factor 0.2, overridable "for individual tables by changing table storage parameters" (autovacuum docs). So: a partial index `ON core.task (run_at) WHERE done_at IS NULL`, a unique index on `dedupe_key`, lower `autovacuum_vacuum_scale_factor` on `core.task`, delete finished rows in small batches if they are not kept [inferred]. Serves JOB-006, Arch 5 task queue.

**P10. Timeouts on the app role, set by the migration role.** `idle_in_transaction_session_timeout` exists so "idle sessions do not hold locks for an unreasonable amount of time", and an open transaction "prevents vacuuming away recently-dead tuples" (docs via https://postgresqlco.nf/doc/en/param/idle_in_transaction_session_timeout/); default 0 = off. strong_migrations suggests app `statement_timeout` about 15 s and `lock_timeout` about 10 s, migrations `lock_timeout` about 10 s and a long `statement_timeout`, set with `ALTER ROLE`. `transaction_timeout` arrived in PostgreSQL 17 (https://bytebase.com/blog/postgres-timeout), so it does not exist on PG 16. The exact numbers are settings to log in `docs/decisions.md` [inferred]. Serves JOB-004, OPS-007.

**P11. Sequences have gaps; nothing may depend on them being gapless.** "Transaction aborts or database crashes can result in gaps"; `INSERT … ON CONFLICT` consumes `nextval` before detecting the conflict; sequences "*cannot be used to obtain 'gapless' sequences*" (sequence docs). Use identity columns, not `serial` (wiki). Row IDs are UUIDs and replay uses derived IDs (OPS-016), so a gap in job numbers is never evidence of lost data [inferred]. Serves OPS-016, `jobs.job_number`.

**P12. Blind-index lookups are equality-only and leak equality.** The phone lookup is HMAC-SHA-256 under `VAULT_INDEX_KEY` (Arch 8). A blind index "leaks that rows have the same value", open to frequency analysis on low-entropy fields (blind_index README); a btree on the hash column is right, a range or `LIKE` query is meaningless. Never index plaintext, never put a plaintext phone in a `WHERE` [inferred]. Serves PRIV-002, PRIV-004.

**P13. Disk encryption at the host is not the vault.** Railway staff: "All customer data within Railway projects is encrypted at rest" (community answer, https://station.railway.com/questions/are-databases-encrypted-at-rest-0e719d6c); Render's security page claims AES-256 at rest (quoted in https://community.render.com/t/encryption-at-rest/3609). Neither is first-party documentation of backups: **verify with host** in its security page or DPA. Host encryption does not protect against a leaked database credential or a dump; field encryption under `VAULT_KEY` does (PRIV-002) [inferred]. Serves PRIV-002, OPS-042.

**P14. Point-in-time recovery is a host claim to verify, and restores land in a new database.** Railway PITR archives "every WAL segment" with pgBackRest, keeps about 4 weeks for Postgres, starts only from the first base backup after enabling (not retroactive), and "provisions a brand-new Postgres service alongside the source"; its page does not say which plans include it. Railway volume backups: daily kept 6 days, weekly 1 month, monthly 3 months; "wiping a volume deletes all of its backups" (Railway guides). Render PITR: paid plans only, Hobby 3 days, Pro 7 days, restores to a new instance, target at least 10 minutes back; logical backups kept 7 days (Render docs). Serves OPS-012 (Railway Pro only if the Backups tab shows PITR, else Render), OPS-013, OPS-014.

**P15. Log what finds trouble, never what holds personal data.** Turn on `log_lock_waits` and `log_temp_files = 0` (Pettus). Do not set `log_statement = 'all'`; bind parameters can carry ciphertext, blind-index hashes or scrubbed homeowner text, so if slow-statement logging is on, keep parameter logging off [inferred: check `log_parameter_max_length` in the PG 16 docs]. Managed hosts may not allow these settings: **verify with host**. Serves PRIV-016, SEC-048.

## 3. Shapes to copy

```sql
-- migrations/0012_core_task.sql
-- core (T012): task queue and timers. JOB-006, Arch 5. Grants follow P4/P5.
CREATE SCHEMA IF NOT EXISTS core AUTHORIZATION askhava_migrator;
GRANT USAGE ON SCHEMA core TO askhava_app;
ALTER DEFAULT PRIVILEGES FOR ROLE askhava_migrator IN SCHEMA core
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;
CREATE TABLE core.task ( /* id uuid, kind, payload jsonb, run_at timestamptz, ... */ );
CREATE INDEX task_due ON core.task (run_at) WHERE done_at IS NULL;
ALTER TABLE core.task SET (autovacuum_vacuum_scale_factor = 0.02);   -- value: log in decisions
-- an append-only table in its own module's migration:
-- CREATE TABLE ops.audit_log (...);  REVOKE UPDATE, DELETE, TRUNCATE ON ops.audit_log FROM askhava_app;
```
Role timeouts belong in the bootstrap script with the roles, since roles are outside migrations (D-481): `ALTER ROLE askhava_app SET idle_in_transaction_session_timeout = '30s';` [inferred value].

**Restore drill (OPS-013, OPS-014), monthly, in the test project with its fake data** (live data never enters test, Arch 9, SEC-003):
1. Restore the latest nightly export, or a PITR point, into a *new* database service (both hosts restore to a new service).
2. Run `server migrate` against it; it must be a no-op at the same release.
3. Check: table row counts against the off-database audit stream; the ledger reconcile; one known fake job readable end to end.
4. Switch the test `web` and `worker` to the restored database and pass `/healthz` (OPS-014).
5. Record time to restore and result in `recovery.restore_drill`; a failure raises the OPS-013 High alert.
A live restore follows the same steps inside the live project only, then the OPS-015 replay.

**Failure modes and what to look at**

| Symptom | Likely cause | Look at / fix |
|---|---|---|
| Requests hang, then many time out at once | A DDL waiting for a lock, everything queued behind it (P2) | `pg_locks` joined to `pg_stat_activity`; kill the DDL; rerun with `lock_timeout` |
| Queue claims slow down over hours | Dead tuples in `core.task`; a long or idle-in-transaction session pins the horizon (P9) | `pg_stat_activity` oldest `xact_start`; `n_dead_tup` in `pg_stat_user_tables`; terminate the session (Brandur) |
| "too many clients already" during deploy | Pools sized without deploy overlap (P8) | Recompute the P8 budget; lower per-process pool |
| Gaps in job numbers | Rollbacks or `ON CONFLICT` (P11) | Nothing to fix |
| Migration failed half-way on `CONCURRENTLY` | Index left `INVALID` (P2) | Drop it in a new migration and retry |
| App can `UPDATE` an append-only table | Default privileges re-granted it, or the app role owns the table (P3, P5) | Check `\dp`; revoke in a new migration; the append-only test should have failed |

## 4. Checklist

1. The migration is new, numbered, forward-only, and its first comment names module, purpose and requirement IDs (P1).
2. It touches only its own module's schema; no cross-schema foreign key (Arch 4 rule 3).
3. No blocking DDL on a populated table: `CONCURRENTLY`, `NOT VALID` then `VALIDATE`, expand/contract (P2).
4. Grants and default privileges are present and idempotent; nothing is owned by `askhava_app` (P3, P4).
5. Append-only tables revoke `UPDATE`, `DELETE`, `TRUNCATE` from the app role, and a `SET ROLE` test proves it (P5).
6. Every instant is `timestamptz`; no `BETWEEN` on times; no `serial` (P7, P11).
7. Queue and lookup queries are served by a narrow index (partial, unique dedupe, blind-index hash) (P9, P12).
8. No transaction stays open across a network call; claim transactions are short (P9, P10).
9. Pool size is a setting and fits the P8 budget for the chosen host.
10. Nothing assumes gapless sequences (P11).
11. No plaintext contact value in a column, index, `WHERE`, log or idempotency key (P12, P15, PRIV-002).
12. Host claims used by the change (PITR window, plan, connection cap, encryption, log settings) are marked **verify with host** or cite the checked dashboard (P13, P14).
13. `.sqlx/` regenerated and `cargo sqlx prepare --check` passes after `sqlx migrate run` (Arch 2).
14. `docs/architecture.md` and `docs/decisions.md` updated when a table, owner, timeout or setting changed.

## 5. Sources

- PostgreSQL privileges: https://www.postgresql.org/docs/18/ddl-priv.html
- ALTER DEFAULT PRIVILEGES: https://www.postgresql.org/docs/17/sql-alterdefaultprivileges.html
- PostgreSQL 15 release notes (public schema): https://www.postgresql.org/docs/18/release-15.html
- Date/time types: https://www.postgresql.org/docs/19/datatype-datetime.html (same text as 16 [inferred])
- Sequence functions: https://www.postgresql.org/docs/15/functions-sequence.html
- Autovacuum settings: https://www.postgresql.org/docs/16/runtime-config-autovacuum.html
- SKIP LOCKED doc text (commit): https://forger.sitiv.fr/open-dsi/postgres/commit/df630b0dd5ea2de52972d456f5978a012436115e.diff
- idle_in_transaction_session_timeout (doc text): https://postgresqlco.nf/doc/en/param/idle_in_transaction_session_timeout/
- Timeouts overview, transaction_timeout in PG 17: https://bytebase.com/blog/postgres-timeout
- CREATE INDEX CONCURRENTLY limits: https://www.bytebase.com/blog/postgres-create-index-concurrently/
- PostgreSQL wiki, Don't Do This: https://wiki.postgresql.org/wiki/Don't_Do_This
- Brandur Leach, Postgres Job Queues & Failure By MVCC: https://www.brandur.org/postgres-queues
- PlanetScale, Keeping a Postgres queue healthy: https://www.planetscale.com/blog/keeping-a-postgres-queue-healthy
- Samokhvalov, lock_timeout and retries: https://postgres.ai/blog/20210923-zero-downtime-postgres-schema-migrations-lock-timeout-and-retries
- strong_migrations README: https://raw.githubusercontent.com/ankane/strong_migrations/master/README.md
- Pettus, log lock waits and temp files: https://thebuild.com/blog/always-do-this-3-log-lock-waits-and-temp-files/
- HikariCP, About Pool Sizing: https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing
- sqlx PoolOptions: https://docs.rs/sqlx-core/0.8.6/sqlx_core/pool/struct.PoolOptions.html ; sqlx-cli: https://docs.rs/crate/sqlx-cli ; sqlx Migration source: https://pages.pvv.ntnu.no/Projects/mysqladm-rs/main/docs/src/sqlx_core/migrate/migration.rs.html
- blind_index README: https://github.com/ankane/blind_index
- Railway PITR: https://docs.railway.com/volumes/point-in-time-recovery ; backups: https://docs.railway.com/guides/postgres-backups-restores ; encryption answer: https://station.railway.com/questions/are-databases-encrypted-at-rest-0e719d6c
- Render backups and PITR: https://render.com/docs/postgresql-backups ; connection limits: https://render.com/tutorials/postgres-on-render/connection-limits-and-pooling ; encryption thread: https://community.render.com/t/encryption-at-rest/3609
- Continuous archiving explained (malisper): https://malisper.me/postgres-backups-with-continuous-archiving
