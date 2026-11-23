-- messaging (T645): a message typed in the web chat once its phone code is verified is a way to
-- revoke texting consent (CON-035, D-4292), logged with channel `web_chat` (CON-022). Widens the
-- channel check of 0044_messaging_optout.sql; no later migration copied it.
ALTER TABLE messaging.optout_event DROP CONSTRAINT optout_event_channel_check;
ALTER TABLE messaging.optout_event
  ADD CONSTRAINT optout_event_channel_check CHECK (channel IN (
    'text', 'email', 'email_reply', 'help_form', 'portal', 'owner', 'carrier', 'web_chat'
  ));
