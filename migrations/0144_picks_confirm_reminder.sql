-- picks (T197): picks.window_extension and picks.confirm_reminder (PICK-019, PICK-037, PICK-043,
-- D-345, D-380). picks.window_extension: a HOLD or the all-bounced grace, once per pick and kind by
-- the primary key, with the window's new end. picks.confirm_reminder: each reminder email of a pick
-- waiting for Confirm, claimed once per pick and send time, so a rerun sends nothing twice. No
-- contact value is stored (PRIV-002). Both are append-only for askhava_app; both are new and empty.

CREATE TABLE picks.window_extension (
  pick_id uuid NOT NULL REFERENCES picks.pick (id),
  kind text NOT NULL CHECK (kind IN ('hold', 'bounce')),
  new_due timestamptz NOT NULL,
  granted_at timestamptz NOT NULL,
  PRIMARY KEY (pick_id, kind)
);
GRANT SELECT, INSERT ON picks.window_extension TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.window_extension FROM askhava_app;

CREATE TABLE picks.confirm_reminder (
  pick_id uuid NOT NULL REFERENCES picks.pick (id),
  remind_at timestamptz NOT NULL,
  last boolean NOT NULL,
  claimed_at timestamptz NOT NULL,
  PRIMARY KEY (pick_id, remind_at)
);
GRANT SELECT, INSERT ON picks.confirm_reminder TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.confirm_reminder FROM askhava_app;
