-- portals (T086): Casey's owner sign-in: the one owner, his second factors, his offline recovery
-- codes, the owner session's idle and step-up times and the passkey ceremony state (SEC-005,
-- SEC-006, SEC-007, SEC-043, SEC-044, D-098, D-480, D-1917).
-- portals.owner is the single owner record: the argon2id password hash (PHC string) only. The
-- app role may read it and change the hash but never insert or delete a row, so no web route can
-- create an owner: only the bootstrap command, run as the migration role, inserts it (SEC-044).
-- portals.owner_factor is one registered second factor: a passkey (webauthn-rs's credential JSON:
-- public key, ID and counter, no private key) or a TOTP authenticator (its 20-byte secret and the
-- last 30-second step accepted, so a code is never accepted twice).
-- portals.owner_recovery_code holds the SHA256 of each offline one-time recovery code.
-- portals.owner_session extends an owner-realm session with its last request time (30 minutes
-- idle ends it) and its last second-factor time (5-minute step-up); it goes with its session.
-- portals.owner_challenge is one passkey ceremony's server state, used once, 5 minutes long.
-- All tables are new and empty.

CREATE TABLE portals.owner (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  singleton boolean NOT NULL DEFAULT true UNIQUE CHECK (singleton),
  password_hash text NOT NULL CHECK (password_hash LIKE '$argon2id$%'),
  created_at timestamptz NOT NULL
);

CREATE TABLE portals.owner_factor (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id uuid NOT NULL REFERENCES portals.owner (id),
  kind text NOT NULL CHECK (kind IN ('passkey', 'totp')),
  totp_secret bytea CHECK (length(totp_secret) = 20),
  totp_last_step bigint,
  passkey jsonb,
  created_at timestamptz NOT NULL,
  CHECK ((kind = 'totp') = (totp_secret IS NOT NULL)),
  CHECK ((kind = 'passkey') = (passkey IS NOT NULL))
);
CREATE INDEX owner_factor_owner_idx ON portals.owner_factor (owner_id);

CREATE TABLE portals.owner_recovery_code (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  owner_id uuid NOT NULL REFERENCES portals.owner (id),
  code_sha256 bytea NOT NULL UNIQUE CHECK (length(code_sha256) = 32),
  used_at timestamptz
);

CREATE TABLE portals.owner_session (
  session_id uuid PRIMARY KEY REFERENCES portals.session (id) ON DELETE CASCADE,
  last_seen_at timestamptz NOT NULL,
  factor_at timestamptz NOT NULL
);

CREATE TABLE portals.owner_challenge (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  purpose text NOT NULL CHECK (purpose IN ('sign_in', 'register')),
  state jsonb NOT NULL,
  expires_at timestamptz NOT NULL
);
CREATE INDEX owner_challenge_expires_idx ON portals.owner_challenge (expires_at);

-- The schema's default privileges grant the app role every table right; take back what no route
-- may do.
REVOKE INSERT, DELETE, TRUNCATE ON portals.owner FROM askhava_app;
REVOKE UPDATE ON portals.owner FROM askhava_app;
GRANT UPDATE (password_hash) ON portals.owner TO askhava_app;
-- Recovery codes are made only by the bootstrap (the migration role); the app role may only mark
-- one used. A TOTP secret never changes once stored: the app role may update only the last step
-- and a passkey's credential JSON (its counter).
REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON portals.owner_recovery_code FROM askhava_app;
GRANT UPDATE (used_at) ON portals.owner_recovery_code TO askhava_app;
REVOKE UPDATE ON portals.owner_factor FROM askhava_app;
GRANT UPDATE (totp_last_step, passkey) ON portals.owner_factor TO askhava_app;
REVOKE TRUNCATE ON portals.owner_factor, portals.owner_session, portals.owner_challenge
  FROM askhava_app;
