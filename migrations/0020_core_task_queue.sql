-- core (T013): the task queue and timers, and the rate-limit counters (JOB-006, SEC-014, SEC-040, D-090, D-491).
-- core.task holds every timer and every event a worker runs: kind, an IDs-only payload (never a contact
-- value, PRIV-002), when it is due, how many attempts it has had, the lease of the worker running it and a
-- unique dedupe key, so a timer survives a restart and two workers never run one task (app/core/src/tasks.rs).
-- core.rate_limit holds one counter per bucket and fixed window, raised by one atomic
-- INSERT ... ON CONFLICT ... DO UPDATE ... RETURNING (app/core/src/rate_limit.rs). A bucket is an IP or a
-- blind-index hash, never a plaintext phone or email (postgres brain P12). Both tables are new and empty,
-- so no blocking-DDL care is needed; the app role's rights come from 0019's default privileges.

CREATE TABLE core.task (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  kind text NOT NULL CHECK (kind <> ''),
  payload jsonb NOT NULL DEFAULT '{}'::jsonb,
  run_at timestamptz NOT NULL,
  attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  max_attempts integer NOT NULL DEFAULT 5 CHECK (max_attempts > 0),
  lease_owner uuid,
  lease_until timestamptz,
  last_error text,
  done_at timestamptz,
  dead_at timestamptz,
  dedupe_key text UNIQUE,
  CHECK ((lease_owner IS NULL) = (lease_until IS NULL))
);

-- The claim scans only open tasks in due order (postgres brain P9).
CREATE INDEX task_due_idx ON core.task (run_at) WHERE done_at IS NULL AND dead_at IS NULL;
-- The queue is hot: vacuum it at 2% dead rows, not autovacuum's default 20% (postgres brain P9).
ALTER TABLE core.task SET (autovacuum_vacuum_scale_factor = 0.02, autovacuum_analyze_scale_factor = 0.05);

CREATE TABLE core.rate_limit (
  bucket text NOT NULL CHECK (bucket <> ''),
  window_start timestamptz NOT NULL,
  hits integer NOT NULL CHECK (hits > 0),
  PRIMARY KEY (bucket, window_start)
);
