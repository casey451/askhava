-- quotes (T411): the scrubbed job description and the shown or released job card, kept per job for
-- 3 years after close (PRIV-008, PRIV-011, PRIV-006, CARD-007, CARD-023, INT-041; D-3189, D-801,
-- D-3118). quotes.job_description is one row per inbound text whose scrubbed words describe a job:
-- the request (intake.conversation ID), the job, the message (the messaging.inbox row ID for a text,
-- the chat message's own ID for web chat), the words exactly as privacy's scrubber gave them and the
-- recorded time; a SPLIT opens a second job from the same text, so the key is message and job
-- together. quotes.shown_card is one row per card shown or released for a job: request, job, the
-- 32-hex card digest (the first 32 hex digits of the SHA-256 of the card text, as intake's approval
-- stores it), the card text, town, place code and photo IDs exactly as build_card made them, the
-- category code and remaining candidate job type codes, the INT-029 emergency mark, the recorded
-- time and sent_at (null until the card sender marks it; at most one sent card per job). No name,
-- phone, email, street address or contact column. Raw inbound bodies are never stored here (D-801).
-- Request, job and message IDs name other modules' rows by ID only: no cross-schema foreign key, so
-- an inbox purge is never blocked. The app role reads, inserts and deletes (the PRIV-008 text stage
-- and "Delete my data") and may update shown_card.sent_at only. Both tables are new and empty.

CREATE TABLE quotes.job_description (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  request_id uuid NOT NULL,
  job_id uuid NOT NULL,
  message_id uuid NOT NULL,
  words text NOT NULL CHECK (length(words) BETWEEN 1 AND 20000),
  recorded_at timestamptz NOT NULL,
  UNIQUE (message_id, job_id)
);
CREATE INDEX job_description_job_idx ON quotes.job_description (job_id, recorded_at);
CREATE INDEX job_description_request_idx ON quotes.job_description (request_id);

CREATE TABLE quotes.shown_card (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  request_id uuid NOT NULL,
  job_id uuid NOT NULL,
  digest text NOT NULL CHECK (digest ~ '^[0-9a-f]{32}$'),
  card_text text NOT NULL CHECK (length(card_text) BETWEEN 1 AND 20000),
  town text NOT NULL,
  place text NOT NULL CHECK (place IN ('house', 'marina', 'storage_lot', 'ramp')),
  photo_ids uuid[] NOT NULL,
  category text NOT NULL CHECK (category ~ '^[a-z][a-z0-9_]{0,63}$'),
  candidates text[] NOT NULL,
  emergency boolean NOT NULL,
  recorded_at timestamptz NOT NULL,
  sent_at timestamptz CHECK (sent_at IS NULL OR sent_at >= recorded_at),
  UNIQUE (request_id, job_id, digest)
);
CREATE INDEX shown_card_job_idx ON quotes.shown_card (job_id, recorded_at);
CREATE INDEX shown_card_request_idx ON quotes.shown_card (request_id);
CREATE UNIQUE INDEX shown_card_one_sent_idx ON quotes.shown_card (job_id)
  WHERE sent_at IS NOT NULL;

REVOKE UPDATE, TRUNCATE ON quotes.job_description FROM askhava_app;
GRANT SELECT, INSERT, DELETE ON quotes.job_description TO askhava_app;
REVOKE UPDATE, TRUNCATE ON quotes.shown_card FROM askhava_app;
GRANT SELECT, INSERT, DELETE ON quotes.shown_card TO askhava_app;
GRANT UPDATE (sent_at) ON quotes.shown_card TO askhava_app;
