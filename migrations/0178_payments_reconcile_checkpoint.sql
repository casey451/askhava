-- payments (T334): payments.reconcile_checkpoint, the start of the last nightly reconcile that
-- finished (FEE-029, FEE-051, FEE-028; D-2304). Each run reads every Stripe charge created since the
-- earlier of now minus 7 days and this time minus 1 day, so a reconcile outage of any length skips
-- no charge; a gap of over 2 days raises one information-only High `payments.reconcile_gap` alert.
-- The row moves only when a run finishes, never backwards. One row (`id` is always true), a time
-- only: no homeowner or business detail. The table is new and empty; the schema default (0098)
-- gives the app role SELECT and INSERT, the run moves only `run_started_at`, so only it gets
-- UPDATE; DELETE and TRUNCATE are revoked by name (P5).

CREATE TABLE payments.reconcile_checkpoint (
  id boolean PRIMARY KEY DEFAULT true CHECK (id),
  run_started_at timestamptz NOT NULL
);

GRANT SELECT, INSERT ON payments.reconcile_checkpoint TO askhava_app;
GRANT UPDATE (run_started_at) ON payments.reconcile_checkpoint TO askhava_app;
REVOKE DELETE, TRUNCATE ON payments.reconcile_checkpoint FROM askhava_app;
