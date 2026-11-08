-- businesses (T149): the sign-up inbox check with its sample card, the sample pick step and each
-- person's backup notice address (BIZ-042, BIZ-043, SEC-024, SEC-038, D-328, D-329, D-462).
-- businesses.email_check is one row per team member: when the sample card went, the answer to
-- "Did it arrive in your inbox?", when an OK reply came, when the sample pick step was first
-- offered (the first real job card the person opened) and the last Yes to "Did your phone alert
-- you?". None of it gates activation. businesses.backup_address is one row per backup request: the
-- member, the opaque mailbox ID, who asked, the send time and wrong tries for its keyed 6-digit
-- code (never stored), verified_at once the code was right and removed_at once replaced or
-- removed; at most one verified live row per member (a newer request removes older pending ones). No address value is stored (PRIV-001). Both tables
-- are new and empty.

CREATE TABLE businesses.email_check (
  member_id uuid PRIMARY KEY REFERENCES businesses.team_member (id),
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  sample_sent_at timestamptz,
  answer text CHECK (answer IN ('inbox', 'spam', 'tab', 'not_yet')),
  answered_at timestamptz,
  ok_reply_at timestamptz,
  pick_step_offered_at timestamptz,
  pick_alert_yes_at timestamptz
);
GRANT SELECT, INSERT, UPDATE ON businesses.email_check TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.email_check FROM askhava_app;

CREATE TABLE businesses.backup_address (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  member_id uuid NOT NULL REFERENCES businesses.team_member (id),
  mailbox_id uuid NOT NULL,
  requested_by uuid NOT NULL REFERENCES businesses.team_member (id),
  sent_at timestamptz NOT NULL,
  tries integer NOT NULL DEFAULT 0 CHECK (tries >= 0),
  verified_at timestamptz,
  removed_at timestamptz
);
CREATE UNIQUE INDEX backup_address_one_live
  ON businesses.backup_address (member_id) WHERE removed_at IS NULL AND verified_at IS NOT NULL;
CREATE INDEX backup_address_business ON businesses.backup_address (business_id, member_id)
  WHERE removed_at IS NULL;
GRANT SELECT, INSERT, UPDATE ON businesses.backup_address TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.backup_address FROM askhava_app;
