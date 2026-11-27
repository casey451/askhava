-- picks (T744): the 'job_final' end of a pick (FEE-021, PICK-038; D-4748 (4), D-5664). When a job
-- reaches a final status, its `jobs.final_status` handler ends every pick still awaiting Confirm or
-- in Payment pending through `end_pending` with this reason, which also cancels its PaymentIntent.
-- The check is replaced NOT VALID, then validated, so no write waits on the scan; every stored
-- reason already passes it.

ALTER TABLE picks.pick DROP CONSTRAINT pick_end_reason_check;
ALTER TABLE picks.pick ADD CONSTRAINT pick_end_reason_check
  CHECK (end_reason IN (
    'replaced', 'paused', 'ineligible', 'cant_take', 'fee_terms_changed', 'job_final'
  ))
  NOT VALID;
ALTER TABLE picks.pick VALIDATE CONSTRAINT pick_end_reason_check;
