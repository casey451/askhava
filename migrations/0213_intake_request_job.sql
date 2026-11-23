-- intake (T745): one row per job a request opened, kept after the request closes (D-4748 (5a),
-- D-5553; CARD-023, FEE-013, FEE-036, PRIV-011). A request's later jobs (an INT-032 ALSO or SPLIT
-- reply) lived only in `job.%` answer rows, which a close drops, so `jobs_of_contact` missed them.
-- `models::set_first_job` writes the row in the job-opening transaction. It holds only IDs and a
-- time, like the conversation row it hangs on, which is never deleted (a purge keeps it as the
-- consent record's anchor), so the app role gets SELECT, INSERT only (migration 0110's default).
CREATE TABLE intake.request_job (
  conversation_id uuid NOT NULL REFERENCES intake.conversation (id),
  job_id uuid NOT NULL,
  opened_at timestamptz NOT NULL,
  PRIMARY KEY (conversation_id, job_id)
);
CREATE INDEX request_job_job_idx ON intake.request_job (job_id);

INSERT INTO intake.request_job (conversation_id, job_id, opened_at)
SELECT c.id, c.job_id, c.opened_at FROM intake.conversation c WHERE c.job_id IS NOT NULL
UNION ALL
SELECT a.conversation_id, a.choice::uuid, a.answered_at FROM intake.intake_answer a
WHERE a.question_id LIKE 'job.%' AND a.choice ~ '^[0-9a-f]{32}$'
ON CONFLICT (conversation_id, job_id) DO NOTHING;
