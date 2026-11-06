-- payments (T067): payments.reconcile_charge, the nightly reconcile's record of each unknown Stripe
-- charge it met (FEE-029, FEE-030, FEE-051, D-092, D-1495). An unknown charge is one with no ledger
-- `charge` entry when the reconcile first reads it; the row is written once per `ch_…` with the
-- pick its PaymentIntent or metadata names (null when none) and the refund reason when the
-- reconcile refunds it, so its alert is raised once. IDs and a code only, no personal data. The
-- table is new and empty; the schema default (0098) gives the app role SELECT and INSERT only, and
-- it is append-only like the ledger, so UPDATE, DELETE and TRUNCATE are revoked by name (P5).

CREATE TABLE payments.reconcile_charge (
  stripe_charge_id text PRIMARY KEY CHECK (stripe_charge_id ~ '^ch_[A-Za-z0-9]{1,200}$'),
  stripe_intent_id text NOT NULL CHECK (stripe_intent_id ~ '^pi_[A-Za-z0-9]{1,200}$'),
  pick_id uuid,
  refund_reason text CHECK (refund_reason IN ('no-pick', 'closed-job', 'second-charge')),
  seen_at timestamptz NOT NULL
);

GRANT SELECT, INSERT ON payments.reconcile_charge TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON payments.reconcile_charge FROM askhava_app;
