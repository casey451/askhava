-- core (T012): create the database roles, once per cluster, before any migration (D-092, D-491).
-- recovery (T109) adds askhava_backup, the nightly export's read-only role (OPS-051, D-4293).
--
-- askhava_migrator owns every schema and runs `server migrate`; askhava_app is what web and worker
-- connect as, and it owns nothing (postgres brain P3). Migrations never create roles; they only grant
-- to askhava_app, so a missing role fails the first migration with its name.
--
-- Run as a superuser (or the host's admin role) against the app database; safe to run again:
--   ASKHAVA_MIGRATOR_PASSWORD=... ASKHAVA_APP_PASSWORD=... \
--     ASKHAVA_BACKUP_PASSWORD=... psql "$ADMIN_URL" -q -v ON_ERROR_STOP=1 -f scripts/bootstrap_roles.sql
-- Passwords come only from the environment through \getenv, never from this file or a log line;
-- with none set a role keeps NOLOGIN. On a build or CI cluster add `-v test_cluster=1`, which lets the
-- migrator create sqlx test databases and SET ROLE askhava_app (D-491 deviation 3).
-- The passwords travel inside ALTER ROLE ... PASSWORD statements, so the server must not log them:
-- run this only where log_statement is 'none' (the PostgreSQL default; check with SHOW log_statement)
-- and log_min_duration_statement is -1, or the plain-text passwords land in the server log.
\set ON_ERROR_STOP on
\getenv migrator_password ASKHAVA_MIGRATOR_PASSWORD
\getenv app_password ASKHAVA_APP_PASSWORD
\getenv backup_password ASKHAVA_BACKUP_PASSWORD

DO $roles$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'askhava_migrator') THEN
    CREATE ROLE askhava_migrator NOLOGIN NOINHERIT;
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'askhava_app') THEN
    CREATE ROLE askhava_app NOLOGIN;
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'askhava_backup') THEN
    CREATE ROLE askhava_backup NOLOGIN NOINHERIT;
  END IF;
END
$roles$;
-- A role made earlier by hand keeps its other attributes; the migrator never inherits the app's rights.
ALTER ROLE askhava_migrator NOINHERIT;

\if :{?migrator_password}
ALTER ROLE askhava_migrator LOGIN PASSWORD :'migrator_password';
\endif
\if :{?app_password}
ALTER ROLE askhava_app LOGIN PASSWORD :'app_password';
\endif
\if :{?backup_password}
ALTER ROLE askhava_backup LOGIN PASSWORD :'backup_password';
\endif

-- askhava_backup (T109, OPS-051, D-4293): reads every table and sequence for `pg_dump` through
-- PostgreSQL's built-in pg_read_all_data (the app role holds no sequence rights, so a dump as the
-- app fails), owns nothing, is never granted to the app, and inherits only that one grant. Its
-- read-only default and timeouts bound anything else that logs in as it.
ALTER ROLE askhava_backup NOINHERIT CONNECTION LIMIT 2;
GRANT pg_read_all_data TO askhava_backup WITH INHERIT TRUE;
ALTER ROLE askhava_backup SET default_transaction_read_only = on;
ALTER ROLE askhava_backup SET statement_timeout = '4h';
ALTER ROLE askhava_backup SET idle_in_transaction_session_timeout = '60s';

-- Timeouts (postgres brain P10; values in docs/decisions.md D-699): the app never holds a lock or an
-- open transaction for long; a migration gives up on a busy table instead of queueing behind it.
ALTER ROLE askhava_app SET statement_timeout = '15s';
ALTER ROLE askhava_app SET lock_timeout = '10s';
ALTER ROLE askhava_app SET idle_in_transaction_session_timeout = '30s';
ALTER ROLE askhava_app SET timezone = 'UTC';
ALTER ROLE askhava_migrator SET lock_timeout = '10s';
ALTER ROLE askhava_migrator SET idle_in_transaction_session_timeout = '60s';

-- The migrator creates the module schemas; both may connect. sqlx keeps _sqlx_migrations in public,
-- so the migrator needs CREATE there (D-491 deviation 4); nobody else creates anything in public.
SELECT format('GRANT CONNECT, CREATE ON DATABASE %I TO askhava_migrator', current_database()) \gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO askhava_app', current_database()) \gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO askhava_backup', current_database()) \gexec
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE, CREATE ON SCHEMA public TO askhava_migrator;

\if :{?test_cluster}
ALTER ROLE askhava_migrator CREATEDB;
GRANT askhava_app TO askhava_migrator WITH INHERIT FALSE, SET TRUE;
\endif
