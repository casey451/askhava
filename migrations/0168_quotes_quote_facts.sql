-- quotes (T406): quotes.quote_facts, each quote's PICK-040 quote facts (PICK-040, PICK-051; D-233,
-- D-283, D-1049, D-3134).
-- One row per quote (its job and business, the key of quotes.quote): the business's own five
-- answers, as checked codes and a boolean (trip fee counts toward the repair, parts and materials,
-- permit, price may change, speaks Spanish), the job's type code the quote was for (for the form's
-- "Same as last time" pre-fill per job type; NULL while the job has no jobs.job_facts row) and
-- when the answers were recorded. The earliest visit stays on quotes.quote. Ask Hava never sets or
-- checks these answers. A quote edit replaces its row's answers (PICK-051); nothing is deleted. No
-- homeowner or contact detail. The table is new and empty.

CREATE TABLE quotes.quote_facts (
  job_id uuid NOT NULL,
  business_id uuid NOT NULL,
  trip_fee text NOT NULL CHECK (trip_fee IN ('counts', 'does_not_count', 'none')),
  parts text NOT NULL CHECK (parts IN ('included', 'extra', 'on_site')),
  permit text NOT NULL CHECK (permit IN ('included', 'not_included', 'not_needed')),
  may_change boolean NOT NULL,
  spanish text NOT NULL CHECK (spanish IN ('phone', 'on_site', 'both', 'no')),
  job_type text CHECK (char_length(job_type) BETWEEN 1 AND 100),
  recorded_at timestamptz NOT NULL,
  PRIMARY KEY (job_id, business_id),
  FOREIGN KEY (job_id, business_id) REFERENCES quotes.quote (job_id, business_id)
);

-- The "Same as last time" read: a business's latest answers on one job type.
CREATE INDEX quote_facts_last_idx ON quotes.quote_facts (business_id, job_type, recorded_at DESC);

-- The schema default gives SELECT and INSERT; an edit may update the answers, the type and the
-- time only; no row is deleted.
REVOKE UPDATE, DELETE, TRUNCATE ON quotes.quote_facts FROM askhava_app;
GRANT SELECT, INSERT ON quotes.quote_facts TO askhava_app;
GRANT UPDATE (trip_fee, parts, permit, may_change, spanish, job_type, recorded_at)
  ON quotes.quote_facts TO askhava_app;
