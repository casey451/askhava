-- jobs (T339): each job's catalog category code and remaining candidate job type codes, kept as
-- append-only narrowing rows for the introduction fee by job size at the pick (INT-018, FEE-005,
-- FEE-045; D-2210, D-2330, D-3157).
-- The first row is written at card send; a later row has the same category and a candidate set
-- that is a strict subset of the latest row's (checked by `jobs::api::record_job_facts` under the
-- job's row lock). The latest row (highest seq) is the job's current facts. Codes only: whether a
-- type is fee-free, its visit tier and whether it recurs are read from the catalog at the pick,
-- never stored here. No personal data. The table is new and empty.

CREATE TABLE jobs.job_facts (
  job_id uuid NOT NULL REFERENCES jobs.job (id),
  seq integer NOT NULL CHECK (seq >= 1),
  category text NOT NULL CHECK (char_length(category) BETWEEN 1 AND 100),
  job_types text[] NOT NULL CHECK (
    cardinality(job_types) >= 1 AND array_position(job_types, NULL) IS NULL
  ),
  set_at timestamptz NOT NULL,
  PRIMARY KEY (job_id, seq)
);

-- The schema default gives SELECT and INSERT only (0110); a row is never changed or deleted.
REVOKE UPDATE, DELETE, TRUNCATE ON jobs.job_facts FROM askhava_app;
