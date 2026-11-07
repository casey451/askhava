-- businesses (T147): each business's notice channel choice, email, text or both (BIZ-036,
-- TXT-035, BIZ-019, D-307). One row per business that chose; no row means email, the default and
-- the only choice while the business-texting switch is off (the code refuses text then). No
-- contact value is stored (PRIV-001). The table is new and empty.

CREATE TABLE businesses.notice_channel (
  business_id uuid PRIMARY KEY REFERENCES businesses.business (id),
  channel text NOT NULL CHECK (channel IN ('email', 'text', 'both')),
  changed_at timestamptz NOT NULL
);

-- The owner changes the choice in the portal; a row is never deleted by the app.
GRANT SELECT, INSERT, UPDATE ON businesses.notice_channel TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.notice_channel FROM askhava_app;
