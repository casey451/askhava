-- catalog (T021): categories, job types, license rules, fee rules and the not-offered list (LIC-001 to LIC-005, AI-017, D-202).
-- catalog.category is the one place a category is switched on or off: `enabled` is false until it is confirmed
-- in the K2b list (docs/catalog/categories.yaml) and always false for the not-offered kinds (LIC-003, LIC-004).
-- Turning it on or off is an UPDATE of that one column plus an ops audit row in the same transaction (LIC-002,
-- app/catalog/src/service.rs), so it needs no code change. catalog.license_rule is written only from the
-- reviewed file by the loader; no AI output reaches any column here (AI-017). The card-field flag of each intake
-- question is T177's table, not this one. All tables are new and empty; the app role's rights come from the
-- default privileges below (postgres brain P5).

CREATE SCHEMA IF NOT EXISTS catalog;
GRANT USAGE ON SCHEMA catalog TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA catalog
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE catalog.category (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  code text NOT NULL UNIQUE CHECK (code ~ '^[a-z][a-z0-9_]{0,63}$'),
  name text NOT NULL CHECK (name <> ''),
  trade_group text NOT NULL CHECK (trade_group <> ''),
  enabled boolean NOT NULL DEFAULT false,
  recurring boolean NOT NULL,
  after_hours_emergency boolean NOT NULL,
  quote_format text NOT NULL
    CHECK (quote_format IN ('range', 'monthly', 'tiers', 'firm', 'diagnostic_visit')),
  keywords text[] NOT NULL DEFAULT '{}'
);

CREATE TABLE catalog.job_type (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  category_id uuid NOT NULL REFERENCES catalog.category (id),
  name text NOT NULL CHECK (name <> ''),
  one_time boolean NOT NULL DEFAULT false,
  fee_free boolean NOT NULL DEFAULT false,
  kind text CHECK (kind IN ('repair', 'replace', 'install')),
  quote_format text
    CHECK (quote_format IN ('range', 'monthly', 'tiers', 'firm', 'diagnostic_visit')),
  UNIQUE (category_id, name)
);

CREATE TABLE catalog.license_rule (
  category_id uuid PRIMARY KEY REFERENCES catalog.category (id),
  kind text NOT NULL CHECK (kind IN ('none', 'roc', 'state_agency', 'roc_and_state_agency')),
  classes text[] NOT NULL DEFAULT '{}',
  agency text,
  handyman_allowed boolean NOT NULL,
  construction_type boolean NOT NULL,
  CHECK ((kind IN ('state_agency', 'roc_and_state_agency')) = (agency IS NOT NULL)),
  CHECK (kind IN ('roc', 'roc_and_state_agency') OR cardinality(classes) = 0)
);

CREATE TABLE catalog.fee_rule (
  category_id uuid PRIMARY KEY REFERENCES catalog.category (id),
  trip_fee_tier integer NOT NULL CHECK (trip_fee_tier BETWEEN 25 AND 75)
);

CREATE TABLE catalog.not_offered (
  name text PRIMARY KEY CHECK (name = lower(name) AND name <> '')
);
