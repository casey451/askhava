-- businesses (T232): the Confirm page's business-terms acceptance (TERMS-033, TD-TERMS-11; D-492,
-- D-493, D-3055). businesses.terms_acceptance took only the sign-up's rows (0119); a Confirm press
-- now stores one row too, with channel confirm_page and the pick it confirmed. pick_id is a picks
-- record by ID only (no cross-schema foreign key, Arch 4 rule 3), set exactly on confirm_page rows,
-- and unique where set, so a second press for the same pick (after a decline, or a repeat) stores
-- nothing. Every existing row is a business_signup row with no pick, so both checks hold for it.
-- The table stays append-only for askhava_app (0119's REVOKE stands). The table is small and the
-- checks are added NOT VALID then validated (P2); the partial index is built in the migration's
-- transaction, as the table is small before launch (verify with host before Gate C).

ALTER TABLE businesses.terms_acceptance ADD COLUMN pick_id uuid;

ALTER TABLE businesses.terms_acceptance DROP CONSTRAINT terms_acceptance_channel_check;
ALTER TABLE businesses.terms_acceptance ADD CONSTRAINT terms_acceptance_channel_check
  CHECK (channel IN ('business_signup', 'confirm_page')) NOT VALID;
ALTER TABLE businesses.terms_acceptance VALIDATE CONSTRAINT terms_acceptance_channel_check;

ALTER TABLE businesses.terms_acceptance ADD CONSTRAINT terms_acceptance_pick_check
  CHECK ((channel = 'confirm_page') = (pick_id IS NOT NULL)) NOT VALID;
ALTER TABLE businesses.terms_acceptance VALIDATE CONSTRAINT terms_acceptance_pick_check;

CREATE UNIQUE INDEX terms_acceptance_pick_idx ON businesses.terms_acceptance (pick_id)
  WHERE pick_id IS NOT NULL;
