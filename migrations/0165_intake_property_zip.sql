-- intake (T251): the 5-digit ZIP of the home a job is at, stored on the property beside the town
-- (INT-049, PRIV-028, INT-048; D-1521, D-1519).
-- intake.property holds only the vault contact ID, the town and this ZIP: never a street address.
-- Rows written before the ZIP question existed keep no ZIP, so the column stays nullable. The
-- check is added NOT VALID and validated separately, so no long lock is held on a populated table.

ALTER TABLE intake.property ADD COLUMN zip text;

ALTER TABLE intake.property
  ADD CONSTRAINT property_zip_check CHECK (zip ~ '^[0-9]{5}$') NOT VALID;

ALTER TABLE intake.property VALIDATE CONSTRAINT property_zip_check;
