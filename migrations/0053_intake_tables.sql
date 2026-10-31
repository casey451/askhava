-- intake (T038): the intake schema and the tables later intake tasks fill (docs/architecture.md
-- section 3, D-492, D-493), with the consent columns (CON-001, CON-005, CON-022, CON-027, CON-031).
-- intake.homeowner is one row per vault contact; intake.property holds only the vault contact ID
-- and the town, never an address (PRIV-028). intake.conversation is one request's conversation:
-- at most one open per homeowner (a partial unique index, so two first texts at once make one
-- request and one consent text, CON-027), its language (CON-026), the inbound text that opened it
-- and when its consent text was queued with which template (CON-031). intake.consent_record keeps
-- each consent: time, inbound message or web-chat session, channel and consent-text version
-- (CON-005, T181 writes it); it is append-only for askhava_app and kept 5 years (CON-022).
-- intake.intake_answer, intake.emergency_event and intake.recruiting_demand are filled by T040,
-- T039 and T043; they hold IDs, codes and times only, never homeowner text (PRIV-001).
-- job IDs and message IDs name other modules' records by ID only: no cross-schema foreign key.
-- All tables are new and empty.

CREATE SCHEMA IF NOT EXISTS intake;
GRANT USAGE ON SCHEMA intake TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA intake
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE intake.homeowner (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  contact_id uuid NOT NULL UNIQUE,
  created_at timestamptz NOT NULL
);

CREATE TABLE intake.property (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  homeowner_id uuid NOT NULL REFERENCES intake.homeowner (id),
  contact_id uuid NOT NULL,
  town text NOT NULL CHECK (town ~ '^[a-z][a-z0-9_]{0,63}$'),
  created_at timestamptz NOT NULL
);
CREATE INDEX property_homeowner_idx ON intake.property (homeowner_id);

CREATE TABLE intake.conversation (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  homeowner_id uuid NOT NULL REFERENCES intake.homeowner (id),
  job_id uuid,
  language text NOT NULL CHECK (language IN ('en', 'es')),
  first_inbox_id uuid NOT NULL,
  consent_template text NOT NULL CHECK (consent_template ~ '^[a-z][a-z0-9_]{0,63}$'),
  consent_sent_at timestamptz NOT NULL,
  opened_at timestamptz NOT NULL,
  closed_at timestamptz CHECK (closed_at IS NULL OR closed_at >= opened_at)
);
CREATE UNIQUE INDEX conversation_one_open_idx ON intake.conversation (homeowner_id)
  WHERE closed_at IS NULL;

CREATE TABLE intake.consent_record (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  homeowner_id uuid NOT NULL REFERENCES intake.homeowner (id),
  conversation_id uuid NOT NULL REFERENCES intake.conversation (id),
  channel text NOT NULL CHECK (channel IN ('sms', 'web_chat')),
  inbox_id uuid,
  web_chat_session uuid,
  consent_version text NOT NULL CHECK (consent_version ~ '^[a-z][a-z0-9_.:-]{0,63}$'),
  consented_at timestamptz NOT NULL,
  CHECK ((channel = 'sms') = (inbox_id IS NOT NULL AND web_chat_session IS NULL)),
  CHECK ((channel = 'web_chat') = (web_chat_session IS NOT NULL AND inbox_id IS NULL))
);
CREATE INDEX consent_record_homeowner_idx ON intake.consent_record (homeowner_id);
REVOKE UPDATE, DELETE, TRUNCATE ON intake.consent_record FROM askhava_app;

CREATE TABLE intake.intake_answer (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  conversation_id uuid NOT NULL REFERENCES intake.conversation (id),
  question_id text NOT NULL CHECK (question_id ~ '^[a-z][a-z0-9_.]{0,63}$'),
  choice text CHECK (choice ~ '^[a-z0-9][a-z0-9_.]{0,63}$'),
  inbox_id uuid,
  answered_at timestamptz NOT NULL
);
CREATE INDEX intake_answer_conversation_idx ON intake.intake_answer (conversation_id);

CREATE TABLE intake.emergency_event (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  conversation_id uuid REFERENCES intake.conversation (id),
  inbox_id uuid NOT NULL UNIQUE,
  matched text NOT NULL CHECK (matched ~ '^[a-z][a-z0-9_]{0,63}$'),
  text_queued_at timestamptz,
  safe_at timestamptz,
  created_at timestamptz NOT NULL
);

CREATE TABLE intake.recruiting_demand (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  conversation_id uuid NOT NULL REFERENCES intake.conversation (id),
  town text NOT NULL CHECK (town ~ '^[a-z][a-z0-9_]{0,63}$'),
  trade text NOT NULL CHECK (trade ~ '^[a-z][a-z0-9_]{0,63}$'),
  job_type text CHECK (job_type ~ '^[a-z][a-z0-9_]{0,63}$'),
  created_at timestamptz NOT NULL
);
CREATE INDEX recruiting_demand_town_trade_idx ON intake.recruiting_demand (town, trade);
