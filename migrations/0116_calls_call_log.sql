-- calls (T078): calls.call_log, one row per finished masked call (CALL-006, CALL-007, CALL-008;
-- D-141, D-1689 to D-1691).
-- Each row names the line it was on (a calls.number_pool row, same schema), the provider's call
-- SID (one row per call, so a repeated status callback logs nothing twice), who called (the
-- business or the homeowner, by role only: no phone number is stored), who answered (a person, a
-- machine or nobody), its length in whole seconds and whether it reached the 45-second mark, kept
-- for reports only (CALL-007: no rule reads it). Calls are never recorded (CALL-006), so there is
-- no recording column. The table is new and empty.
--
-- Append-only: the calls default privileges give the app role SELECT and INSERT only (D-1520,
-- TD-SEC-34); nothing grants UPDATE or DELETE.

CREATE TABLE calls.call_log (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  line_id uuid NOT NULL REFERENCES calls.number_pool (id),
  call_sid text NOT NULL UNIQUE CHECK (call_sid ~ '^CA[0-9a-zA-Z]{32}$'),
  caller text NOT NULL CHECK (caller IN ('business', 'homeowner')),
  answered_by text NOT NULL CHECK (answered_by IN ('person', 'machine', 'nobody')),
  length_seconds integer NOT NULL CHECK (length_seconds >= 0),
  reached_45s boolean NOT NULL CHECK (reached_45s = (length_seconds >= 45)),
  ended_at timestamptz NOT NULL
);
CREATE INDEX call_log_line_idx ON calls.call_log (line_id, ended_at);
