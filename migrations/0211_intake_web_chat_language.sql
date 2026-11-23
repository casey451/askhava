-- intake (T645): the web chat keeps its first message's language code, never its words
-- (TXT-002, TXT-041, PRIV-008, D-4292): `language` is `wanted_language`'s `en` or `es`, NULL
-- before the first message. The app role may delete a session: an under-13 stop deletes it in the
-- same transaction as the request's purge, so nothing of the session remains (INT-038, D-4726).
ALTER TABLE intake.web_chat_session
  ADD COLUMN language text CHECK (language IN ('en', 'es'));
GRANT DELETE ON intake.web_chat_session TO askhava_app;
