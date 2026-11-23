-- licensing (T653): a BTR number a business types at sign-up is stored unchecked (D-4310; LIC-013,
-- LIC-015, LIC-034, CARD-015). No BTR lookup exists yet, so a typed inspector or alarm number is
-- `typed_unchecked`: the holder does not count it as an agency license (no card) and the quote card
-- prints no LIC-034 "showed Active ... when we checked" line for it. `agency_record` stays for a
-- number read from the agency's own source. Only home-inspector and alarm-business rows may be
-- `typed_unchecked`; the pest and septage-hauler rules are unchanged.

ALTER TABLE licensing.business_license DROP CONSTRAINT business_license_verification_check;
ALTER TABLE licensing.business_license
  ADD CONSTRAINT business_license_verification_known
  CHECK (verification IN ('self_attested', 'agency_record', 'typed_unchecked'));
ALTER TABLE licensing.business_license
  ADD CONSTRAINT business_license_typed_unchecked_btr_only
  CHECK (verification <> 'typed_unchecked' OR agency IN ('home_inspector', 'alarm_business'));

-- Only sign-up ever wrote home-inspector and alarm-business rows (no BTR source exists), so every
-- such `agency_record` row is a typed number nobody checked.
UPDATE licensing.business_license SET verification = 'typed_unchecked'
WHERE agency IN ('home_inspector', 'alarm_business') AND verification = 'agency_record';
