-- businesses (T193): the sign-up's "That number is old" wait and the one launch notice (BIZ-021,
-- BIZ-026, BIZ-031, D-178, D-254, D-312, D-493). waiting_on_record_since is set when the business
-- says its record contact is old; old_text_key, old_mail_key and old_letter_key are keyed digests
-- (never the values, PRIV-001) of the record's texting phone, email and mailing address it saw, so
-- the next daily list showing a different contact clears the wait and restarts the identity chain
-- at the first changed record contact. trades holds the
-- category codes chosen at sign-up, so the waitlist done screen can count businesses waiting for a
-- trade. launch_notice_sent_at marks BIZ-031's one email "requests in your trade start"; it is set
-- once and never cleared, so the notice goes once.

ALTER TABLE businesses.signup
  ADD COLUMN waiting_on_record_since timestamptz,
  ADD COLUMN old_text_key text CHECK (old_text_key ~ '^[0-9a-f]{64}$'),
  ADD COLUMN old_mail_key text CHECK (old_mail_key ~ '^[0-9a-f]{64}$'),
  ADD COLUMN old_letter_key text CHECK (old_letter_key ~ '^[0-9a-f]{64}$'),
  ADD COLUMN trades text[] NOT NULL DEFAULT '{}',
  ADD COLUMN launch_notice_sent_at timestamptz,
  ADD CONSTRAINT signup_waiting_has_key
    CHECK ((waiting_on_record_since IS NULL) = (old_text_key IS NULL)
      AND (waiting_on_record_since IS NULL) = (old_mail_key IS NULL)
      AND (waiting_on_record_since IS NULL) = (old_letter_key IS NULL));
CREATE INDEX signup_waiting_idx ON businesses.signup (business_id)
  WHERE waiting_on_record_since IS NOT NULL;
