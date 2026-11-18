-- ops (T151): help requests, their messages and the HELP keyword times (OPS-036, CON-034,
-- PRIV-026, PRIV-030; D-348, D-3753, D-3881, D-4044). Written and read only through
-- app/ops/src/help_requests.rs. IDs, code names and times only: a message's words live in
-- privacy.message_body, and ops.help_message holds its inbox ID (body_ref), never text
-- (PRIV-001, D-482). ops.help_keyword keeps the time a contact last texted HELP, so the next
-- message is checked against CON-034 first. All three tables are new and empty; the app role keeps
-- the schema default SELECT and INSERT (0110) and gains UPDATE only where rows move (a request is
-- notified, closed or reopened; a keyword time is renewed). ops.help_message is append-only.

CREATE TABLE ops.help_request (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  asker text NOT NULL CHECK (asker IN ('homeowner', 'business', 'email')),
  asker_id uuid CHECK ((asker = 'email') = (asker_id IS NULL)),
  opened_at timestamptz NOT NULL,
  due_at timestamptz NOT NULL CHECK (due_at > opened_at),
  notified_at timestamptz,
  closed_at timestamptz
);
CREATE INDEX help_request_open_idx ON ops.help_request (asker_id, opened_at)
  WHERE closed_at IS NULL;
CREATE INDEX help_request_notified_idx ON ops.help_request (notified_at)
  WHERE notified_at IS NOT NULL;
GRANT UPDATE ON ops.help_request TO askhava_app;

CREATE TABLE ops.help_message (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  request_id uuid NOT NULL REFERENCES ops.help_request (id),
  body_ref uuid NOT NULL,
  from_owner boolean NOT NULL,
  at timestamptz NOT NULL
);
CREATE INDEX help_message_request_idx ON ops.help_message (request_id, at);
CREATE INDEX help_message_body_idx ON ops.help_message (body_ref);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.help_message FROM askhava_app;

CREATE TABLE ops.help_keyword (
  contact_id uuid PRIMARY KEY,
  at timestamptz NOT NULL
);
GRANT UPDATE ON ops.help_keyword TO askhava_app;
