-- portals (T084): the business sign-in link and the business behind each business session
-- (SEC-004, SEC-019, SEC-026, D-1724).
-- portals.login_link is one emailed sign-in link for one team member of one business: the
-- SHA-256 of its 256-bit token (the token itself is never stored or logged), the member and
-- business IDs (no cross-schema foreign key), when it was made, when it stops working (15 minutes
-- later) and when it was used. A link is used once: the sign-in sets used_at only while it is NULL
-- and the link has not expired; removing the member sets used_at on every unused link (SEC-024).
-- portals.business_session names the business a business session's member signed in to, so a
-- route is scoped to that business; it goes with its session.
-- Both tables are new and empty.

CREATE TABLE portals.login_link (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  token_sha256 bytea NOT NULL UNIQUE CHECK (length(token_sha256) = 32),
  member_id uuid NOT NULL,
  business_id uuid NOT NULL,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  used_at timestamptz,
  CHECK (expires_at > created_at)
);
CREATE INDEX login_link_member_idx ON portals.login_link (member_id, created_at);

CREATE TABLE portals.business_session (
  session_id uuid PRIMARY KEY REFERENCES portals.session (id) ON DELETE CASCADE,
  business_id uuid NOT NULL
);

-- Only used_at changes after insert; nothing is deleted by the app.
GRANT SELECT, INSERT ON portals.login_link TO askhava_app;
GRANT UPDATE (used_at) ON portals.login_link TO askhava_app;
GRANT SELECT, INSERT ON portals.business_session TO askhava_app;
