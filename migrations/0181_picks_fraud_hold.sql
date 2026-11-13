-- picks (T355): picks.fraud_hold (FEE-052, PICK-022, PRIV-005, D-2627). One row per pick whose
-- early fraud warning found no picks.release row under the pick's row lock; it commits before the
-- refund is asked for, and the release claim, under the same lock, refuses a pick that has one,
-- so a refunded charge is never also released. The pick by ID and the time only, no contact
-- value (PRIV-002). Append-only for askhava_app; the table is new and empty.

CREATE TABLE picks.fraud_hold (
  pick_id uuid PRIMARY KEY REFERENCES picks.pick (id),
  held_at timestamptz NOT NULL
);
GRANT SELECT, INSERT ON picks.fraud_hold TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.fraud_hold FROM askhava_app;
