-- payments (T332): payments.refund.trigger gains its own code for a late success, `late-success`
-- (FEE-021, FEE-026; D-1547, D-2290). Until now a payment that succeeded after its job moved on
-- was refunded under `charged-after-cancel`, the only code 0102's check allowed for it. The check
-- is replaced by one that also allows `late-success`; rows already stored keep their code. The new
-- check is added NOT VALID and then validated (P2), but the migration runs in one transaction and
-- DROP CONSTRAINT takes an ACCESS EXCLUSIVE lock held to commit, so the table is locked for the
-- whole migration, the VALIDATE scan included; payments.refund is small, so that lock is brief
-- (D-2957). Grants are unchanged.

ALTER TABLE payments.refund DROP CONSTRAINT refund_trigger_check;

ALTER TABLE payments.refund ADD CONSTRAINT refund_trigger_check CHECK (trigger IN (
  'unreachable', 'cancelled-before-talk', 'charged-after-cancel', 'fraud-warning', 'late-success'
)) NOT VALID;

ALTER TABLE payments.refund VALIDATE CONSTRAINT refund_trigger_check;
