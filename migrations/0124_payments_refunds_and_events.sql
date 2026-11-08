-- payments (T258): a refund is recorded as Stripe reports it, and the webhook's event record
-- (FEE-026, FEE-028, FEE-029, FEE-030, SEC-017, D-1753, D-1754, D-2067). payments.refund's status
-- gains `pending` (Stripe answered `pending` or `requires_action`: the re_… is stored, nothing is in
-- the ledger yet) and `canceled` (Stripe canceled it; retried hourly like `failed`). The ledger's
-- kind check gains `refund-reversal` (a refund Stripe failed or canceled after it succeeded, the
-- same amount back, FEE-028 as changed by D-1753); it is above 0 and names its business and job like
-- `refund`. Both checks are replaced NOT VALID and then validated, so no write waits on a full scan
-- under a lock (postgres brain P2). payments.webhook_event is one row per Stripe event ID (unique),
-- its type and when it arrived, written by T259's dispatch in the event's own transaction so a
-- repeat changes nothing (SEC-017); IDs and a type only, no personal data. It is new and empty; the
-- schema default gives the app role SELECT and INSERT only (D-1495), and it is append-only.

ALTER TABLE payments.refund DROP CONSTRAINT refund_status_check;
ALTER TABLE payments.refund
  ADD CONSTRAINT refund_status_check
    CHECK (status IN ('sending', 'failed', 'succeeded', 'blocked', 'pending', 'canceled')) NOT VALID;
ALTER TABLE payments.refund VALIDATE CONSTRAINT refund_status_check;

ALTER TABLE payments.ledger_entry DROP CONSTRAINT ledger_entry_kind_check;
ALTER TABLE payments.ledger_entry
  ADD CONSTRAINT ledger_entry_kind_check CHECK (kind IN (
    'charge', 'refund', 'credit', 'free-first-win', 'repeat-customer', 'fee-free-job-type',
    'dispute', 'countering-fee', 'countering-fee-return', 'processing-fee', 'payout',
    'refund-reversal'
  )) NOT VALID;
ALTER TABLE payments.ledger_entry VALIDATE CONSTRAINT ledger_entry_kind_check;

CREATE TABLE payments.webhook_event (
  stripe_event_id text PRIMARY KEY CHECK (stripe_event_id ~ '^evt_[A-Za-z0-9]{1,200}$'),
  event_type text NOT NULL CHECK (event_type ~ '^[a-z_]+(\.[a-z_]+){1,4}$'),
  received_at timestamptz NOT NULL
);

GRANT SELECT, INSERT ON payments.webhook_event TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON payments.webhook_event FROM askhava_app;
