-- messaging (T527): the texting provider's written answer on business texting, filed by Casey in
-- the owner portal (TXT-035, TERMS-014; D-303, D-305, D-3881). messaging.business_eligibility is
-- append-only: each filing is a new row and the newest (seq) is in force. A row holds the answer's
-- date, the provider's reference, the answer text as received, whether it says business texting is
-- allowed, the separate business campaign's registration reference (none until registered), who
-- filed it (the owner record's ID) and when. `messaging::api::switch_on` turns the business line
-- on only while the newest row says allowed and names a campaign reference; no row means no. No
-- homeowner or business contact detail. The app role keeps SELECT and INSERT only. The table is new.

CREATE TABLE messaging.business_eligibility (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  answered_on date NOT NULL,
  reference text NOT NULL CHECK (length(btrim(reference)) BETWEEN 1 AND 200),
  answer text NOT NULL CHECK (length(btrim(answer)) BETWEEN 1 AND 4000),
  allowed boolean NOT NULL,
  campaign_reference text
    CHECK (campaign_reference IS NULL OR length(btrim(campaign_reference)) BETWEEN 1 AND 200),
  filed_by uuid NOT NULL,
  filed_at timestamptz NOT NULL
);

GRANT SELECT, INSERT ON messaging.business_eligibility TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON messaging.business_eligibility FROM askhava_app;
