-- picks (T214): picks.share_consent and the 'cant_take' end of a pick (PICK-049, PICK-050; D-443,
-- D-446, D-492, D-3032). picks.share_consent: each tick of the pick review step's share box
-- ("Share my first name and the job address with <Business> once they accept."), keyed like
-- picks.backup_choice: the job, the quote she was picking (`for_quote`), the browser that sent it
-- (`browser`: SHA256 of the page's CSRF token, never the token) and when. A row with no `pick_id`
-- is only the final Pick's tick; once the same browser's 6-digit code is verified a new row with
-- `pick_id` binds it to the pick in the transaction that stores the pick. Only an unticked box is
-- never stored: the final Pick refuses it. Quote numbers, hashes and IDs only, no contact value
-- (PRIV-002). Append-only for askhava_app; new and empty.

CREATE TABLE picks.share_consent (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  job_id uuid NOT NULL,
  for_quote integer NOT NULL CHECK (for_quote > 0),
  browser bytea NOT NULL CHECK (octet_length(browser) = 32),
  pick_id uuid REFERENCES picks.pick (id),
  set_at timestamptz NOT NULL
);
CREATE INDEX share_consent_job_idx ON picks.share_consent (job_id, for_quote, set_at);
CREATE INDEX share_consent_pick_idx ON picks.share_consent (pick_id)
  WHERE pick_id IS NOT NULL;
GRANT SELECT, INSERT ON picks.share_consent TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.share_consent FROM askhava_app;

-- "Can't take it" (PICK-050) cancels the waiting pick as 'cant_take', with no fee and not counted
-- against the business; the backup move (PICK-047) then runs on it.
ALTER TABLE picks.pick DROP CONSTRAINT pick_end_reason_check;
ALTER TABLE picks.pick ADD CONSTRAINT pick_end_reason_check
  CHECK (end_reason IN ('replaced', 'paused', 'ineligible', 'cant_take'));
