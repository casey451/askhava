-- businesses (T373): text consent is per number, not per phone line (CON-019, BIZ-036; D-191,
-- D-2707). businesses.staff_phone.number_version counts the numbers a line has had: 1 at link,
-- raised by one in team::change_phone_at's transaction, under the line's row lock. Each text row
-- of businesses.esign_consent carries the number_version its box was ticked for (an email row has
-- none), and the once-per-box unique index includes it, so the new number can get its own box.
-- consent::may_text counts only a box ticked for the line's current version: after a number
-- change the phone gets no text until a member ticks the box again, and a text-only business gets
-- its notices by email meanwhile (BIZ-036's email default). Old rows stay as history. Every
-- existing line and text consent row is version 1. The Rust path (consent::record_consent) sets
-- the version itself; the trigger below only fills a text row that plain SQL (done tests' seeds)
-- left without one with its line's current version, as T361's 0202 does. No row exists outside a test database before
-- Gate C, so the columns, backfill and index run in this migration's one transaction (P2).
-- Grants are unchanged: the app role updates staff_phone (migration 0056) and still only selects
-- and inserts esign_consent rows (migration 0121). No contact value is stored (PRIV-001).

ALTER TABLE businesses.staff_phone
  ADD COLUMN number_version integer NOT NULL DEFAULT 1 CHECK (number_version >= 1);

ALTER TABLE businesses.esign_consent ADD COLUMN number_version integer
  CHECK (number_version >= 1);

UPDATE businesses.esign_consent SET number_version = 1 WHERE channel = 'text';

ALTER TABLE businesses.esign_consent ADD CONSTRAINT esign_consent_text_has_number_version
  CHECK ((channel = 'text') = (number_version IS NOT NULL));

DROP INDEX businesses.esign_consent_once_idx;
CREATE UNIQUE INDEX esign_consent_once_idx ON businesses.esign_consent
  (business_id, member_id, channel, line_id, number_version, wording_version) NULLS NOT DISTINCT;

-- A text row inserted with no number version gets its line's current one (the version the
-- current-version record_consent form would store); a given version is kept as it is.
CREATE FUNCTION businesses.esign_consent_number_version() RETURNS trigger
  LANGUAGE plpgsql SET search_path = businesses, pg_temp AS $$
BEGIN
  IF NEW.channel = 'text' AND NEW.number_version IS NULL THEN
    NEW.number_version := (SELECT number_version FROM businesses.staff_phone
                            WHERE id = NEW.line_id AND business_id = NEW.business_id);
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER esign_consent_number_version BEFORE INSERT ON businesses.esign_consent
  FOR EACH ROW EXECUTE FUNCTION businesses.esign_consent_number_version();
