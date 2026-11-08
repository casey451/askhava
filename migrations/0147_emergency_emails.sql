-- businesses (T150): each person's "Emergency emails at any hour" switch with its test emergency
-- email and the answer to "Did your phone alert you with Do Not Disturb or Sleep on?", and the
-- after-hours tick's suspension (BIZ-007, BIZ-037, BIZ-042, CARD-021, INT-033; D-346, D-362).
-- businesses.emergency_switch is one row per team member: the switch, when the last test
-- emergency email was queued and the last Yes (cleared by a No or the 90-day re-test). No address
-- or phone is stored (PRIV-001) and the app role cannot delete a row. business.after_hours_suspended_at
-- is set when 2 night EMERGENCY cards in a row got no own-link visit within 60 minutes and cleared
-- by a new Yes; while set, the business is out of the INT-033 night count. The table is new and
-- empty and the column is nullable with no default, so nothing is rewritten.

CREATE TABLE businesses.emergency_switch (
  member_id uuid PRIMARY KEY REFERENCES businesses.team_member (id),
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  switch_on boolean NOT NULL DEFAULT false,
  test_sent_at timestamptz,
  yes_at timestamptz
);
CREATE INDEX emergency_switch_business ON businesses.emergency_switch (business_id)
  WHERE switch_on;
GRANT SELECT, INSERT, UPDATE ON businesses.emergency_switch TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.emergency_switch FROM askhava_app;

ALTER TABLE businesses.business ADD COLUMN after_hours_suspended_at timestamptz;
