-- picks (T456): the stored Confirm end and the Away-delayed mark (PICK-019, PICK-043, BIZ-034;
-- D-285, D-3675, D-3890). picks.pick.confirm_due is the end of the pick's business-hour Confirm
-- window: written when the window starts at the notice's send (handlers::start_window) and moved
-- once by a HOLD (handlers::hold_pick); NULL before the send and for an emergency job, whose 60
-- clock minutes stay jobs' own (PICK-044). picks.pick.away_delayed marks a pick whose Confirm clock
-- started late because the business was away (BIZ-034, written by T543); a HOLD on such a pick is
-- refused (PICK-043). Existing rows get NULL and false: before Gate C no row exists outside a
-- test database. Grants are unchanged: the app role already updates picks.pick (migration 0079).
-- No contact value is stored (PRIV-002).

ALTER TABLE picks.pick
  ADD COLUMN confirm_due timestamptz,
  ADD COLUMN away_delayed boolean NOT NULL DEFAULT false;
