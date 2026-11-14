-- picks (T341): the package a homeowner chose on a tiers quote (PICK-053, FEE-001; D-2332).
-- `tier_index` is the package's place on the quote (0 for the first, at most 3 packages) and
-- `tier_name` and `tier_cents` its name and price as the quote showed them (D-3575), all written
-- in the pick's own transaction, all or none; a pick of any other format has none. The fee is
-- fixed from that package's price.
-- No contact value (PRIV-002). The columns are new, so every existing row has none.

ALTER TABLE picks.pick
  ADD COLUMN tier_index smallint CHECK (tier_index BETWEEN 0 AND 2),
  ADD COLUMN tier_name text,
  ADD COLUMN tier_cents bigint CHECK (tier_cents >= 0),
  ADD CONSTRAINT pick_tier_both CHECK (
    (tier_index IS NULL) = (tier_name IS NULL) AND (tier_index IS NULL) = (tier_cents IS NULL)
  );
