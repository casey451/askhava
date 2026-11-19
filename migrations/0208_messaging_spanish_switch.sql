-- messaging (T480): the owner-portal "Spanish conversations" switch with the records it rests on
-- (TXT-039, TXT-043; D-427, D-490, D-3753, D-3844).
-- messaging.spanish_switch is append-only: each change Casey makes on the owner settings page is
-- a new row and the newest (changed_at, then id) is the one in force; no row means off, the
-- launch state (TXT-039). An "on" row holds the paid professional translation review of every
-- Spanish template (its date, the reviewer, and `reviewed`, a JSON object of each Spanish
-- template ID to its version: the lowercase hex SHA256 of its Spanish text, D-3844), the
-- campaign note's date and reference (the Console update saying texts are in English or Spanish,
-- T113's document) and the Spanish sample message on file (TXT-043). `messaging::api` stores an
-- "on" row only when the review covers every Spanish template at its current version and the note
-- and a non-empty sample are given; an "off" row holds none of them. `changed_by` is the owner
-- record's ID. No homeowner detail. The messaging default privileges give the app role SELECT and
-- INSERT only (0110); UPDATE, DELETE and TRUNCATE are revoked here as well. The table is new.

CREATE TABLE messaging.spanish_switch (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  is_on boolean NOT NULL,
  reviewed_on date,
  reviewer text CHECK (reviewer IS NULL OR length(btrim(reviewer)) BETWEEN 1 AND 200),
  reviewed jsonb CHECK (reviewed IS NULL OR jsonb_typeof(reviewed) = 'object'),
  note_on date,
  note_reference text
    CHECK (note_reference IS NULL OR length(btrim(note_reference)) BETWEEN 1 AND 200),
  sample text CHECK (sample IS NULL OR length(btrim(sample)) BETWEEN 1 AND 1600),
  changed_by uuid NOT NULL,
  changed_at timestamptz NOT NULL,
  CHECK (is_on = (reviewed_on IS NOT NULL AND reviewer IS NOT NULL AND reviewed IS NOT NULL)),
  CHECK (is_on = (note_on IS NOT NULL AND note_reference IS NOT NULL AND sample IS NOT NULL))
);
CREATE INDEX spanish_switch_latest_idx ON messaging.spanish_switch (changed_at DESC, id);

GRANT SELECT, INSERT ON messaging.spanish_switch TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON messaging.spanish_switch FROM askhava_app;
