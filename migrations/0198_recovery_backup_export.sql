-- recovery (T109): the recovery schema and recovery.backup_check (OPS-013, OPS-051, PRIV-018,
-- PRIV-024, PRIV-029; D-492, D-1520, D-3883, D-4293).
-- recovery.backup_check: one row per nightly encrypted export (kind 'export', this task) or monthly
-- test restore (kind 'restore', T648), with whether it succeeded, the stored object's name, size
-- and SHA256 when one was written, a failure code name when it did not, and when it started and
-- finished. Each row is also one `ops` audit entry, which the owner health page reads through
-- `ops::api::latest_audit`. Append-only: a result is never edited. No contact value is stored.
--
-- Default privileges: SELECT and INSERT only on every recovery table (D-1520, TD-SEC-34);
-- UPDATE, DELETE and TRUNCATE are revoked here as well. The schema and table are new.

CREATE SCHEMA IF NOT EXISTS recovery;
GRANT USAGE ON SCHEMA recovery TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA recovery GRANT SELECT, INSERT ON TABLES TO askhava_app;

CREATE TABLE recovery.backup_check (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  kind text NOT NULL CHECK (kind IN ('export', 'restore')),
  ok boolean NOT NULL,
  object_name text CHECK (object_name ~ '^[a-z0-9_./-]{1,200}$'),
  object_bytes bigint CHECK (object_bytes >= 0),
  object_sha256 bytea CHECK (octet_length(object_sha256) = 32),
  failure text CHECK (failure ~ '^[a-z][a-z0-9_]{0,63}$'),
  started_at timestamptz NOT NULL,
  finished_at timestamptz NOT NULL,
  CHECK (ok = (failure IS NULL)),
  CHECK ((object_name IS NULL) = (object_bytes IS NULL)),
  CHECK ((object_name IS NULL) = (object_sha256 IS NULL)),
  CHECK (finished_at >= started_at)
);
REVOKE UPDATE, DELETE, TRUNCATE ON recovery.backup_check FROM askhava_app;
CREATE INDEX backup_check_kind_idx ON recovery.backup_check (kind, finished_at DESC);
