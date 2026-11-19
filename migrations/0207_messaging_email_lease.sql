-- messaging (T470): the queued-email drain's lease and the dropped status (SEC-018, SEC-025,
-- OPS-038, BIZ-051, SEC-024; D-3750, D-3889, D-4196, D-4327).
-- messaging.email_outbox.claimed_at is when the `messaging.email_due` drain leased the row (it
-- sets `sending` and claimed_at together; a lease older than 5 minutes may be taken again).
-- lease_count counts the unsure or setup answers in a row and only sets the back-off (1 minute
-- doubling to 60); first_unsure_at is when the first call with an unknown outcome began, set once
-- and never cleared, so a row is never handed to the provider 23 hours after it (the provider keeps
-- an idempotency key 24 hours). The status `dropped` marks a row never to be sent (a refusal, a
-- marked body, a removed recipient, a give-up or an expiry); its subject and bodies are cleared
-- in the same statement, as a sent row's are. Existing rows get NULL, 0 and keep their status.
-- Grants are unchanged: the app role already updates the table (migration 0089). No contact value
-- is stored (PRIV-002).

ALTER TABLE messaging.email_outbox
  ADD COLUMN claimed_at timestamptz,
  ADD COLUMN lease_count smallint NOT NULL DEFAULT 0 CHECK (lease_count >= 0),
  ADD COLUMN first_unsure_at timestamptz;

ALTER TABLE messaging.email_outbox DROP CONSTRAINT email_outbox_status_check;
ALTER TABLE messaging.email_outbox DROP CONSTRAINT email_outbox_check1;
ALTER TABLE messaging.email_outbox DROP CONSTRAINT email_outbox_check2;
ALTER TABLE messaging.email_outbox
  ADD CONSTRAINT email_outbox_status_check
    CHECK (status IN ('queued', 'sending', 'sent', 'dropped')),
  ADD CONSTRAINT email_outbox_parts_check CHECK (status IN ('sent', 'dropped')
    OR (subject IS NOT NULL AND html IS NOT NULL AND plain IS NOT NULL)),
  ADD CONSTRAINT email_outbox_cleared_check CHECK (status NOT IN ('sent', 'dropped')
    OR (subject IS NULL AND html IS NULL AND plain IS NULL));

DROP INDEX messaging.email_outbox_due_idx;
CREATE INDEX email_outbox_due_idx ON messaging.email_outbox (not_before)
  WHERE status IN ('queued', 'sending');
