-- core (T012): create the two database roles, once per cluster, before any migration (D-092, D-491).
--
-- askhava_migrator owns every schema and runs `server migrate`; askhava_app is what web and worker
-- connect as, and it owns nothing (postgres brain P3). Migrations never create roles; they only grant
-- to askhava_app, so a missing role fails the first migration with its name.
--
-- Run as a superuser (or the host's admin role) against the app database; safe to run again:
--   ASKHAVA_MIGRATOR_PASSWORD=... ASKHAVA_APP_PASSWORD=... \
--     psql "$ADMIN_URL" -q -v ON_ERROR_STOP=1 -f scripts/bootstrap_roles.sql
-- Passwords come only from the environment through \getenv, never from this file or a log line;
-- with none set a role keeps NOLOGIN. On a build or CI cluster add `-v test_cluster=1`, which lets the
-- migrator create sqlx test databases and SET ROLE askhava_app (D-491 deviation 3).
-- The passwords travel inside ALTER ROLE ... PASSWORD statements, so the server must not log them:
-- run this only where log_statement is 'none' (the PostgreSQL default; check with SHOW log_statement)
-- and log_min_duration_statement is -1, or the plain-text passwords land in the server log.
\set ON_ERROR_STOP on
\getenv migrator_password ASKHAVA_MIGRATOR_PASSWORD
\getenv app_password ASKHAVA_APP_PASSWORD

DO $roles$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'askhava_migrator') THEN
    CREATE ROLE askhava_migrator NOLOGIN NOINHERIT;
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'askhava_app') THEN
    CREATE ROLE askhava_app NOLOGIN;
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
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT USAGE, CREATE ON SCHEMA public TO askhava_migrator;

\if :{?test_cluster}
ALTER ROLE askhava_migrator CREATEDB;
GRANT askhava_app TO askhava_migrator WITH INHERIT FALSE, SET TRUE;
\endif
