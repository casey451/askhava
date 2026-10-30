-- ai (T024): one row per AI call, holding the model ID, instruction version and provider request ID
-- beside the outcome, never any text (AI-015, AI-021, AI-023, AI-024, PRIV-016).
-- The daily cap counts today's rows in Arizona time (AI-021); a key, spend or cap alert is raised
-- once a day per kind, found by the partial index, and its `ops.alert` ID is kept (AI-023). Rows are append-only for the app role.
-- The table is new and empty.

CREATE SCHEMA IF NOT EXISTS ai;
GRANT USAGE ON SCHEMA ai TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA ai
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE ai.call_stat (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  at timestamptz NOT NULL DEFAULT now(),
  subject_id uuid NOT NULL,
  job text NOT NULL CHECK (job ~ '^[a-z][a-z0-9_]{0,63}$'),
  instruction_version text NOT NULL CHECK (instruction_version ~ '^[a-z][a-z0-9_.]{0,63}$'),
  model text NOT NULL CHECK (model ~ '^claude-[a-z0-9.-]{1,57}$'),
  request_id text CHECK (request_id ~ '^[A-Za-z0-9_-]{1,128}$'),
  outcome text NOT NULL CHECK (outcome IN ('ok', 'daily_cap', 'timed_out', 'connection',
    'http_error', 'malformed', 'refusal', 'max_tokens', 'other_stop', 'schema', 'catalog')),
  http_status integer CHECK (http_status BETWEEN 100 AND 599),
  attempts integer NOT NULL CHECK (attempts BETWEEN 0 AND 3),
  alert text CHECK (alert ~ '^ai\.[a-z_]{1,61}$'),
  alert_id uuid CHECK (alert_id IS NULL OR alert IS NOT NULL)
);
CREATE INDEX call_stat_at_idx ON ai.call_stat (at);
CREATE INDEX call_stat_alert_idx ON ai.call_stat (alert, at) WHERE alert IS NOT NULL;

GRANT SELECT, INSERT ON ai.call_stat TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON ai.call_stat FROM askhava_app;
