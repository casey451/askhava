-- payments (T288): payments.reconcile_open, the nightly reconcile's record of each Stripe charge it
-- read but could not settle (FEE-029, FEE-030, FEE-051; D-1872). A charge whose ledger write failed,
-- whose refund Stripe refused or left pending, whose ledger did not match Stripe, or which Stripe
-- could not be read for again, gets one row with the facts the run read (charge, PaymentIntent,
-- the pick its metadata names, the amounts and the balance transaction); every later run retries
-- each unsettled row, whatever its age, until it settles (`settled_at`). A row still open 7 days
-- after `first_seen` raises one High alert, marked by `alerted_at` in the same transaction. Stripe
-- IDs, amounts, a code and times only: no homeowner or business contact detail. The table is new
-- and empty; the schema default (0098) gives the app role SELECT and INSERT, and the run updates
-- only the try, alert and settle columns, so only those get UPDATE; DELETE and TRUNCATE are
-- revoked by name (P5).

CREATE TABLE payments.reconcile_open (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  stripe_charge_id text NOT NULL UNIQUE CHECK (stripe_charge_id ~ '^ch_[A-Za-z0-9]{1,200}$'),
  stripe_intent_id text NOT NULL CHECK (stripe_intent_id ~ '^pi_[A-Za-z0-9]{1,200}$'),
  pick_id uuid,
  amount_cents bigint NOT NULL CHECK (amount_cents > 0),
  refunded_cents bigint NOT NULL CHECK (refunded_cents >= 0 AND refunded_cents <= amount_cents),
  fee_cents bigint NOT NULL CHECK (fee_cents >= 0),
  balance_transaction text NOT NULL CHECK (balance_transaction ~ '^txn_[A-Za-z0-9]{1,200}$'),
  first_seen timestamptz NOT NULL,
  last_tried timestamptz NOT NULL,
  tries integer NOT NULL CHECK (tries >= 1),
  last_code text NOT NULL CHECK (last_code IN
    ('write-failed', 'refund-refused', 'refund-pending', 'mismatch', 'not-found',
     'stripe-unreadable')),
  alerted_at timestamptz,
  settled_at timestamptz
);

CREATE INDEX reconcile_open_unsettled_idx ON payments.reconcile_open (first_seen)
  WHERE settled_at IS NULL;

GRANT SELECT, INSERT ON payments.reconcile_open TO askhava_app;
GRANT UPDATE (last_tried, tries, last_code, alerted_at, settled_at)
  ON payments.reconcile_open TO askhava_app;
REVOKE DELETE, TRUNCATE ON payments.reconcile_open FROM askhava_app;
