-- core (T250): narrow every schema's default privileges to SELECT, INSERT for the app role
-- (OPS-010, CON-022, D-092, D-1520). Migrations 0019, 0021, 0027, 0031, 0036, 0040, 0042, 0048,
-- 0053, 0055, 0058, 0067 and 0079 each gave askhava_app SELECT, INSERT, UPDATE and DELETE on every
-- future table of their schema, so an append-only table stayed append-only only while its own
-- migration remembered a REVOKE (payments was narrowed by 0098, T245). From here a new table in any
-- schema gets SELECT, INSERT only; a wider privilege is an explicit GRANT in the migration that
-- creates the table, which tests/integration/tests/schema_grants.rs reads and checks (TD-SEC-34).
--
-- The statement names no schema: it walks the per-schema default ACLs the migrating role owns
-- (the defaults carry no FOR ROLE, 0019) and revokes UPDATE, DELETE wherever one gives them to
-- askhava_app. This keeps core's migration inside the module-boundary rule (TD-SEC-10, D-491,
-- D-1617): it is one role-wide policy, not an edit of another module's objects. Default privileges
-- apply only to tables created later, so no existing table's grants change. Revoking a default that
-- is already gone is a no-op, so a second run changes nothing. The block holds no GRANT keyword and
-- casts the role name, so a missing askhava_app stops it with an error naming the role
-- (tests/integration/tests/migrations.rs, D-491).
DO $narrow$
DECLARE
  nsp text;
BEGIN
  FOR nsp IN
    SELECT n.nspname::text
    FROM pg_default_acl d
    JOIN pg_namespace n ON n.oid = d.defaclnamespace
    CROSS JOIN LATERAL aclexplode(d.defaclacl) AS a(giver, holder, kind, can_pass)
    WHERE pg_get_userbyid(d.defaclrole) = current_user
      AND d.defaclobjtype = 'r'
      AND a.holder = 'askhava_app'::regrole
      AND a.kind IN ('UPDATE', 'DELETE')
    GROUP BY n.nspname
    ORDER BY n.nspname
  LOOP
    EXECUTE format(
      'ALTER DEFAULT PRIVILEGES IN SCHEMA %I REVOKE UPDATE, DELETE ON TABLES FROM askhava_app',
      nsp
    );
  END LOOP;
END
$narrow$;
