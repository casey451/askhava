-- ops (T015): the audit log, alerts, pause switches and their history (JOB-005, OPS-003, OPS-010, OPS-011, PRIV-028, D-729).
-- ops.audit_log and ops.switch_history are append-only: the app role keeps SELECT and INSERT from the
-- default privileges below and loses UPDATE, DELETE and TRUNCATE right after each CREATE TABLE
-- (postgres brain P5, D-092). Every audit row holds record IDs and fixed names only, never a contact
-- value (PRIV-028): actor and subject are UUIDs, and every text column is a lower-case code name
-- (letters, digits, `_` and `.`, starting with a letter) that no phone, email or street address
-- matches. The id column is the export cursor `ops::api::export_audit` reads in order (OPS-010);
-- T017 copies each row off the database from it (OPS-011). All tables are new and empty.

CREATE SCHEMA IF NOT EXISTS ops;
GRANT USAGE ON SCHEMA ops TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA ops
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE ops.audit_log (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  at timestamptz NOT NULL DEFAULT now(),
  actor_kind text NOT NULL CHECK (actor_kind IN ('system', 'owner', 'business', 'homeowner')),
  actor_id uuid,
  action text NOT NULL CHECK (action ~ '^[a-z][a-z0-9_.]{0,63}$'),
  subject_kind text NOT NULL CHECK (subject_kind ~ '^[a-z][a-z0-9_.]{0,63}$'),
  subject_id uuid NOT NULL,
  from_status text CHECK (from_status ~ '^[a-z][a-z0-9_.]{0,63}$'),
  to_status text CHECK (to_status ~ '^[a-z][a-z0-9_.]{0,63}$'),
  field text CHECK (field ~ '^[a-z][a-z0-9_.]{0,63}$'),
  CHECK ((actor_kind = 'system') = (actor_id IS NULL)),
  CHECK ((from_status IS NULL) = (to_status IS NULL))
);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.audit_log FROM askhava_app;
CREATE INDEX audit_log_subject_idx ON ops.audit_log (subject_kind, subject_id, id);

CREATE TABLE ops.alert (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  raised_at timestamptz NOT NULL DEFAULT now(),
  kind text NOT NULL CHECK (kind ~ '^[a-z][a-z0-9_.]{0,63}$'),
  severity text NOT NULL CHECK (severity IN ('critical', 'high', 'normal')),
  subject_kind text CHECK (subject_kind ~ '^[a-z][a-z0-9_.]{0,63}$'),
  subject_id uuid,
  resolved_at timestamptz,
  CHECK ((subject_kind IS NULL) = (subject_id IS NULL))
);
CREATE INDEX alert_open_idx ON ops.alert (raised_at) WHERE resolved_at IS NULL;

-- A town or trade switch names its town or trade by record ID; the others have no scope.
CREATE TABLE ops.pause_switch (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  name text NOT NULL CHECK (name IN ('all', 'town', 'trade', 'charging', 'texting')),
  scope_id uuid,
  paused boolean NOT NULL,
  changed_at timestamptz NOT NULL DEFAULT now(),
  CHECK ((name IN ('town', 'trade')) = (scope_id IS NOT NULL)),
  UNIQUE NULLS NOT DISTINCT (name, scope_id)
);

CREATE TABLE ops.switch_history (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  at timestamptz NOT NULL DEFAULT now(),
  name text NOT NULL CHECK (name IN ('all', 'town', 'trade', 'charging', 'texting')),
  scope_id uuid,
  paused boolean NOT NULL,
  actor_kind text NOT NULL CHECK (actor_kind IN ('system', 'owner', 'business', 'homeowner')),
  actor_id uuid,
  CHECK ((actor_kind = 'system') = (actor_id IS NULL))
);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.switch_history FROM askhava_app;
