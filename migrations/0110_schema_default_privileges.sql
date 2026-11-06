-- core (T250): narrow every schema's default privileges to SELECT, INSERT for the app role
-- (OPS-010, CON-022, D-092, D-1520, D-1617). Migrations 0019 (core), 0021 (ops), 0027 (catalog),
-- 0031 (jobs), 0036 (privacy), 0040 (ai), 0042 (messaging), 0048 (licensing), 0053 (intake), 0055
-- (businesses), 0058 (portals), 0067 (quotes) and 0079 (picks) each gave askhava_app SELECT,
-- INSERT, UPDATE and DELETE on every future table of their schema, so an append-only table stayed
-- append-only only while its own migration remembered a REVOKE (payments is T245's, 0098). From
-- here a new table in any of these schemas gets SELECT, INSERT only; a wider privilege is an
-- explicit GRANT in the migration that creates the table, which
-- tests/integration/tests/schema_grants.rs reads and checks (TD-SEC-34).
--
-- Default privileges apply only to tables created later, so no existing table's grants change.
-- Revoking a default that is already gone is a no-op, so a second run changes nothing, and each
-- statement fails alone on a missing askhava_app (D-701). This file holds nothing but these
-- REVOKEs: that is the one shape in which a core migration may name other modules' schemas
-- (docs/architecture.md section 4 rule 3, TD-SEC-35, D-1632).
ALTER DEFAULT PRIVILEGES IN SCHEMA core REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA ops REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA catalog REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA jobs REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA privacy REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA ai REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA messaging REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA licensing REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA intake REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA businesses REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA portals REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA quotes REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA picks REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
