-- picks (T157): picks.backup_choice and picks.backup_move (PICK-047, PICK-046; D-400, D-446).
-- picks.backup_choice: each backup the homeowner named on the quote page's pick review step, by
-- job, for the quote she was picking (`for_quote`), the quote to go to (`backup_quote`; NULL is
-- "No backup") and when; the latest row of the job counts, so a change or a removal is a new row.
-- picks.backup_move: once per ended pick, whether it moved to the backup and the new pick, so a
-- rerun moves nothing twice. Only quote numbers and IDs, no contact value (PRIV-002). Both are
-- append-only for askhava_app; both are new and empty.

CREATE TABLE picks.backup_choice (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid NOT NULL,
  for_quote integer NOT NULL CHECK (for_quote > 0),
  backup_quote integer CHECK (backup_quote > 0 AND backup_quote <> for_quote),
  set_at timestamptz NOT NULL
);
CREATE INDEX backup_choice_job_idx ON picks.backup_choice (job_id, set_at);
GRANT SELECT, INSERT ON picks.backup_choice TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.backup_choice FROM askhava_app;

CREATE TABLE picks.backup_move (
  pick_id uuid PRIMARY KEY REFERENCES picks.pick (id),
  new_pick_id uuid REFERENCES picks.pick (id),
  decided_at timestamptz NOT NULL
);
GRANT SELECT, INSERT ON picks.backup_move TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.backup_move FROM askhava_app;

-- A Confirm refused by the PICK-020 re-check cancels the pick as 'ineligible' before the move.
ALTER TABLE picks.pick DROP CONSTRAINT pick_end_reason_check;
ALTER TABLE picks.pick ADD CONSTRAINT pick_end_reason_check
  CHECK (end_reason IN ('replaced', 'paused', 'ineligible'));
