-- catalog (T248): markets as reviewed data and job locations by ZIP centroid (INT-048, INT-022,
-- LIC-011; D-1515, D-1518, D-1521, D-3113).
--
-- catalog.market, catalog.market_town and catalog.zcta_centroid hold what the catalog sync
-- (`markets::store`) copies from docs/catalog/markets.yaml and app/catalog/data/zcta_centroids_az.csv.
-- A town code belongs to one market (its primary key); a market's launch time stays empty until a
-- later owner action records a launch, so the app role may neither insert nor update `launched_at`
-- (both grants on catalog.market name their columns). No foreign
-- key to or from another schema's tables: businesses and intake hold town codes and ZIPs as text.

CREATE TABLE catalog.market (
  id text PRIMARY KEY CHECK (id ~ '^[a-z0-9_-]+$'),
  name text NOT NULL CHECK (btrim(name) <> ''),
  state text NOT NULL CHECK (state ~ '^[A-Z]{2}$'),
  time_zone text NOT NULL CHECK (btrim(time_zone) <> ''),
  identity_sources text[] NOT NULL CHECK (cardinality(identity_sources) > 0),
  launched_at timestamptz
);

CREATE TABLE catalog.market_town (
  code text PRIMARY KEY CHECK (code ~ '^[a-z0-9_-]+$'),
  market_id text NOT NULL REFERENCES catalog.market (id),
  name text NOT NULL CHECK (btrim(name) <> ''),
  zips text[] NOT NULL,
  extra_zips text[] NOT NULL
);
CREATE INDEX market_town_market ON catalog.market_town (market_id);

CREATE TABLE catalog.zcta_centroid (
  zcta text PRIMARY KEY CHECK (zcta ~ '^[0-9]{5}$'),
  lat double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
  lon double precision NOT NULL CHECK (lon BETWEEN -180 AND 180)
);

-- The schema's default privileges (0110) give table-wide INSERT; take it back so only the
-- column-level grant below applies.
REVOKE INSERT ON catalog.market FROM askhava_app;
GRANT SELECT ON catalog.market TO askhava_app;
GRANT INSERT (id, name, state, time_zone, identity_sources) ON catalog.market TO askhava_app;
GRANT SELECT, INSERT ON catalog.market_town, catalog.zcta_centroid TO askhava_app;
GRANT UPDATE (name, state, time_zone, identity_sources) ON catalog.market TO askhava_app;
GRANT UPDATE ON catalog.market_town, catalog.zcta_centroid TO askhava_app;
REVOKE DELETE, TRUNCATE ON catalog.market, catalog.market_town, catalog.zcta_centroid FROM askhava_app;
