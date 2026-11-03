-- payments (T189): the FEE-042 evidence record of each successful charge, written at charge time
-- in the transaction that stores the succeeded attempt (FEE-042, FEE-015, D-365, D-1474). IDs and
-- timestamps only, never card data (FEE-014) or a homeowner field (PRIV-027): the pick, business
-- and job, the press and its attempt, the idempotency label the attempt's key was derived from,
-- the Stripe customer and PaymentIntent, the FEE-015 card-authorization version shown above the
-- button, the press time and the success time. One row per pick (a pick is paid once, FEE-016).
-- Append-only and kept as the ledger is: the app role may only insert and select. The table is
-- new and empty.

CREATE TABLE payments.charge_evidence (
  pick_id uuid PRIMARY KEY REFERENCES payments.pick_intent (pick_id),
  business_id uuid NOT NULL,
  job_id uuid NOT NULL,
  press_id uuid NOT NULL,
  try_no integer NOT NULL CHECK (try_no >= 1),
  idempotency_label text NOT NULL CHECK (idempotency_label ~ '^pick-[0-9a-f-]{36}-try[0-9]+$'),
  stripe_customer_id text NOT NULL CHECK (stripe_customer_id LIKE 'cus\_%'),
  stripe_intent_id text NOT NULL CHECK (stripe_intent_id LIKE 'pi\_%'),
  authorization_version integer NOT NULL CHECK (authorization_version > 0),
  amount_cents bigint NOT NULL CHECK (amount_cents > 0),
  pressed_at timestamptz NOT NULL,
  charged_at timestamptz NOT NULL,
  FOREIGN KEY (pick_id, try_no) REFERENCES payments.charge_attempt (pick_id, try_no)
);

GRANT SELECT, INSERT ON payments.charge_evidence TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON payments.charge_evidence FROM askhava_app;
