-- intake (T181): the homeowner terms acceptance record (TERMS-001, TERMS-016, TERMS-033, D-492).
-- One row per pick that accepted the homeowner terms, on the quote page or by a "<number> YES"
-- text pick, never by the consent text, the first text or any other reply (TERMS-001). Each row
-- keeps the document, its version, the SHA-256 of the exact published text (64 lower-case hex
-- digits), the language, the channel, the assent line shown beside the pick, the time, the request
-- and the pick, and for a text pick the inbound message that picked. One acceptance per pick, so a
-- pick handled twice stores one row. It holds IDs, codes and the published assent line only, never
-- a contact value (PRIV-028). Append-only for askhava_app, like intake.consent_record (postgres
-- brain P5). Business terms acceptances (business sign-up, Confirm page) are the businesses
-- module's. The pick ID names a picks record by ID only: no cross-schema foreign key. New and empty.

CREATE TABLE intake.terms_acceptance (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  homeowner_id uuid NOT NULL REFERENCES intake.homeowner (id),
  conversation_id uuid NOT NULL REFERENCES intake.conversation (id),
  pick_id uuid NOT NULL UNIQUE,
  document text NOT NULL CHECK (document ~ '^[a-z][a-z0-9_]{0,63}$'),
  version text NOT NULL CHECK (version ~ '^[a-z0-9][a-z0-9_.:-]{0,63}$'),
  text_sha256 text NOT NULL CHECK (text_sha256 ~ '^[0-9a-f]{64}$'),
  language text NOT NULL CHECK (language IN ('en', 'es')),
  channel text NOT NULL CHECK (channel IN ('quote_page', 'text_pick')),
  assent_line text NOT NULL CHECK (length(assent_line) BETWEEN 1 AND 300),
  inbox_id uuid,
  accepted_at timestamptz NOT NULL,
  CHECK ((channel = 'text_pick') = (inbox_id IS NOT NULL))
);
CREATE INDEX terms_acceptance_conversation_idx ON intake.terms_acceptance (conversation_id);
REVOKE UPDATE, DELETE, TRUNCATE ON intake.terms_acceptance FROM askhava_app;
