-- intake (T050): intake.homeowner_pause, a live job "paused by the homeowner" (CON-029, D-129).
-- A sole-word CANCEL or CANCELAR opt-out from a number with a live job writes one row per live job
-- of the open request, with the inbox row of that text; the job's status never changes (CON-012,
-- JOB-020, D-005). While a job has a row with no ended_at, no new pick, Confirm, charge or release
-- happens for it (T054, T224); the homeowner's pick or close on the quote page or by email ends it.
-- One open pause per job.
CREATE TABLE intake.homeowner_pause (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  conversation_id uuid NOT NULL REFERENCES intake.conversation (id),
  job_id uuid NOT NULL,
  inbox_id uuid NOT NULL,
  paused_at timestamptz NOT NULL,
  ended_at timestamptz CHECK (ended_at IS NULL OR ended_at >= paused_at)
);
CREATE UNIQUE INDEX homeowner_pause_open_idx ON intake.homeowner_pause (job_id)
  WHERE ended_at IS NULL;
