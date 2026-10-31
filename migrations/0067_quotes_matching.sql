-- quotes (T044): the quotes schema and quotes.job_card, one row per business that received a
-- job's card (CARD-001 to CARD-003, LIC-035). The row holds the job and business IDs, whether the
-- card carries the handyman condition (LIC-014, LIC-019) and when it was sent; no homeowner detail
-- (PRIV-001, CARD-005). There is no cap on rows per job (CARD-003). jobs.job and
-- businesses.business are other schemas, so no foreign key (architecture section 4 rule 3).
-- The table is new and empty.

CREATE SCHEMA IF NOT EXISTS quotes;
GRANT USAGE ON SCHEMA quotes TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA quotes
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE quotes.job_card (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid NOT NULL,
  business_id uuid NOT NULL,
  handyman boolean NOT NULL,
  sent_at timestamptz NOT NULL,
  UNIQUE (job_id, business_id)
);
CREATE INDEX job_card_business_idx ON quotes.job_card (business_id);

-- A card once sent is never deleted by the app.
GRANT SELECT, INSERT, UPDATE ON quotes.job_card TO askhava_app;
REVOKE DELETE, TRUNCATE ON quotes.job_card FROM askhava_app;
