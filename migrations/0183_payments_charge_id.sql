-- payments (T359): the FEE-042 evidence record keeps the Stripe charge ID of the success it
-- records (FEE-042, D-2623). Nullable, since rows written before this task have none; every success
-- written since carries the `ch_` ID of the intent's `latest_charge`, which the success path also
-- writes to the pick's `charge` ledger entry. The table stays append-only for the app role: its
-- table-level SELECT and INSERT grants cover the new column and nothing grants UPDATE or DELETE.

ALTER TABLE payments.charge_evidence
  ADD COLUMN stripe_charge_id text CHECK (stripe_charge_id ~ '^ch_[A-Za-z0-9]{1,200}$');
