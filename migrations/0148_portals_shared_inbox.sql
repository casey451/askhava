-- portals (T199): each person's "This inbox is shared" switch (SEC-027, D-333, D-478).
-- portals.shared_inbox holds one row per team member who ever used the switch: the member and
-- business IDs (no cross-schema foreign key), whether the notice inbox is marked shared now and
-- when it last changed. While shared, every Confirm by that person needs a fresh Verify code to
-- their own phone (T232); Quote and Hold links keep working. No address or phone is stored.
-- The table is new and empty.

CREATE TABLE portals.shared_inbox (
  member_id uuid PRIMARY KEY,
  business_id uuid NOT NULL,
  shared boolean NOT NULL,
  changed_at timestamptz NOT NULL
);

-- The switch only flips; nothing is deleted by the app.
GRANT SELECT, INSERT ON portals.shared_inbox TO askhava_app;
GRANT UPDATE (shared, changed_at) ON portals.shared_inbox TO askhava_app;
