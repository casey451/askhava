-- privacy (T090): homeowner photos and their short-lived links (PRIV-013, PRIV-014, PRIV-023, PRIV-025, PRIV-028; D-454).
-- privacy.photo names each photo's job, the VAULT_KEY version it is sealed under and its object
-- name in storage; the photo itself is in object storage, re-encoded with no metadata and sealed
-- with AES-256-GCM whose associated data names the job and photo ID, never in PostgreSQL (PRIV-023).
-- privacy.photo_link holds the SHA-256 of each link token (the token is never stored or logged),
-- the photo it opens and when it stops working. Rows are only added here; the 30-day photo purge
-- (PRIV-007) is a later task's and grants its own DELETE. Both tables are new and empty, so no
-- statement blocks; the app role keeps the schema default, SELECT and INSERT (0110).

CREATE TABLE privacy.photo (
  id uuid PRIMARY KEY,
  job_id uuid NOT NULL,
  key_version integer NOT NULL CHECK (key_version > 0),
  storage_key text NOT NULL UNIQUE,
  stored_at timestamptz NOT NULL
);
CREATE INDEX photo_job_idx ON privacy.photo (job_id);

CREATE TABLE privacy.photo_link (
  token_hash bytea PRIMARY KEY CHECK (length(token_hash) = 32),
  photo_id uuid NOT NULL REFERENCES privacy.photo (id) ON DELETE CASCADE,
  issued_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  CHECK (expires_at > issued_at)
);
CREATE INDEX photo_link_photo_idx ON privacy.photo_link (photo_id);
