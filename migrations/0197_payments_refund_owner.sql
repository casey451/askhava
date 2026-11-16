-- payments (T529): payments.refund.trigger gains its own code for a refund the owner issues from
-- the owner portal's review step (T205), `owner` (FEE-026, SEC-006; D-3883). No code 0177's check
-- allowed fits it. The check is replaced by one that also allows `owner`; rows already stored keep
-- their code. As in 0177, the new check is added NOT VALID and then validated (P2); the migration
-- runs in one transaction and DROP CONSTRAINT takes an ACCESS EXCLUSIVE lock held to commit, so the
-- table is locked for the whole migration, the VALIDATE scan included; payments.refund is small,
-- so that lock is brief (D-2957). Grants are unchanged.

ALTER TABLE payments.refund DROP CONSTRAINT refund_trigger_check;

ALTER TABLE payments.refund ADD CONSTRAINT refund_trigger_check CHECK (trigger IN (
  'unreachable', 'cancelled-before-talk', 'charged-after-cancel', 'fraud-warning', 'late-success',
  'owner'
)) NOT VALID;

ALTER TABLE payments.refund VALIDATE CONSTRAINT refund_trigger_check;
