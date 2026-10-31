-- intake (T156): intake.golden_result, one row per golden-set run (OPS-040, D-493). A run is made
-- once per catalog version (the fingerprint of the compiled-in catalog, question bank and golden
-- set), so the version is unique; the row holds counts and a time only, no request text. T210's
-- monthly report reads the share through intake::api. The table is new and empty.

CREATE TABLE intake.golden_result (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  catalog_version text NOT NULL UNIQUE CHECK (catalog_version ~ '^[0-9a-z_-]{1,64}$'),
  rows_total integer NOT NULL CHECK (rows_total > 0),
  rows_right integer NOT NULL CHECK (rows_right BETWEEN 0 AND rows_total),
  ran_at timestamptz NOT NULL
);

-- A stored run is a record: the app adds runs and never changes one.
GRANT SELECT, INSERT ON intake.golden_result TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON intake.golden_result FROM askhava_app;
