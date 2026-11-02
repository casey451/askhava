-- payments (T059): Stripe customers, the FEE-015 card authorization and saved cards
-- (FEE-009, FEE-014, FEE-015, FEE-036, FEE-048, D-073, D-089, D-365). payments.customer holds one
-- Stripe customer per business, created at sign-up with no card (FEE-036); it has no homeowner
-- column, so no homeowner can ever be a Stripe customer (FEE-009). payments.card_authorization is
-- append-only: each acceptance of the authorization keeps its version, its exact text and the time
-- (FEE-015). payments.payment_method is a saved card: only the Stripe PaymentMethod ID, brand and
-- last four, never card data (FEE-014); `method_type` must be 'card' (cards only, D-073) and `usage`
-- 'off_session' (FEE-048); `removed_at` is set when the card is detached (BIZ-051). The business is
-- in another schema, so no foreign key to it (architecture section 4 rule 3). All tables are new
-- and empty.

CREATE TABLE payments.customer (
  business_id uuid PRIMARY KEY,
  stripe_customer_id text NOT NULL UNIQUE CHECK (stripe_customer_id ~ '^cus_[A-Za-z0-9_]{1,200}$'),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE payments.card_authorization (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL REFERENCES payments.customer (business_id),
  version integer NOT NULL CHECK (version > 0),
  text text NOT NULL CHECK (length(text) BETWEEN 1 AND 2000),
  accepted_at timestamptz NOT NULL
);
CREATE INDEX card_authorization_business_idx ON payments.card_authorization (business_id, accepted_at);

CREATE TABLE payments.payment_method (
  stripe_payment_method_id text PRIMARY KEY CHECK (stripe_payment_method_id ~ '^pm_[A-Za-z0-9_]{1,200}$'),
  business_id uuid NOT NULL REFERENCES payments.customer (business_id),
  authorization_id uuid NOT NULL REFERENCES payments.card_authorization (id),
  method_type text NOT NULL CHECK (method_type = 'card'),
  usage text NOT NULL CHECK (usage = 'off_session'),
  brand text NOT NULL CHECK (brand ~ '^[a-z_]{1,30}$'),
  last4 text NOT NULL CHECK (last4 ~ '^[0-9]{4}$'),
  saved_at timestamptz NOT NULL,
  removed_at timestamptz CHECK (removed_at IS NULL OR removed_at >= saved_at)
);
CREATE INDEX payment_method_business_idx ON payments.payment_method (business_id)
  WHERE removed_at IS NULL;

GRANT SELECT, INSERT ON payments.customer TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON payments.customer FROM askhava_app;
GRANT SELECT, INSERT ON payments.card_authorization TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON payments.card_authorization FROM askhava_app;
GRANT SELECT, INSERT, UPDATE ON payments.payment_method TO askhava_app;
REVOKE DELETE, TRUNCATE ON payments.payment_method FROM askhava_app;
