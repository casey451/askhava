-- businesses (T193): the sign-up's "That number is old" wait and the one launch notice (BIZ-021,
-- BIZ-026, BIZ-031, D-178, D-254, D-312, D-493). waiting_on_record_since is set when the business
-- says its record contact is old; old_contact_key is the keyed digest (never the value, PRIV-001)
-- of the record contact it said was old, so the next daily list showing a different contact
-- clears the wait and restarts the identity chain on the new record contact. trades holds the
-- category codes chosen at sign-up, so the waitlist done screen can count businesses waiting for a
-- trade. launch_notice_sent_at marks BIZ-031's one email "requests in your trade start"; it is set
-- once and never cleared, so the notice goes once.

ALTER TABLE businesses.signup
  ADD COLUMN waiting_on_record_since timestamptz,
  ADD COLUMN old_contact_key text CHECK (old_contact_key ~ '^[0-9a-f]{64}$'),
  ADD COLUMN trades text[] NOT NULL DEFAULT '{}',
  ADD COLUMN launch_notice_sent_at timestamptz,
  ADD CONSTRAINT signup_waiting_has_key
    CHECK ((waiting_on_record_since IS NULL) = (old_contact_key IS NULL));
CREATE INDEX signup_waiting_idx ON businesses.signup (business_id)
  WHERE waiting_on_record_since IS NOT NULL;
