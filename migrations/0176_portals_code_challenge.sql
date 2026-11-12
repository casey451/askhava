-- portals (T346): the local challenge that binds each homeowner Verify code to its purpose and
-- subject, with its try count (SEC-055, SEC-008, PRIV-031; D-2348, D-2349).
-- One Twilio Verify service serves every purpose and a check names only the number, so a code
-- counts here only against the newest unused, unexpired challenge of its own purpose
-- (homeowner_signin: her contact; data_copy or data_delete: the request). A send while that
-- challenge is alive reuses it (tries and sent time kept); a check reserves a try with one guarded
-- UPDATE before Verify is called, at most 5; a right code sets used_at once, in the transaction
-- of the action it confirms. IDs and times only, never a phone, code or name. The table is new
-- and empty.

CREATE TABLE portals.code_challenge (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  purpose text NOT NULL CHECK (purpose IN ('homeowner_signin', 'data_copy', 'data_delete')),
  subject_id uuid NOT NULL,
  sent_at timestamptz NOT NULL,
  used_at timestamptz,
  tries integer NOT NULL DEFAULT 0 CHECK (tries >= 0)
);

-- The newest challenge of a purpose and subject.
CREATE INDEX code_challenge_subject ON portals.code_challenge (purpose, subject_id, id DESC);

-- The app reads and opens challenges and updates only the try count and the used time; nothing
-- is deleted by the app. The schema's default privileges grant table-wide UPDATE and DELETE, so
-- both are revoked first and UPDATE is granted back on the two columns alone.
REVOKE UPDATE, DELETE, TRUNCATE ON portals.code_challenge FROM askhava_app;
GRANT SELECT, INSERT ON portals.code_challenge TO askhava_app;
GRANT UPDATE (tries, used_at) ON portals.code_challenge TO askhava_app;
