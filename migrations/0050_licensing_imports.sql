-- licensing (T036): the daily ROC and city license lists and each business's daily check (CARD-015,
-- LIC-006, LIC-007, LIC-010, LIC-027). `list_import` is one row per list per day, with the rows
-- recorded and the malformed rows skipped; a second import of the same list and day records nothing.
-- `agency_record` is one ROC list row (license number, class, status, original issue date) and
-- `city_license_entry` one city list row (license number, business, trade and owner names, issue
-- date), each with its source and list date. Only what verification needs is kept: no address, phone
-- or email from either list is stored, and nothing here is ever read to find businesses (LIC-010).
-- `license_check` is one row per business per daily check, the result of matching its license number
-- on the latest list (`active`, `inactive` or `not_listed`); a card needs the latest to be `active`
-- (CARD-015). All four tables are append-only for `askhava_app`. The tables are new and empty.

CREATE TABLE licensing.list_import (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  source text NOT NULL CHECK (source IN ('roc', 'city')),
  list_date date NOT NULL,
  rows_recorded integer NOT NULL CHECK (rows_recorded >= 0),
  rows_skipped integer NOT NULL CHECK (rows_skipped >= 0),
  imported_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (source, list_date)
);

CREATE TABLE licensing.agency_record (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  import_id uuid NOT NULL REFERENCES licensing.list_import (id),
  source text NOT NULL CHECK (source = 'roc'),
  list_date date NOT NULL,
  license_number text NOT NULL CHECK (btrim(license_number) <> '' AND length(license_number) <= 64),
  class_code text NOT NULL CHECK (btrim(class_code) <> '' AND length(class_code) <= 16),
  status text NOT NULL CHECK (status IN ('active', 'inactive')),
  issued_on date,
  UNIQUE (import_id, license_number, class_code)
);
CREATE INDEX agency_record_number_idx ON licensing.agency_record (license_number, list_date);

CREATE TABLE licensing.city_license_entry (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  import_id uuid NOT NULL REFERENCES licensing.list_import (id),
  source text NOT NULL CHECK (source = 'city'),
  list_date date NOT NULL,
  license_number text CHECK (license_number IS NULL OR length(license_number) <= 64),
  business_name text NOT NULL CHECK (btrim(business_name) <> '' AND length(business_name) <= 200),
  trade_name text CHECK (trade_name IS NULL OR length(trade_name) <= 200),
  owner_name text CHECK (owner_name IS NULL OR length(owner_name) <= 200),
  issued_on date
);
CREATE INDEX city_license_entry_import_idx ON licensing.city_license_entry (import_id);

CREATE TABLE licensing.license_check (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL,
  source text NOT NULL CHECK (source = 'roc'),
  license_number text NOT NULL CHECK (btrim(license_number) <> '' AND length(license_number) <= 64),
  list_date date NOT NULL,
  status text NOT NULL CHECK (status IN ('active', 'inactive', 'not_listed')),
  checked_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX license_check_business_idx ON licensing.license_check (business_id, checked_at DESC);

GRANT SELECT, INSERT ON licensing.list_import, licensing.agency_record,
  licensing.city_license_entry, licensing.license_check TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON licensing.list_import, licensing.agency_record,
  licensing.city_license_entry, licensing.license_check FROM askhava_app;
