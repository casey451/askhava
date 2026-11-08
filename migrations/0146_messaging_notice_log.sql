-- messaging (T198): the per-job notice log a business sees on its job page (BIZ-045, OPS-037,
-- D-332, D-347, D-1992). One row per notice per address: the business and job it is about, the
-- notice kind, the opaque mailbox ID (never the address, PRIV-001), the email outbox row whose
-- provider events give the latest status, the send time or the quiet-hours hold, the first opening
-- of the page behind the notice's own link (the app's own page request, never a pixel or a click
-- redirect) and the quote or Confirm time. No homeowner detail is stored. Rows are never deleted,
-- kept as the message log's rows are (TXT-019, TXT-020). The table is new and empty.

CREATE TABLE messaging.notice_log (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL,
  job_id uuid NOT NULL,
  kind text NOT NULL
    CHECK (kind IN ('card', 'open_cards', 'pick', 'reminder', 'card_fix', 'release', 'account')),
  mailbox_id uuid NOT NULL,
  email_id uuid REFERENCES messaging.email_outbox (id),
  sent_at timestamptz,
  held_until timestamptz,
  first_opened_at timestamptz,
  acted_at timestamptz,
  CONSTRAINT notice_log_sent_or_held CHECK (sent_at IS NOT NULL OR held_until IS NOT NULL)
);
CREATE INDEX notice_log_job ON messaging.notice_log (business_id, job_id);

GRANT SELECT, INSERT, UPDATE ON messaging.notice_log TO askhava_app;
REVOKE DELETE, TRUNCATE ON messaging.notice_log FROM askhava_app;
