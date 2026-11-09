-- messaging (T279): the stored texting mode of each audience's line: its texting switch, its
-- number's state with the suspension's start, and the launch channel (TXT-021, TXT-022, TXT-024,
-- TXT-035; D-144, D-297, D-303, D-1843). messaging.mode_state is append-only history: every change
-- by Casey (the owner-portal controls) inserts a whole new row for one audience, and the line's
-- state is its latest row; with no row the homeowner line is on and the business line off. The app
-- role keeps SELECT and INSERT only (postgres brain P5). No contact value is stored: the actor is a
-- UUID and the configured number comes from the settings, not from here (PRIV-001). The table is
-- new and empty, so nothing is rewritten.

CREATE TABLE messaging.mode_state (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  audience text NOT NULL CHECK (audience IN ('homeowner', 'business')),
  texting_on boolean NOT NULL,
  line_state text NOT NULL CHECK (line_state IN ('working', 'suspended', 'restored')),
  suspended_since timestamptz,
  launch_channel text NOT NULL CHECK (launch_channel IN ('sms', 'web_and_email')),
  actor_id uuid NOT NULL,
  changed_at timestamptz NOT NULL,
  CHECK ((line_state = 'working') = (suspended_since IS NULL))
);
CREATE INDEX mode_state_latest ON messaging.mode_state (audience, seq DESC);
GRANT SELECT, INSERT ON messaging.mode_state TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON messaging.mode_state FROM askhava_app;
