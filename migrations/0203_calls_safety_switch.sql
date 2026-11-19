-- calls (T407): the owner-portal "safety voice call" switch and Casey's recorded TCPA attorney
-- approval (CALL-019, CALL-022, SEC-043, CON-031, TXT-024; D-275, D-384, D-3134).
-- calls.safety_switch is append-only: each change Casey makes on the owner settings page is a new
-- row and the newest (changed_at, then id) is the one in force; no row means off, the launch
-- state (D-384). An "on" row holds the approval's date and reference (1 to 200 characters), which
-- `calls::api::turn_safety_switch_on` stores only when `SafetySwitch::turn_on` gives on for it and
-- the compiled-in CALL-022 conditions; an "off" row holds neither. `changed_by` is the owner
-- record's ID. No homeowner detail. The calls default privileges give the app role SELECT and
-- INSERT only (D-1520); UPDATE, DELETE and TRUNCATE are revoked here as well. The table is new.

CREATE TABLE calls.safety_switch (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  is_on boolean NOT NULL,
  approved_on date,
  reference text CHECK (reference IS NULL OR length(btrim(reference)) BETWEEN 1 AND 200),
  changed_by uuid NOT NULL,
  changed_at timestamptz NOT NULL,
  CHECK (is_on = (approved_on IS NOT NULL)),
  CHECK (is_on = (reference IS NOT NULL))
);
CREATE INDEX safety_switch_latest_idx ON calls.safety_switch (changed_at DESC, id);

GRANT SELECT, INSERT ON calls.safety_switch TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON calls.safety_switch FROM askhava_app;
