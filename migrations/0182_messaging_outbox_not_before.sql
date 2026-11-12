-- messaging (T356): the outbox sender's not-before time, send fence and two new end states
-- (QH-001, QH-004, QH-006, QH-008, TXT-011, TXT-014, TXT-021, CON-009, CON-030; D-558, D-2188, D-2628).
-- `not_before` is when the sender may next try the row: NULL means at once; a quiet-hours hold or a
-- TXT-014 retry sets it. `quiet_exempt_until` is the QH-004 exemption decided when the row was
-- queued: `infinity` for an any-hour kind or an emergency job's text, the person's inbound time plus
-- 15 minutes for a direct reply, NULL otherwise. `sending_since` is when the row was claimed: a
-- claim takes only a `queued` row, so two senders never both hold it, and a row still `sending`
-- 5 minutes later is never resent blind.
-- The status gains `unknown` (the carrier may have taken it: never resent) and `expired` (never to
-- be sent: queued while its line was paused, a confirmation over 5 minutes old); both keep no
-- body (SEC-037). The checks are replaced NOT VALID, then validated, so no write waits on the scan.
-- All three columns are new and nullable; no existing row changes.

ALTER TABLE messaging.outbox
  ADD COLUMN not_before timestamptz,
  ADD COLUMN quiet_exempt_until timestamptz,
  ADD COLUMN sending_since timestamptz;

ALTER TABLE messaging.outbox DROP CONSTRAINT outbox_status_check;
ALTER TABLE messaging.outbox DROP CONSTRAINT outbox_body_check;
ALTER TABLE messaging.outbox
  ADD CONSTRAINT outbox_status_check
    CHECK (status IN ('queued', 'sending', 'sent', 'blocked', 'failed', 'unknown', 'expired'))
    NOT VALID,
  ADD CONSTRAINT outbox_body_check
    CHECK (status IN ('sent', 'blocked', 'failed', 'unknown', 'expired') OR body IS NOT NULL)
    NOT VALID;
ALTER TABLE messaging.outbox VALIDATE CONSTRAINT outbox_status_check;
ALTER TABLE messaging.outbox VALIDATE CONSTRAINT outbox_body_check;

-- Plain (not CONCURRENTLY) index builds: the outbox is a small table before launch, so the brief
-- write lock while they build is harmless, and a migration runs in one transaction.
CREATE INDEX outbox_due_idx ON messaging.outbox (not_before) WHERE status = 'queued';
CREATE INDEX outbox_sending_idx ON messaging.outbox (sending_since) WHERE status = 'sending';
