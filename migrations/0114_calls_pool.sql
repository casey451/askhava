-- calls (T076): the calls schema; calls.number_pool and calls.pool_alert (CALL-001, CALL-002,
-- CALL-017, CALL-021; D-077, D-160, D-231, D-1520, D-1679 to D-1681).
-- calls.number_pool: one row per connected job's masked line: job, homeowner and business by ID
-- (no foreign key across schemas, architecture section 4 rule 3), the pool number from settings it
-- was given (Ask Hava's own number, never a homeowner's or business's), whether it is shared with
-- another active job of the same homeowner or business (then calls route by PIN, CALL-004), when
-- it was given and, once the line closes (CALL-009, T079), when. One open line per job. `seq`
-- orders assignments, so "a free number was found after the last pool alert" needs no clock.
-- calls.pool_alert: each number-pool alert raised, with the `ops.alert` ID and the assignment that
-- raised it; append-only. An alert counts as open until a later assignment finds a free number
-- (D-1680), so at most one is open at a time (CALL-017, D-231). Both tables are new and empty.
--
-- Default privileges: SELECT and INSERT only on every calls table (D-1520, TD-SEC-34); the one
-- column a line's close changes gets its own UPDATE grant below. Nothing grants DELETE.

CREATE SCHEMA IF NOT EXISTS calls;
GRANT USAGE ON SCHEMA calls TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA calls GRANT SELECT, INSERT ON TABLES TO askhava_app;

CREATE TABLE calls.number_pool (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  job_id uuid NOT NULL,
  homeowner_id uuid NOT NULL,
  business_id uuid NOT NULL,
  pool_number text NOT NULL CHECK (pool_number ~ '^\+[0-9]{8,15}$'),
  shared boolean NOT NULL,
  assigned_at timestamptz NOT NULL,
  closed_at timestamptz,
  CHECK (closed_at IS NULL OR closed_at >= assigned_at)
);
CREATE UNIQUE INDEX number_pool_open_job_idx ON calls.number_pool (job_id) WHERE closed_at IS NULL;
CREATE INDEX number_pool_open_homeowner_idx ON calls.number_pool (homeowner_id)
  WHERE closed_at IS NULL;
CREATE INDEX number_pool_open_business_idx ON calls.number_pool (business_id)
  WHERE closed_at IS NULL;
CREATE INDEX number_pool_free_seq_idx ON calls.number_pool (seq) WHERE NOT shared;
GRANT UPDATE (closed_at) ON calls.number_pool TO askhava_app;

CREATE TABLE calls.pool_alert (
  alert_id uuid PRIMARY KEY,
  line_seq bigint NOT NULL UNIQUE,
  raised_at timestamptz NOT NULL
);
