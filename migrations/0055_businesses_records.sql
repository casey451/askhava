-- businesses (T041): the businesses schema and the business records matching reads (BIZ-004 to
-- BIZ-009, D-112). businesses.business is one row per business account: its status (pending,
-- active, paused with a reason, waitlist, needs review or removed, BIZ-009), the "also send me jobs
-- nobody else covers" switch (BIZ-005), the "We speak Spanish" tick (BIZ-006), the business-reported
-- insurance answer, NULL until answered and with no verified, expiry or pause column (BIZ-008,
-- D-061), and the after-hours tick (BIZ-007; T150 adds the per-person switch and test-email Yes).
-- businesses.town is the towns it serves; businesses.job_type_choice the job types it ticked, each
-- with its category code, within what its license covers (BIZ-004, LIC-043). No name, phone,
-- email or address is stored here (PRIV-001). All tables are new and empty.

CREATE SCHEMA IF NOT EXISTS businesses;
GRANT USAGE ON SCHEMA businesses TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA businesses
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

CREATE TABLE businesses.business (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  status text NOT NULL
    CHECK (status IN ('pending', 'active', 'paused', 'waitlist', 'needs_review', 'removed')),
  pause_reason text CHECK (pause_reason ~ '^[a-z][a-z0-9_]{0,63}$'),
  overflow_on boolean NOT NULL DEFAULT false,
  speaks_spanish boolean NOT NULL DEFAULT false,
  insured boolean,
  after_hours boolean NOT NULL DEFAULT false,
  created_at timestamptz NOT NULL,
  status_changed_at timestamptz NOT NULL,
  CHECK ((status = 'paused') = (pause_reason IS NOT NULL))
);

CREATE TABLE businesses.town (
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  town text NOT NULL CHECK (town ~ '^[a-z][a-z0-9_]{0,63}$'),
  PRIMARY KEY (business_id, town)
);
CREATE INDEX town_town_idx ON businesses.town (town);

CREATE TABLE businesses.job_type_choice (
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  category text NOT NULL CHECK (category ~ '^[a-z][a-z0-9_]{0,63}$'),
  job_type text NOT NULL CHECK (btrim(job_type) <> '' AND length(job_type) <= 100),
  PRIMARY KEY (business_id, category, job_type)
);
CREATE INDEX job_type_choice_category_idx ON businesses.job_type_choice (category, business_id);

-- A business is removed by its status, never by deleting the row (BIZ-009).
GRANT SELECT, INSERT, UPDATE ON businesses.business TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.business FROM askhava_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON businesses.town, businesses.job_type_choice TO askhava_app;
REVOKE TRUNCATE ON businesses.town, businesses.job_type_choice FROM askhava_app;
