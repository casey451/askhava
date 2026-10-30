-- jobs (T031): the job and its status history, the lifecycle state machine's tables (JOB-001, JOB-002, JOB-005, JOB-006).
-- jobs.job holds exactly one of the twelve statuses (JOB-001, the CHECK below) and a version bumped on
-- every move; each status's timer is a core.task row carrying the job ID and that version, so a timer
-- outlives a restart and does nothing once the job has moved on (JOB-006). jobs.status_history is
-- append-only: the app role loses UPDATE, DELETE and TRUNCATE right after CREATE TABLE (postgres brain
-- P5, D-092). No contact value is stored here (PRIV-002). Both tables are new and empty.

CREATE SCHEMA IF NOT EXISTS jobs;
GRANT USAGE ON SCHEMA jobs TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA jobs
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE jobs.job (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  status text NOT NULL CHECK (status IN ('intake', 'open', 'no_quotes_yet', 'quotes_ready',
    'awaiting_confirm', 'payment_pending', 'connected', 'done', 'closed', 'closed_early',
    'not_covered', 'cancelled')),
  version integer NOT NULL DEFAULT 0 CHECK (version >= 0),
  emergency boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL,
  status_at timestamptz NOT NULL
);

CREATE TABLE jobs.status_history (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  job_id uuid NOT NULL REFERENCES jobs.job (id),
  version integer NOT NULL,
  from_status text NOT NULL,
  to_status text NOT NULL,
  at timestamptz NOT NULL,
  UNIQUE (job_id, version)
);
REVOKE UPDATE, DELETE, TRUNCATE ON jobs.status_history FROM askhava_app;
