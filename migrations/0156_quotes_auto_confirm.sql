-- quotes (T209): the optional "Auto-confirm and pay if picked" tick on a quote (PICK-048, PICK-056,
-- FEE-014, SEC-038; D-401, D-480, D-491). Stripe's written answer is `catalog`'s, in
-- 0159_catalog_stripe_answer.sql (a migration names only its own schema, D-2783).
-- quotes.auto_confirm keeps every tick of a quote's box: the member who ticked it, the fee shown and
-- the authorization version and wording version they agreed to, and the saved card it was ticked
-- with; an untick sets unticked_at (by the business, or `void` when picks found the card, the fee,
-- the version or the member changed). At most one live tick per quote. picks reads it through
-- quotes::api at the pick. The app role inserts and reads it and may only stamp the untick; nothing
-- is deleted. businesses, payments and picks are other schemas, so no foreign key there
-- (architecture section 4 rule 3). The table is new.

CREATE TABLE quotes.auto_confirm (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  quote_id uuid NOT NULL REFERENCES quotes.quote (id),
  member_id uuid NOT NULL,
  fee_cents bigint NOT NULL CHECK (fee_cents > 0),
  authorization_version integer NOT NULL CHECK (authorization_version >= 1),
  payment_method text NOT NULL CHECK (payment_method ~ '^pm_[A-Za-z0-9_]{1,200}$'),
  ticked_at timestamptz NOT NULL,
  unticked_at timestamptz CHECK (unticked_at IS NULL OR unticked_at >= ticked_at),
  untick_reason text CHECK (untick_reason IN ('business', 'void')),
  CHECK ((unticked_at IS NULL) = (untick_reason IS NULL))
);
CREATE UNIQUE INDEX auto_confirm_live_idx ON quotes.auto_confirm (quote_id)
  WHERE unticked_at IS NULL;

REVOKE UPDATE, DELETE, TRUNCATE ON quotes.auto_confirm FROM askhava_app;
GRANT SELECT, INSERT ON quotes.auto_confirm TO askhava_app;
GRANT UPDATE (unticked_at, untick_reason) ON quotes.auto_confirm TO askhava_app;
