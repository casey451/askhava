-- businesses (T081): the sign-up record, the identity chain's progress and the business terms
-- acceptance (BIZ-001 to BIZ-003, BIZ-016, BIZ-019 to BIZ-022, BIZ-053, TERMS-016, TERMS-033,
-- D-059, D-145, D-281, D-457, D-492). businesses.signup is one row per business: mailbox_id is the
-- opaque ID of the required sign-up email (BIZ-019; the address lives in privacy, PRIV-001), the
-- optional source answer and g code (BIZ-053, never routed or shown), the identity step reached
-- (text to the record phone, then the record email, then the letter; never a voice call), the
-- code and the license or city-list match, the flag that sends it to needs review, when it began
-- waiting and was last told its status (BIZ-020), and the About us line (BIZ-022, 140 characters).
-- No contact value is stored. businesses.terms_acceptance is append-only: the document, version,
-- SHA-256 of the published text, language, channel, assent line shown, time and business person.
-- Both tables are new and empty.

CREATE TABLE businesses.signup (
  business_id uuid PRIMARY KEY REFERENCES businesses.business (id),
  mailbox_id uuid NOT NULL,
  heard_from text CHECK (heard_from IN ('facebook_group', 'another_business',
    'friend_or_neighbor', 'met_the_owner', 'search', 'other')),
  g_code text CHECK (g_code ~ '^[a-z0-9][a-z0-9_-]{0,31}$'),
  identity_step text CHECK (identity_step IN ('text', 'email', 'letter')),
  code_done boolean NOT NULL DEFAULT false,
  match_passed boolean NOT NULL DEFAULT false,
  flag text CHECK (flag IN ('record_mismatch', 'name_mismatch', 'fraud_signal',
    'no_record_contact')),
  review_since timestamptz,
  review_told_at timestamptz,
  about_us text CHECK (char_length(about_us) BETWEEN 1 AND 140),
  submitted_at timestamptz NOT NULL
);
CREATE INDEX signup_review_idx ON businesses.signup (review_told_at)
  WHERE review_since IS NOT NULL;

CREATE TABLE businesses.terms_acceptance (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  person_id uuid NOT NULL,
  document text NOT NULL CHECK (document ~ '^[a-z][a-z0-9_]{0,63}$'),
  version text NOT NULL CHECK (version ~ '^[a-z0-9][a-z0-9_.:-]{0,63}$'),
  text_sha256 text NOT NULL CHECK (text_sha256 ~ '^[0-9a-f]{64}$'),
  language text NOT NULL CHECK (language IN ('en', 'es')),
  channel text NOT NULL CHECK (channel = 'business_signup'),
  assent_line text NOT NULL CHECK (length(assent_line) BETWEEN 1 AND 300),
  accepted_at timestamptz NOT NULL
);
CREATE INDEX terms_acceptance_business_idx ON businesses.terms_acceptance (business_id);

GRANT SELECT, INSERT, UPDATE ON businesses.signup TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.signup FROM askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON businesses.terms_acceptance FROM askhava_app;
