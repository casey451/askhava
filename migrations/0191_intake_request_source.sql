-- intake (T103): the web-chat session and the request source (INT-001, INT-047, CON-025, AI-021,
-- PRIV-001, D-457, D-4044). intake.conversation.request_source is INT-047's source: the `g` link
-- code a web chat started from, 'web' for a web chat with no code, 'text' for a request started
-- by text (the default, so the text path's insert is unchanged); the scorecard reads it and
-- routing never does. intake.web_chat_session is one web chat: the SHA256 digest of its session
-- key (never the key), its source, the vault contact ID once a number is typed, when the last
-- Verify code went, the wrong tries, when the code was checked and the request it opened. IDs,
-- a digest, a code and times only: the typed number lives in the vault alone (PRIV-001).
ALTER TABLE intake.conversation
  ADD COLUMN request_source text NOT NULL DEFAULT 'text'
  CHECK (request_source ~ '^[a-z0-9][a-z0-9_-]{0,31}$');

CREATE TABLE intake.web_chat_session (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  key_digest bytea NOT NULL UNIQUE CHECK (length(key_digest) = 32),
  request_source text NOT NULL CHECK (request_source ~ '^[a-z0-9][a-z0-9_-]{0,31}$'),
  first_message_id uuid,
  contact_id uuid,
  code_sent_at timestamptz,
  wrong_tries integer NOT NULL DEFAULT 0 CHECK (wrong_tries >= 0),
  verified_at timestamptz,
  conversation_id uuid REFERENCES intake.conversation (id),
  started_at timestamptz NOT NULL,
  CHECK (verified_at IS NULL OR (contact_id IS NOT NULL AND code_sent_at IS NOT NULL))
);
GRANT SELECT, INSERT ON intake.web_chat_session TO askhava_app;
GRANT UPDATE ON intake.web_chat_session TO askhava_app;
