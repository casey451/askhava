-- core (T012): the core schema and what the app role may do in it (JOB-004, D-091, D-092, D-491).
-- The roles come from scripts/bootstrap_roles.sql (smoke.sh and Ci on build clusters), never from a
-- migration. A grant is never wrapped in IF EXISTS: a missing role stops the migration with an error
-- naming it (tests/integration/tests/migrations.rs). The default privileges carry no FOR ROLE, so
-- they cover tables created by the role running the migrations: askhava_ci in smoke and Ci,
-- askhava_migrator on Railway (D-491, D-492). Append-only tables revoke UPDATE, DELETE, TRUNCATE.
DO $guard$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'askhava_app') THEN
    RAISE EXCEPTION 'role askhava_app does not exist: run scripts/bootstrap_roles.sql first';
  END IF;
END
$guard$;

CREATE SCHEMA IF NOT EXISTS core;
GRANT USAGE ON SCHEMA core TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA core
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;
