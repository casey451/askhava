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
