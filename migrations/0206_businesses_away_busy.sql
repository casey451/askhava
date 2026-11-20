-- businesses (T543): the business's "Away until" end and its "Busy until" end (BIZ-034, BIZ-028;
-- D-285, D-309, D-3890). businesses.business.away_until is the end of the business's away time,
-- set by the owner or a "Confirm and billing" member through away_busy::set_away after the caller
-- bounded it with picks::api::away_until (at most the setting's business hours ahead); a time in
-- the past reads as not away. A non-emergency pick notice sent before it starts its Confirm clock
-- at it (picks::handlers::notice_sent, picks.pick.away_delayed). businesses.business.busy_until is
-- T463's "Busy for <n> days" end; this migration only adds the column. Both are NULL when unset;
-- existing rows get NULL. Grants are unchanged: the app role already updates businesses.business
-- (migration 0055). No contact value is stored (PRIV-001).

ALTER TABLE businesses.business
  ADD COLUMN away_until timestamptz,
  ADD COLUMN busy_until timestamptz;
