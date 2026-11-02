-- payments (T062): the pick's one PaymentIntent, its charge attempts, the payment timers and the
-- monthly fee limit (FEE-016 to FEE-020, FEE-037, FEE-038, FEE-041, FEE-047, D-135, D-153, D-204).
-- payments.pick_intent is the one PaymentIntent a pick owns, created once under the key
-- pick-<id>-intent (FEE-047), with the fee, whether its job is an emergency (PICK-044) and the
-- moment payments ended the pick (Stripe down 4 hours, the card-fix window over, or the fee
-- limit). payments.charge_attempt is one row per Confirm press, try 1, 2, ... (FEE-016): its key
-- is derived from pick-<id>-try<n>, and the press ID makes a repeated press the same attempt. payments.pay_timer holds the card-fix window and
-- the 4-hour Stripe release: `due_at` while running, `left_seconds` while charges are paused
-- (FEE-038). picks and jobs are other schemas, so no foreign key there (architecture section 4
-- rule 3). The tables are new and empty.

CREATE TABLE payments.pick_intent (
  pick_id uuid PRIMARY KEY,
  business_id uuid NOT NULL,
  job_id uuid NOT NULL,
  amount_cents bigint NOT NULL CHECK (amount_cents > 0),
  stripe_intent_id text NOT NULL UNIQUE CHECK (stripe_intent_id LIKE 'pi\_%'),
  emergency boolean NOT NULL,
  created_at timestamptz NOT NULL,
  ended_at timestamptz
);

CREATE TABLE payments.charge_attempt (
  pick_id uuid NOT NULL REFERENCES payments.pick_intent (pick_id),
  try_no integer NOT NULL CHECK (try_no >= 1),
  press_id uuid NOT NULL,
  status text NOT NULL CHECK (status IN (
    'sent', 'processing', 'succeeded', 'declined', 'needs_action', 'unreachable'
  )),
  started_at timestamptz NOT NULL,
  decided_at timestamptz,
  PRIMARY KEY (pick_id, try_no),
  UNIQUE (pick_id, press_id)
);
CREATE INDEX charge_attempt_succeeded_idx ON payments.charge_attempt (decided_at)
  WHERE status = 'succeeded';

CREATE TABLE payments.pay_timer (
  pick_id uuid NOT NULL REFERENCES payments.pick_intent (pick_id),
  kind text NOT NULL CHECK (kind IN ('card-fix', 'stripe-down')),
  business_clock boolean NOT NULL,
  due_at timestamptz,
  left_seconds bigint CHECK (left_seconds >= 0),
  ended_at timestamptz,
  PRIMARY KEY (pick_id, kind),
  CHECK (ended_at IS NOT NULL OR (due_at IS NULL) <> (left_seconds IS NULL))
);

CREATE TABLE payments.fee_limit (
  business_id uuid PRIMARY KEY,
  limit_cents bigint NOT NULL CHECK (limit_cents > 0)
);

GRANT SELECT, INSERT, UPDATE ON payments.pick_intent, payments.charge_attempt, payments.pay_timer
  TO askhava_app;
REVOKE DELETE, TRUNCATE ON payments.pick_intent, payments.charge_attempt, payments.pay_timer
  FROM askhava_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON payments.fee_limit TO askhava_app;
