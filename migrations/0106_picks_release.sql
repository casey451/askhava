-- picks (T069): picks.release and picks.address_wait (PICK-022, PICK-023, PICK-024, PRIV-005,
-- PRIV-020, BIZ-014, INT-029, D-492). picks.release: one row per pick whose details were released
-- to the business portal, on a ReleaseProof only: the job and business by ID (no cross-schema
-- foreign key, architecture section 4 rule 3), why (a paid charge or a recorded $0 fee decision)
-- and when; the primary key makes the release happen once however often a proof arrives.
-- picks.address_wait: each acceptance of an emergency job that came before the street address,
-- when nothing was released and the homeowner was asked again (TD-INT-34). No contact value is
-- stored in either (PRIV-002). Both are append-only for askhava_app; both are new and empty.

CREATE TABLE picks.release (
  pick_id uuid PRIMARY KEY REFERENCES picks.pick (id),
  job_id uuid NOT NULL,
  business_id uuid NOT NULL,
  basis text NOT NULL CHECK (basis IN ('paid', 'no_fee')),
  released_at timestamptz NOT NULL
);
GRANT SELECT, INSERT ON picks.release TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.release FROM askhava_app;

CREATE TABLE picks.address_wait (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  pick_id uuid NOT NULL REFERENCES picks.pick (id),
  job_id uuid NOT NULL,
  asked_at timestamptz NOT NULL
);
CREATE INDEX address_wait_pick_idx ON picks.address_wait (pick_id);
GRANT SELECT, INSERT ON picks.address_wait TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.address_wait FROM askhava_app;
