-- licensing (T034): the `licensing` schema and `licensing.business_license`, one row per other-agency
-- license a business holds (LIC-012, LIC-013, LIC-015, LIC-016, LIC-047). A pest-management row is the
-- business's own sign-up attestation (`self_attested`): its license number and the wood-destroying
-- and weed-control record categories it ticked, never checked by Ask Hava (D-417). The inspector,
-- alarm and septage-hauler rows come from the agency record (`agency_record`, filled by T036).
-- `paused_at` is set when a homeowner reports a pest license wrong; it is cleared, with the number
-- replaced, once the business sends a number the homeowner or Casey finds on the Department's
-- search (LIC-047). No contact value is stored. The `agency` codes are `licensing::api::Agency::code`.
-- The table is new and empty.

CREATE SCHEMA IF NOT EXISTS licensing;
GRANT USAGE ON SCHEMA licensing TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA licensing
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE licensing.business_license (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL,
  agency text NOT NULL CHECK (agency IN ('pest_management', 'home_inspector', 'alarm_business', 'septage_hauler')),
  license_number text NOT NULL CHECK (license_number ~ '^[A-Za-z0-9-]{1,20}$'),
  verification text NOT NULL CHECK (verification IN ('self_attested', 'agency_record')),
  wood_destroying boolean NOT NULL DEFAULT false,
  weed_control boolean NOT NULL DEFAULT false,
  recorded_at timestamptz NOT NULL DEFAULT now(),
  paused_at timestamptz,
  CHECK ((agency = 'pest_management') = (verification = 'self_attested')),
  CHECK (agency = 'pest_management' OR NOT (wood_destroying OR weed_control OR paused_at IS NOT NULL))
);
CREATE UNIQUE INDEX business_license_agency_idx ON licensing.business_license (business_id, agency);

GRANT SELECT, INSERT, UPDATE ON licensing.business_license TO askhava_app;
REVOKE DELETE, TRUNCATE ON licensing.business_license FROM askhava_app;
