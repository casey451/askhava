-- messaging (T094): text retries, the email outbox and events, portal notices, the update channel
-- (TXT-014, TXT-015, TXT-018, OPS-038, SEC-028, SEC-037, SEC-050, SEC-051, INT-023, D-491).
-- messaging.text_retry keeps one row per failed outbox text: how many sends failed, when the first
-- failed, when the next retry is due, and how it ended (`emailed`, `call_back` with its due time,
-- or `failed`). The retry rules are pure (`retries::next_step`); the row only records them.
-- messaging.outbox gains the terminal status `failed`: a text given up after its 3 retries keeps no
-- body and is never claimed again (models::claim takes only `queued` or `sending`).
-- messaging.email_outbox holds one row per email, unique on its idempotency key (an HMAC over the
-- template ID, the recipient ID and the subject IDs, never an address, SEC-018). The address is
-- never stored: a homeowner's is read from the vault at send time, a business's comes from the
-- caller (PRIV-002, PRIV-004). `class` `bulk` marks the emails OPS-038 holds above the daily limit
-- (open cards, missed cards, the weekly digest). `not_before` and `waiting_for` are the notice
-- log's "waiting for the limit reset" (BIZ-045); the subject and both bodies are NULL once sent
-- (SEC-037). `sent_at` is when the provider took it, which starts any window counted from it.
-- messaging.portal_notice lists the important texts given up, for the portal to show (TXT-014).
-- messaging.update_channel holds a homeowner's TXT-018 choice; no row means text.
-- All tables are new and empty; the outbox's existing rows are untouched.

ALTER TABLE messaging.outbox DROP CONSTRAINT outbox_status_check;
ALTER TABLE messaging.outbox DROP CONSTRAINT outbox_body_check;
ALTER TABLE messaging.outbox
  ADD CONSTRAINT outbox_status_check
    CHECK (status IN ('queued', 'sending', 'sent', 'blocked', 'failed')),
  ADD CONSTRAINT outbox_body_check CHECK (status IN ('sent', 'blocked', 'failed') OR body IS NOT NULL);

CREATE TABLE messaging.text_retry (
  outbox_id uuid PRIMARY KEY REFERENCES messaging.outbox (id),
  failures integer NOT NULL CHECK (failures > 0),
  first_failed_at timestamptz NOT NULL,
  next_at timestamptz,
  outcome text CHECK (outcome IN ('emailed', 'call_back', 'failed')),
  call_back_due timestamptz CHECK ((outcome = 'call_back') = (call_back_due IS NOT NULL))
);

CREATE TABLE messaging.email_outbox (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  idempotency_key text NOT NULL UNIQUE CHECK (idempotency_key ~ '^[0-9a-f]{64}$'),
  template_id text NOT NULL CHECK (template_id ~ '^[a-z][a-z0-9_.]{0,63}$'),
  recipient_id uuid NOT NULL,
  recipient_kind text NOT NULL CHECK (recipient_kind IN ('homeowner', 'business')),
  subject_ids uuid[] NOT NULL,
  class text NOT NULL CHECK (class IN ('notice', 'bulk')),
  status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'sending', 'sent')),
  subject text,
  html text,
  plain text,
  attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  not_before timestamptz NOT NULL,
  waiting_for text CHECK (waiting_for IN ('daily_limit', 'provider_limit')),
  provider_id text CHECK (provider_id <> '' AND length(provider_id) <= 128),
  queued_at timestamptz NOT NULL,
  sent_at timestamptz,
  CHECK ((status = 'sent') = (sent_at IS NOT NULL AND provider_id IS NOT NULL)),
  CHECK (status = 'sent' OR (subject IS NOT NULL AND html IS NOT NULL AND plain IS NOT NULL)),
  CHECK (status <> 'sent' OR (subject IS NULL AND html IS NULL AND plain IS NULL))
);
CREATE INDEX email_outbox_due_idx ON messaging.email_outbox (not_before) WHERE status <> 'sent';
CREATE INDEX email_outbox_sent_idx ON messaging.email_outbox (sent_at) WHERE status = 'sent';

CREATE TABLE messaging.portal_notice (
  outbox_id uuid PRIMARY KEY REFERENCES messaging.outbox (id),
  recipient_id uuid NOT NULL,
  template_id text NOT NULL,
  subject_ids uuid[] NOT NULL,
  created_at timestamptz NOT NULL
);
CREATE INDEX portal_notice_recipient_idx ON messaging.portal_notice (recipient_id, created_at);

CREATE TABLE messaging.update_channel (
  recipient_id uuid PRIMARY KEY,
  channel text NOT NULL CHECK (channel IN ('text', 'email', 'both')),
  updated_at timestamptz NOT NULL
);

-- messaging.email_event stores each verified email-provider webhook once (its Svix message ID in
-- messaging.webhook_event under source `resend`, SEC-051) with the provider's event time; an
-- address's status is the latest event by that time, whatever order they arrived in.
ALTER TABLE messaging.webhook_event DROP CONSTRAINT webhook_event_source_check;
ALTER TABLE messaging.webhook_event ADD CONSTRAINT webhook_event_source_check
  CHECK (source IN ('twilio_inbound', 'twilio_status', 'resend'));

CREATE TABLE messaging.email_event (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  provider_id text NOT NULL CHECK (provider_id <> '' AND length(provider_id) <= 128),
  kind text NOT NULL CHECK (kind ~ '^email\.[a-z_]{1,32}$'),
  event_at timestamptz NOT NULL,
  received_at timestamptz NOT NULL
);
CREATE INDEX email_event_provider_idx ON messaging.email_event (provider_id, event_at);
CREATE INDEX email_event_received_idx ON messaging.email_event (received_at);
