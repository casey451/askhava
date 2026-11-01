-- picks (T054): the picks schema; picks.pick, picks.code_attempt, picks.verify_cap_day and
-- picks.terms_line (PICK-013, PICK-015, PICK-016, PICK-031, PICK-032, PICK-055, CON-029).
-- picks.pick: one row per pick, job and business by ID (no foreign key across schemas, architecture
-- section 4 rule 3), the quote's page number, the channel (a text pick carries its inbound text),
-- the pick status (architecture section 6, FEE-052) and, once ended, why (replaced by another
-- "<n> YES", PICK-032; withdrawn by a CANCEL pause, CON-029). One pick waits for Confirm per job.
-- picks.code_attempt: one row per Verify code sent, by job and vault contact ID, never the number
-- (PRIV-002, PICK-055): channel, time, tries (at most 5), the last try and when it passed.
-- picks.verify_cap_day: the Arizona days the global Verify cap was hit, so one High alert each.
-- picks.terms_line: each queued text ending "By picking, you agree to the Terms" for a job, with
-- its template version (PICK-016, D-492); append-only. All tables are new and empty.

CREATE SCHEMA IF NOT EXISTS picks;
GRANT USAGE ON SCHEMA picks TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA picks
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE picks.pick (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid NOT NULL,
  business_id uuid NOT NULL,
  quote_number integer NOT NULL CHECK (quote_number > 0),
  channel text NOT NULL CHECK (channel IN ('text', 'page')),
  inbox_id uuid,
  status text NOT NULL CHECK (status IN ('awaiting_confirm', 'needs_card', 'charging',
    'auth_needed', 'payment_failed', 'paid', 'free', 'expired', 'cancelled', 'no_show',
    'fraud_refunded')),
  picked_at timestamptz NOT NULL,
  ended_at timestamptz,
  end_reason text CHECK (end_reason IN ('replaced', 'paused')),
  CHECK ((channel = 'text') = (inbox_id IS NOT NULL)),
  CHECK (ended_at IS NULL OR ended_at >= picked_at)
);
CREATE UNIQUE INDEX pick_waiting_idx ON picks.pick (job_id) WHERE status = 'awaiting_confirm';
CREATE INDEX pick_job_idx ON picks.pick (job_id);
GRANT SELECT, INSERT, UPDATE ON picks.pick TO askhava_app;
REVOKE DELETE, TRUNCATE ON picks.pick FROM askhava_app;

CREATE TABLE picks.code_attempt (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid NOT NULL,
  contact_id uuid NOT NULL,
  channel text NOT NULL CHECK (channel IN ('sms', 'call')),
  sent_at timestamptz NOT NULL,
  tries integer NOT NULL DEFAULT 0 CHECK (tries BETWEEN 0 AND 5),
  last_try_at timestamptz,
  verified_at timestamptz
);
CREATE INDEX code_attempt_contact_idx ON picks.code_attempt (contact_id, sent_at);
CREATE INDEX code_attempt_job_idx ON picks.code_attempt (job_id, sent_at);
CREATE INDEX code_attempt_sent_idx ON picks.code_attempt (sent_at);
GRANT SELECT, INSERT, UPDATE ON picks.code_attempt TO askhava_app;
REVOKE DELETE, TRUNCATE ON picks.code_attempt FROM askhava_app;

CREATE TABLE picks.verify_cap_day (
  day date PRIMARY KEY,
  hit_at timestamptz NOT NULL
);
GRANT SELECT, INSERT ON picks.verify_cap_day TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.verify_cap_day FROM askhava_app;

CREATE TABLE picks.terms_line (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid NOT NULL,
  outbox_id uuid NOT NULL,
  template_version text NOT NULL,
  queued_at timestamptz NOT NULL,
  UNIQUE (job_id, outbox_id)
);
GRANT SELECT, INSERT ON picks.terms_line TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.terms_line FROM askhava_app;
