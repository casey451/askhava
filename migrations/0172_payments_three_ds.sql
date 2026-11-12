-- payments (T405): payments.charge_three_ds, whether each charged pick's payment was 3-D Secure
-- authenticated, read from Stripe's expanded latest_charge (FEE-050, FEE-027; D-2204, D-3118).
-- FEE-050's auto-submit condition reads it: only a stored true counts, no row is false. One row
-- per pick, written once; the pick ID and a boolean only, never card data, a cardholder name or a
-- homeowner field. The table is new and empty; the app role may only select and insert (P5).

CREATE TABLE payments.charge_three_ds (
  pick_id uuid PRIMARY KEY REFERENCES payments.charge_evidence (pick_id),
  authenticated boolean NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT now()
);

GRANT SELECT, INSERT ON payments.charge_three_ds TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON payments.charge_three_ds FROM askhava_app;
