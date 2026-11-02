-- payments (T225): the payments schema and payments.ledger_entry, the append-only money ledger
-- (FEE-028, FEE-032, D-092, D-491). One row per money event, written only by
-- payments::api::ledger_append. `kind` is one of the eleven FEE-028 entry types; `amount_cents` is
-- never negative (the kind gives the direction) and is exactly 0 for the three $0 kinds (free first
-- win, repeat customer FEE-013, fee-free job type FEE-045) and above 0 for every other kind. A payout
-- belongs to no business and no job; every other entry names its business, and every kind but
-- payout and credit names its job. `stripe_id` is the Stripe object's ID (never on a $0 entry);
-- `idempotency_key` is the caller's derived key (no personal data, SEC-018), so a task run twice
-- appends once (JOB-006). jobs.job and businesses.business are other schemas, so no foreign key
-- (architecture section 4 rule 3). The table is new and empty. The app role may insert and select,
-- never update, delete or truncate (D-092).

CREATE SCHEMA IF NOT EXISTS payments;
GRANT USAGE ON SCHEMA payments TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA payments
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE payments.ledger_entry (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  recorded_at timestamptz NOT NULL DEFAULT now(),
  kind text NOT NULL CHECK (kind IN (
    'charge', 'refund', 'credit', 'free-first-win', 'repeat-customer', 'fee-free-job-type',
    'dispute', 'countering-fee', 'countering-fee-return', 'processing-fee', 'payout'
  )),
  business_id uuid,
  job_id uuid,
  amount_cents bigint NOT NULL,
  stripe_id text CHECK (stripe_id ~ '^[a-z]{2,8}_[A-Za-z0-9]{1,200}$'),
  idempotency_key text NOT NULL UNIQUE CHECK (idempotency_key ~ '^[A-Za-z0-9._:-]{1,200}$'),
  CONSTRAINT ledger_entry_amount_check CHECK (
    CASE WHEN kind IN ('free-first-win', 'repeat-customer', 'fee-free-job-type')
      THEN amount_cents = 0 AND stripe_id IS NULL
      ELSE amount_cents > 0
    END
  ),
  CONSTRAINT ledger_entry_business_check CHECK ((kind = 'payout') = (business_id IS NULL)),
  CONSTRAINT ledger_entry_job_check CHECK (kind IN ('payout', 'credit') OR job_id IS NOT NULL),
  CONSTRAINT ledger_entry_payout_job_check CHECK (kind <> 'payout' OR job_id IS NULL)
);
CREATE INDEX ledger_entry_business_idx ON payments.ledger_entry (business_id, seq);
CREATE INDEX ledger_entry_job_idx ON payments.ledger_entry (job_id) WHERE job_id IS NOT NULL;

GRANT SELECT, INSERT ON payments.ledger_entry TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON payments.ledger_entry FROM askhava_app;
