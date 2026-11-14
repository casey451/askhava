-- picks (T467): the 'fee_terms_changed' end of a pick (PICK-050, PICK-046, PICK-047, OPS-034,
-- FEE-005; D-1871 (3), D-3722). When the owner switched the fee mode after a pick and before its
-- Confirm, the waiting pick ends at once through the "Can't take it" path with this reason: no fee,
-- never counted against the business, and not a lapse. The check is replaced NOT VALID, then
-- validated, so no write waits on the scan; every stored reason already passes it.

ALTER TABLE picks.pick DROP CONSTRAINT pick_end_reason_check;
ALTER TABLE picks.pick ADD CONSTRAINT pick_end_reason_check
  CHECK (end_reason IN ('replaced', 'paused', 'ineligible', 'cant_take', 'fee_terms_changed'))
  NOT VALID;
ALTER TABLE picks.pick VALIDATE CONSTRAINT pick_end_reason_check;
