-- businesses (T248): each business's service area, a base ZIP and how far it will travel for a job
-- (BIZ-058, BIZ-049, INT-048; D-1515, D-1518, D-3116).
--
-- businesses.service_area is one row per business: its base ZIP as 5 digits and its radius in whole
-- miles from 1 to 100, both required. No location is stored: the ZIP's centroid is read from the
-- catalog's reviewed table (`catalog::api::zip_location`), so a base with none (an out-of-state
-- ZIP) has no location. No foreign key to `catalog`. Sign-up inserts it and the portal edit
-- (T249) updates it, so UPDATE is granted explicitly (T250 narrows the schema default).

CREATE TABLE businesses.service_area (
  business_id uuid PRIMARY KEY REFERENCES businesses.business (id),
  base_zip text NOT NULL CHECK (base_zip ~ '^[0-9]{5}$'),
  radius_miles smallint NOT NULL CHECK (radius_miles >= 1 AND radius_miles <= 100),
  changed_at timestamptz NOT NULL
);
GRANT SELECT, INSERT, UPDATE ON businesses.service_area TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.service_area FROM askhava_app;
