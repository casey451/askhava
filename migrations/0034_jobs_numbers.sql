-- jobs (T179): job numbers, never reused (JOB-024, D-851).
-- Every job gets its number in the transaction that creates it, from the one-row counter
-- jobs.job_number_next, and keeps it for ever: jobs.job_number is append-only for the app role
-- (postgres brain P5, D-092) and its primary key refuses a number twice, so inside one database a
-- number is never issued again, even for a cancelled job or after a lowered counter. A restore
-- rolls the counter back with everything else, so the replay (T110) raises it past the highest
-- number the vendors' sent-message records show before new jobs are taken
-- (jobs::api::raise_number_floor; the counter never moves down). Numbers start at 1001 so a texted
-- number is never a short count. jobs.job holds no live rows yet, so no backfill is needed.

CREATE TABLE jobs.job_number (
  number bigint PRIMARY KEY CHECK (number >= 1001),
  job_id uuid NOT NULL UNIQUE REFERENCES jobs.job (id),
  issued_at timestamptz NOT NULL
);
REVOKE UPDATE, DELETE, TRUNCATE ON jobs.job_number FROM askhava_app;

CREATE TABLE jobs.job_number_next (
  only_row boolean PRIMARY KEY DEFAULT true CHECK (only_row),
  next bigint NOT NULL CHECK (next >= 1001)
);
INSERT INTO jobs.job_number_next (next) VALUES (1001);
REVOKE INSERT, DELETE, TRUNCATE ON jobs.job_number_next FROM askhava_app;
