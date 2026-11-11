-- quotes (T164): the visit schedule of a per-visit price (REC-010, PICK-053; D-2333, D-1407).
-- A recurring quote priced per visit must say how often the visits come, one of the five
-- frequencies payments::rules::Period converts to a monthly amount (weekly, every two weeks,
-- monthly, quarterly, yearly); the quote form requires it. Any other schedule is entered as a
-- monthly price ("Other: enter the monthly amount"), so it has no schedule here. A schedule is
-- kept only beside period 'visit'. Existing rows stay NULL. The app role already updates
-- quotes.quote (migration 0070); nothing is deleted. No homeowner detail.

ALTER TABLE quotes.quote
  ADD COLUMN visit_schedule text
    CHECK (visit_schedule IN ('weekly', 'every_two_weeks', 'monthly', 'quarterly', 'yearly')),
  ADD CONSTRAINT quote_visit_schedule_per_visit
    CHECK (visit_schedule IS NULL OR period = 'visit');

-- A quote saved again (every upsert sets period) drops its old schedule, whichever path wrote it,
-- so an edit through quotes::intake::submit_quote never keeps a stale one; save_quote sets the new
-- schedule after the upsert in its own UPDATE of visit_schedule alone, which this does not fire on.
-- CHECK constraints run after this, so an edit away from a per-visit price never fails on them.
CREATE FUNCTION quotes.quote_schedule_resets() RETURNS trigger
  LANGUAGE plpgsql SET search_path = quotes, pg_temp AS $$
BEGIN
  NEW.visit_schedule := NULL;
  RETURN NEW;
END
$$;

CREATE TRIGGER quote_schedule_resets BEFORE UPDATE OF period ON quotes.quote
  FOR EACH ROW EXECUTE FUNCTION quotes.quote_schedule_resets();
