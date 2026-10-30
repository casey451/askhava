-- ai (T025): the AI-008 gate's records, the switch's history and the structured readings (AI-008, AI-013, D-412, D-491, D-492).
-- The owner enters the provider-terms rows (the account checklist) and the privacy-policy versions; the gate reads
-- the newest terms row, any signed zero-retention date and the policy version in force today in Arizona.
-- ai.reading holds no text column: trade and job type go only to the off-database stream (AI-013, PRIV-029, D-842).
-- Every table is new and empty and append-only for the app role.

CREATE TABLE ai.provider_terms (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  recorded_at timestamptz NOT NULL DEFAULT now(),
  terms_url text NOT NULL CHECK (terms_url ~ '^https://[^[:space:]]+$' AND length(terms_url) <= 300),
  checked_on date NOT NULL,
  inputs_not_used_for_training boolean NOT NULL,
  retention_days integer NOT NULL CHECK (retention_days BETWEEN 0 AND 3650),
  flagged_retention_days integer CHECK (flagged_retention_days BETWEEN 0 AND 3650),
  spend_limit_cents bigint CHECK (spend_limit_cents > 0),
  expected_monthly_cents bigint CHECK (expected_monthly_cents > 0),
  zero_retention_signed_on date
);

CREATE TABLE ai.privacy_policy_version (
  version text PRIMARY KEY CHECK (version ~ '^[0-9a-z][0-9a-z.-]{0,31}$'),
  effective_on date NOT NULL,
  has_ai018_sentence boolean NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE ai.switch_history (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
  at timestamptz NOT NULL DEFAULT now(),
  turned_on boolean NOT NULL,
  owner_id uuid NOT NULL
);

CREATE TABLE ai.reading (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  at timestamptz NOT NULL DEFAULT now(),
  subject_id uuid NOT NULL,
  trade_confidence double precision NOT NULL CHECK (trade_confidence BETWEEN 0 AND 1),
  job_type_confidence double precision NOT NULL CHECK (job_type_confidence BETWEEN 0 AND 1),
  emergency boolean NOT NULL,
  urgent boolean NOT NULL,
  major boolean NOT NULL,
  spanish boolean NOT NULL
);
CREATE INDEX reading_at_idx ON ai.reading (at);

GRANT SELECT, INSERT ON ai.provider_terms, ai.privacy_policy_version, ai.switch_history, ai.reading TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON ai.provider_terms, ai.privacy_policy_version, ai.switch_history, ai.reading
  FROM askhava_app;
