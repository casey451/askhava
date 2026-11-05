-- payments (T066): the one refund of a pick and bank disputes (FEE-026, FEE-027, FEE-042, FEE-050,
-- PRIV-027, D-1495, D-1542 to D-1544). payments.refund is a pick's one full refund (primary key
-- pick_id, so a refund happens once): its trigger, its status (sending, failed, succeeded, blocked on
-- a disputed charge), the try count each Stripe call is keyed by, and the re_… ID once Stripe took it.
-- payments.dispute is one row per Stripe dispute (unique dp_… ID): the charged pick, the disputed
-- amount, Stripe's evidence due date, whether T191 marked it for auto-submit (FEE-050's condition,
-- read from picks and calls) and when and by whom the evidence went in. IDs and timestamps only
-- (PRIV-027). Both name a pick through its FEE-042 evidence record, so only a charged pick has
-- either. The tables are new and empty. The schema default gives the app role SELECT and INSERT
-- (D-1495); the status moves below are explicit column grants (TD-SEC-33), and nothing is deleted.

CREATE TABLE payments.refund (
  pick_id uuid PRIMARY KEY REFERENCES payments.charge_evidence (pick_id),
  trigger text NOT NULL CHECK (trigger IN (
    'unreachable', 'cancelled-before-talk', 'charged-after-cancel', 'fraud-warning'
  )),
  status text NOT NULL CHECK (status IN ('sending', 'failed', 'succeeded', 'blocked')),
  tries integer NOT NULL CHECK (tries >= 0),
  stripe_refund_id text CHECK (stripe_refund_id ~ '^re_[A-Za-z0-9]{1,200}$'),
  requested_at timestamptz NOT NULL,
  decided_at timestamptz
);

CREATE TABLE payments.dispute (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  stripe_dispute_id text NOT NULL UNIQUE CHECK (stripe_dispute_id ~ '^(dp|du)_[A-Za-z0-9]{1,200}$'),
  pick_id uuid NOT NULL REFERENCES payments.charge_evidence (pick_id),
  amount_cents bigint NOT NULL CHECK (amount_cents > 0),
  due_at timestamptz NOT NULL,
  opened_at timestamptz NOT NULL,
  auto_submit boolean NOT NULL DEFAULT false,
  submitted_by text CHECK (submitted_by IN ('auto', 'owner')),
  submitted_at timestamptz,
  CHECK ((submitted_by IS NULL) = (submitted_at IS NULL))
);
CREATE INDEX dispute_pick_idx ON payments.dispute (pick_id);

GRANT UPDATE (status, tries, stripe_refund_id, decided_at) ON payments.refund TO askhava_app;
GRANT UPDATE (auto_submit, submitted_by, submitted_at) ON payments.dispute TO askhava_app;
