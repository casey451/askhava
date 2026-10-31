-- portals (T051): the portal sessions and the security event log (SEC-009, SEC-042, SEC-046,
-- SEC-048, D-099, D-480, D-943).
-- portals.session is one signed-in session in one realm (homeowner, business or owner): the
-- SHA-256 of its `__Host-askhava-session` cookie token (the token itself is never stored or
-- logged), the realm's actor ID (a vault contact, a business person or Casey's owner record, by
-- ID only, no cross-schema foreign key) and its expiry. A login deletes the session the browser
-- held before and makes a new row with a new token, so no cookie survives a login (SEC-042).
-- portals.security_event is one authentication failure, authorization denial, CSRF rejection or
-- webhook signature failure: its kind, the route template (never the path, which can hold a
-- token), the request ID and an actor ID, never a phone or a name (SEC-048). Events are counted
-- per kind over the last hour for the High alert, so the table is append-only for askhava_app
-- (postgres brain P5); raised_alert marks the event that raised one, so a kind raises at most one
-- alert per hour. Both tables are new and empty.

CREATE SCHEMA IF NOT EXISTS portals;
GRANT USAGE ON SCHEMA portals TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA portals
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE portals.session (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  token_sha256 bytea NOT NULL UNIQUE CHECK (length(token_sha256) = 32),
  realm text NOT NULL CHECK (realm IN ('homeowner', 'business', 'owner')),
  actor_id uuid NOT NULL,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  CHECK (expires_at > created_at)
);
CREATE INDEX session_expires_idx ON portals.session (expires_at);

CREATE TABLE portals.security_event (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  kind text NOT NULL
    CHECK (kind IN ('auth_failed', 'authz_denied', 'csrf_rejected', 'webhook_signature_failed')),
  route text NOT NULL CHECK (length(route) BETWEEN 1 AND 200 AND route !~ '[?#[:space:]]'),
  request_id uuid,
  actor_ref uuid,
  at timestamptz NOT NULL,
  raised_alert boolean NOT NULL DEFAULT false
);
CREATE INDEX security_event_kind_at_idx ON portals.security_event (kind, at);
REVOKE UPDATE, DELETE, TRUNCATE ON portals.security_event FROM askhava_app;
