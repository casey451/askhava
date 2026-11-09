-- payments (T287): the fee terms in force and the introduction fee by job size fixed at each pick
-- (FEE-005, FEE-007, FEE-016, FEE-047; D-072, D-1415, D-1871).
-- payments.fee_terms is the one row of the fee mode in force and the card-authorization version a
-- business must have accepted before its next charge under it; `switch_fee_mode` updates it with
-- its audit row in one transaction, every reader uses `fee_terms_in_force`. It starts at tiers and
-- `AUTHORIZATION_VERSION` (1). payments.pick_fee is the fee fixed at the homeowner's pick, written
-- once in the pick's transaction and never changed: a press must match it before Stripe is called.
-- picks and jobs are other schemas, so no foreign key there (architecture section 4 rule 3). Both
-- tables are new.

CREATE TABLE payments.fee_terms (
  id boolean PRIMARY KEY DEFAULT true CHECK (id),
  mode text NOT NULL CHECK (mode IN ('tiers', 'flat')),
  authorization_version integer NOT NULL CHECK (authorization_version >= 1)
);
INSERT INTO payments.fee_terms (id, mode, authorization_version) VALUES (true, 'tiers', 1);

CREATE TABLE payments.pick_fee (
  pick_id uuid PRIMARY KEY,
  business_id uuid NOT NULL,
  job_id uuid NOT NULL,
  mode text NOT NULL CHECK (mode IN ('tiers', 'flat')),
  kind text NOT NULL CHECK (kind IN ('charged', 'fee_free')),
  amount_cents bigint NOT NULL CHECK (amount_cents >= 0),
  terms_version integer NOT NULL CHECK (terms_version >= 1),
  fixed_at timestamptz NOT NULL,
  CHECK ((kind = 'charged') = (amount_cents > 0))
);

-- The one terms row is only ever updated (FEE-007); the fixed fee only inserted (FEE-005).
REVOKE INSERT ON payments.fee_terms FROM askhava_app;
GRANT UPDATE ON payments.fee_terms TO askhava_app;
