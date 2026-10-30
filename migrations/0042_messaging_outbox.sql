-- messaging (T028): the text outbox and the sender directory (TXT-009, TXT-011, D-492).
-- messaging.outbox holds one row per text, unique on its idempotency key, an HMAC of the action and
-- the record IDs the text identifies, never a phone or a body word (SEC-018, PRIV-002); a resend
-- re-derives the same key from the stored IDs, so the carrier delivers it once (TXT-011). The
-- recipient is an opaque ID (a vault contact or a business address), never the number (TXT-019).
-- The body is kept only until the send is marked; a sent row's body is NULL, so a link token in it
-- does not outlive the send (SEC-037, D-850).
-- messaging.sender_directory holds each linked business main or staff phone as blind-index digests
-- only, one row per index key version, unique per digest so a number belongs to one business
-- (PRIV-002, the per-purpose `sender-dir` key of PRIV-024, D-492); `line_id` is the linked
-- phone's record ID, named so no column outside `privacy` reads as a contact field (TD-PRIV-01).
-- Both are new and empty.

CREATE SCHEMA IF NOT EXISTS messaging;
GRANT USAGE ON SCHEMA messaging TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA messaging
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE messaging.outbox (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  idempotency_key text NOT NULL UNIQUE CHECK (idempotency_key ~ '^[0-9a-f]{64}$'),
  template_id text NOT NULL CHECK (template_id ~ '^[a-z][a-z0-9_.]{0,63}$'),
  recipient_id uuid NOT NULL,
  subject_ids uuid[] NOT NULL,
  body text CHECK (status = 'sent' OR body IS NOT NULL),
  status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'sending', 'sent')),
  attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  provider_sid text CHECK (provider_sid ~ '^(SM|MM)[A-Za-z0-9]{32}$'),
  created_at timestamptz NOT NULL DEFAULT now(),
  sent_at timestamptz,
  CHECK ((status = 'sent') = (sent_at IS NOT NULL AND provider_sid IS NOT NULL AND body IS NULL))
);
CREATE INDEX outbox_unsent_idx ON messaging.outbox (created_at) WHERE status <> 'sent';

CREATE TABLE messaging.sender_directory (
  line_id uuid NOT NULL,
  index_version smallint NOT NULL CHECK (index_version > 0),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  business_id uuid NOT NULL,
  PRIMARY KEY (line_id, index_version),
  UNIQUE (index_version, digest)
);
