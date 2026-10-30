-- jobs (T014): quote-page link tokens, stored as hashes with their per-record salt (SEC-037, PICK-028, D-491).
-- A token is HMAC-SHA256 under ID_SECRET (version key_version) over the job ID and a 128-bit random
-- salt; only its SHA256 is stored, so the table never holds a usable link. One live token per job
-- (the partial unique index); re-issuing revokes the old row and adds a new one, so a revoked or
-- replaced link resolves to nothing. No contact value is stored (PRIV-002). The table is new and
-- empty; it takes the jobs schema's default grants from 0031.

CREATE TABLE jobs.quote_token (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid NOT NULL REFERENCES jobs.job (id),
  salt bytea NOT NULL CHECK (octet_length(salt) = 16),
  key_version integer NOT NULL CHECK (key_version > 0),
  token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
  issued_at timestamptz NOT NULL,
  revoked_at timestamptz,
  CHECK (revoked_at IS NULL OR revoked_at >= issued_at)
);
CREATE UNIQUE INDEX quote_token_live ON jobs.quote_token (job_id) WHERE revoked_at IS NULL;
