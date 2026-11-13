-- privacy (T277): the scrubbed words of an inbound text kept for a reason, here a check-in PROBLEM (JOB-016, TXT-020, PRIV-006; D-801, D-1840).
-- privacy.message_body keeps one row per kept inbound text: the `messaging.inbox` row it came in
-- as (unique, so a resend keeps one row), the job it answers, the scrubbed words only (never the
-- raw text, never a contact field; only `privacy::api::scrub` makes the words), whether they hold
-- a safety word, and when the row was made. Only Casey's owner session reads the words
-- (`problem_words`). Rows go 30 days after they are made (TXT-020, the daily
-- `privacy.cut_problem_words` step) and with the job's PRIV-008 text stage. The inbox row and the
-- job live in other schemas, so they are plain IDs with no foreign key (Arch 4 rule 3). The app
-- role reads, adds and deletes; it never updates. The table is new and empty, so nothing blocks.

CREATE TABLE privacy.message_body (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  inbox_id uuid NOT NULL UNIQUE,
  job_id uuid NOT NULL,
  words text NOT NULL,
  safety boolean NOT NULL,
  created_at timestamptz NOT NULL
);

CREATE INDEX message_body_job_idx ON privacy.message_body (job_id);
CREATE INDEX message_body_created_idx ON privacy.message_body (created_at);

GRANT SELECT, INSERT, DELETE ON privacy.message_body TO askhava_app;
REVOKE UPDATE, TRUNCATE ON privacy.message_body FROM askhava_app;
