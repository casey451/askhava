-- businesses (T196): verified notice addresses, the address-change step-up and the identity
-- episode (BIZ-040, BIZ-055, SEC-024, SEC-038, D-1712, D-1756). businesses.mailbox_check is one
-- row per business mailbox that must be verified before it gets anything: the sign-up email
-- (member_id null) or a team member's changed notice address (replaces is the old mailbox). The
-- 6-digit code is keyed from the row ID and sent time, never stored; tries counts wrong codes;
-- returned_at marks a hard bounce, a complaint or two soft bounces 14 days apart (soft_at is the
-- first). No address value is stored (PRIV-001). team_member.step_up_at is when the member last
-- passed a Verify code (SEC-038). On businesses.signup, identity_attempt is the identity episode,
-- raised only when a step's destination changed; sent_*_key are keyed digests of the destination
-- each step last went to; code_tries counts wrong identity codes in the episode.

CREATE TABLE businesses.mailbox_check (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  member_id uuid REFERENCES businesses.team_member (id),
  mailbox_id uuid NOT NULL,
  replaces uuid,
  sent_at timestamptz NOT NULL,
  tries integer NOT NULL DEFAULT 0 CHECK (tries >= 0),
  verified_at timestamptz,
  soft_at timestamptz,
  returned_at timestamptz,
  UNIQUE (business_id, mailbox_id)
);
GRANT SELECT, INSERT, UPDATE ON businesses.mailbox_check TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.mailbox_check FROM askhava_app;

ALTER TABLE businesses.team_member ADD COLUMN step_up_at timestamptz;

ALTER TABLE businesses.signup
  ADD COLUMN identity_attempt integer NOT NULL DEFAULT 0 CHECK (identity_attempt >= 0),
  ADD COLUMN sent_text_key text CHECK (sent_text_key ~ '^[0-9a-f]{64}$'),
  ADD COLUMN sent_mail_key text CHECK (sent_mail_key ~ '^[0-9a-f]{64}$'),
  ADD COLUMN sent_letter_key text CHECK (sent_letter_key ~ '^[0-9a-f]{64}$'),
  ADD COLUMN code_tries integer NOT NULL DEFAULT 0 CHECK (code_tries >= 0);
