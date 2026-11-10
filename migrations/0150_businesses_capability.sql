-- businesses (T155): the two capability answers matching reads beside the checklist (BIZ-049,
-- CARD-001; D-393, D-492). businesses.capability is one row per business: the largest job it
-- will quote in cents (NULL is "no limit") and its recurring-work answer. The checklist, towns and
-- after-hours tick keep their own tables and column (0055). No name, phone, email or address is
-- stored (PRIV-001). The table is new and empty; the app role cannot delete a row.

CREATE TABLE businesses.capability (
  business_id uuid PRIMARY KEY REFERENCES businesses.business (id),
  largest_job_cents bigint CHECK (largest_job_cents IS NULL OR largest_job_cents > 0),
  recurring boolean NOT NULL,
  changed_at timestamptz NOT NULL
);
GRANT SELECT, INSERT, UPDATE ON businesses.capability TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.capability FROM askhava_app;
