-- businesses (T165): the business's own names and its review links (BIZ-022, BIZ-052, PICK-041,
-- LIC-011; D-336, D-439, D-2943, D-2954).
--
-- businesses.business_name is one row per business: the business name it signed up under, its
-- trade name or DBA (optional) and, for a no-license business, the city license number it entered
-- (optional). These are the business's own public names, not a person's: no owner name, phone,
-- email or address is stored here (PRIV-001 covers homeowner details). A save replaces the row,
-- so the app role gets UPDATE explicitly; 0110's default for businesses gives only SELECT, INSERT.
--
-- businesses.review_link is one row per business: its Google Business Profile link and its Yelp
-- page link, each optional, stored only after `businesses::api::check_links` found it on the
-- matching site and open through the SEC-041 guard. A change replaces the row. No DELETE.

CREATE TABLE businesses.business_name (
  business_id uuid PRIMARY KEY REFERENCES businesses.business (id),
  business_name text NOT NULL
    CHECK (btrim(business_name) <> '' AND char_length(business_name) <= 200),
  trade_name text
    CHECK (trade_name IS NULL OR (btrim(trade_name) <> '' AND char_length(trade_name) <= 200)),
  city_license_number text
    CHECK (city_license_number IS NULL
      OR (btrim(city_license_number) <> '' AND char_length(city_license_number) <= 64)),
  changed_at timestamptz NOT NULL
);
GRANT SELECT, INSERT, UPDATE ON businesses.business_name TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.business_name FROM askhava_app;

CREATE TABLE businesses.review_link (
  business_id uuid PRIMARY KEY REFERENCES businesses.business (id),
  google_url text CHECK (google_url IS NULL
    OR (google_url LIKE 'https://%' AND char_length(google_url) <= 500)),
  yelp_url text CHECK (yelp_url IS NULL
    OR (yelp_url LIKE 'https://%' AND char_length(yelp_url) <= 500)),
  changed_at timestamptz NOT NULL
);
GRANT SELECT, INSERT, UPDATE ON businesses.review_link TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.review_link FROM askhava_app;
