-- picks (T341): the package a homeowner chose on a tiers quote (PICK-053, FEE-001; D-2332).
-- `tier_index` is the package's place on the quote (0 for the first, at most 3 packages) and
-- `tier_name` its name as the quote showed it, both written in the pick's own transaction, both
-- or neither; a pick of any other format has neither. The fee is fixed from that package's price.
-- No contact value (PRIV-002). The columns are new, so every existing row has neither.

ALTER TABLE picks.pick
  ADD COLUMN tier_index smallint CHECK (tier_index BETWEEN 0 AND 2),
  ADD COLUMN tier_name text,
  ADD CONSTRAINT pick_tier_both CHECK ((tier_index IS NULL) = (tier_name IS NULL));
