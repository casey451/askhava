-- messaging (T178): the webhook dedupe table, the inbox and the delivery events (TXT-012, TXT-013).
-- messaging.webhook_event is the module's dedupe table in the shape `askhava_core::api::Dedupe`
-- documents (SEC-017, SEC-039): an inbound text is keyed on its MessageSid under the source
-- `twilio_inbound`, a status callback on (MessageSid, MessageStatus) under `twilio_status`, so a
-- repeated delivery changes nothing after the first (TXT-012).
-- messaging.inbox holds one row per inbound text, unique on Twilio's MessageSid; later tasks add
-- the scrubbed text and flags (docs/architecture.md section 7). No phone or body here (PRIV-002).
-- messaging.delivery_event holds every first status callback per (MessageSid, status), the status
-- folded to queued, sending, sent, delivered, undelivered or failed, with Twilio's error code;
-- `messaging::api::delivery_status` reads a text's status from them by its provider SID, so a
-- callback that arrives before the send is marked still counts once it is (TXT-013, D-861).
-- All three are new and empty.

CREATE TABLE messaging.webhook_event (
  source text NOT NULL CHECK (source IN ('twilio_inbound', 'twilio_status')),
  event_key text NOT NULL,
  received_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (source, event_key)
);

CREATE TABLE messaging.inbox (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  message_sid text NOT NULL UNIQUE CHECK (message_sid ~ '^(SM|MM)[A-Za-z0-9]{32}$'),
  received_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE messaging.delivery_event (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  provider_sid text NOT NULL CHECK (provider_sid ~ '^(SM|MM)[A-Za-z0-9]{32}$'),
  status text NOT NULL
    CHECK (status IN ('queued', 'sending', 'sent', 'delivered', 'undelivered', 'failed')),
  error_code integer CHECK (error_code > 0),
  received_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX delivery_event_sid_idx ON messaging.delivery_event (provider_sid, id);
