-- payments (T245): narrow the payments schema's default privileges to SELECT, INSERT for the app
-- role (FEE-028, FEE-042, D-092, D-1495). 0090 gave askhava_app SELECT, INSERT, UPDATE and DELETE
-- on every future payments table, so a money table stayed append-only only while its own migration
-- remembered a REVOKE. From here a new payments table gets SELECT, INSERT only; any wider privilege
-- is an explicit GRANT in the migration that creates the table, which
-- tests/integration/tests/payments_grants.rs reads and checks (TD-SEC-33). Default privileges
-- apply to tables created later, so no existing table's grants change: fee_limit keeps UPDATE and
-- DELETE, pick_intent, charge_attempt and pay_timer keep UPDATE, payment_method keeps UPDATE of
-- removed_at. Revoking a default privilege that is already gone is a no-op, so this is idempotent.

ALTER DEFAULT PRIVILEGES IN SCHEMA payments
  REVOKE UPDATE, DELETE ON TABLES FROM askhava_app;
