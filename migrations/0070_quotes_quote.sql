-- quotes (T046): quotes.quote, one quote per business per job (PICK-002, PICK-051, D-1049).
-- It stores the format (PICK-053's code; an hourly quote with hours is a range, FEE-043), the
-- amount or the low and high amounts or up to 3 named tiers, a recurring price's period and any
-- one-time setup charge (FEE-044), any hourly rate with its estimated hours (FEE-043), the
-- "Range to be confirmed on site" tick, the notes, the original wording, the AI-read and
-- business-confirmed flags, the earliest visit and any edit or withdrawal time. There is no expiry
-- column: no quote expires on its own (PICK-051, TD-PICK-02). Every text field passed the PICK-007
-- bounce before it was stored, so no contact detail is here; no homeowner detail either
-- (PRIV-001). jobs.job and businesses.business are other schemas, so no foreign key
-- (architecture section 4 rule 3). The table is new and empty.

CREATE TABLE quotes.quote (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid NOT NULL,
  business_id uuid NOT NULL,
  format text NOT NULL
    CHECK (format IN ('range', 'monthly', 'tiers', 'firm', 'diagnostic_visit')),
  amount_cents bigint CHECK (amount_cents > 0),
  low_cents bigint CHECK (low_cents > 0),
  high_cents bigint CHECK (high_cents >= low_cents),
  tier_names text[],
  tier_cents bigint[],
  period text CHECK (period IN ('visit', 'week', 'month', 'quarter', 'year')),
  setup_cents bigint CHECK (setup_cents > 0),
  hourly_rate_cents bigint CHECK (hourly_rate_cents > 0),
  hours_low integer CHECK (hours_low > 0),
  hours_high integer CHECK (hours_high >= hours_low),
  confirm_on_site boolean NOT NULL,
  notes text NOT NULL,
  original text NOT NULL,
  ai_read boolean NOT NULL,
  confirmed boolean NOT NULL,
  earliest_visit date,
  visit_part text CHECK (visit_part IN ('morning', 'afternoon', 'evening', 'tonight')),
  received_at timestamptz NOT NULL,
  edited_at timestamptz,
  withdrawn_at timestamptz,
  UNIQUE (job_id, business_id),
  CHECK (cardinality(tier_names) BETWEEN 1 AND 3 OR tier_names IS NULL),
  CHECK (cardinality(tier_names) = cardinality(tier_cents) OR tier_names IS NULL)
);
CREATE INDEX quote_business_idx ON quotes.quote (business_id);

-- A quote is edited (PICK-051) or confirmed (PICK-006) in place, never deleted: a withdrawal
-- sets withdrawn_at.
GRANT SELECT, INSERT, UPDATE ON quotes.quote TO askhava_app;
REVOKE DELETE, TRUNCATE ON quotes.quote FROM askhava_app;
