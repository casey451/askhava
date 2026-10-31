-- businesses (T182): a business's own quiet times (BIZ-010, D-085, D-968). Two nullable columns
-- on businesses.business hold the business's own notice window as minutes after midnight, Arizona
-- time (QH-002): the first minute inside and the first minute outside. Both are NULL (no own quiet
-- times, so the app-wide 8 a.m. to 8 p.m. window applies) or both are set, and the check keeps the
-- window inside 8:00 (480) to 20:00 (1200) and non-empty, so it can only narrow the app-wide window
-- (QH-001), never widen it. No contact value is added (PRIV-001). Existing rows get NULL.

ALTER TABLE businesses.business
  ADD COLUMN own_open_minute smallint,
  ADD COLUMN own_close_minute smallint,
  ADD CONSTRAINT business_own_hours_check CHECK (
    (own_open_minute IS NULL) = (own_close_minute IS NULL)
    AND (own_open_minute IS NULL
         OR (own_open_minute >= 480 AND own_open_minute < own_close_minute
             AND own_close_minute <= 1200))
  );
