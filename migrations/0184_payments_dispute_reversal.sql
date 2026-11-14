-- payments (T353): a won dispute's returned amount (FEE-027, FEE-028, FEE-030, FEE-050, D-2625).
-- The ledger's kind check gains `dispute-reversal`: on a `charge.dispute.closed` with status `won`
-- the webhook appends one, of the disputed amount without Stripe's $15 dispute fee, naming the
-- dispute's `dp_…`, beside one `countering-fee-return` of the recorded countering fee. It is above
-- 0 and names its business and job like `dispute`. The check is replaced NOT VALID and then
-- validated, so no write waits on a full scan under a lock (postgres brain P2).

ALTER TABLE payments.ledger_entry DROP CONSTRAINT ledger_entry_kind_check;
ALTER TABLE payments.ledger_entry
  ADD CONSTRAINT ledger_entry_kind_check CHECK (kind IN (
    'charge', 'refund', 'credit', 'free-first-win', 'repeat-customer', 'fee-free-job-type',
    'dispute', 'countering-fee', 'countering-fee-return', 'processing-fee', 'payout',
    'refund-reversal', 'dispute-reversal'
  )) NOT VALID;
ALTER TABLE payments.ledger_entry VALIDATE CONSTRAINT ledger_entry_kind_check;
