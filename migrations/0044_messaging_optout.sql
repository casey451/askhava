-- messaging (T029): the one suppression list, the opt-out event log and the blocked outbox status
-- (CON-008 to CON-010, CON-013, CON-014, CON-022, CON-030, CON-035, CON-036).
-- messaging.optout_event logs every opt-out and START with its channel (text, email, email reply,
-- help form, portal switch, owner-recorded, carrier 21610), how it was detected (`word:<word>`,
-- `phrase:<pattern ID>`, `provider`, or the channel's own label, CON-036) and the time it took
-- effect (CON-022, CON-035). The number is held only as its blind-index digest under the
-- homeowner-phone index key, never in plain text (PRIV-002, PRIV-024). Rows are never changed or
-- deleted by the app: UPDATE, DELETE and TRUNCATE are revoked from askhava_app.
-- messaging.suppression is the one list every channel feeds and every send checks: one row per
-- index key version and digest, with the event that put the number there and since when; START
-- deletes the number's rows (CON-015, CON-035).
-- messaging.outbox gains the terminal status `blocked`: a text the carrier refused with 21610 is
-- never retried and keeps no body (CON-014, SEC-037). The body check 0042 declared at table level
-- was auto-named `outbox_check`; it is replaced by the named `outbox_body_check`. Both tables are new and empty; existing
-- outbox rows are untouched.

CREATE TABLE messaging.optout_event (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  action text NOT NULL CHECK (action IN ('opt_out', 'start')),
  channel text NOT NULL
    CHECK (channel IN ('text', 'email', 'email_reply', 'help_form', 'portal', 'owner', 'carrier')),
  detected text NOT NULL CHECK (detected ~ '^[a-z_]+(:[a-z0-9_. ]{1,64})?$'),
  index_version smallint NOT NULL CHECK (index_version > 0),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  effective_at timestamptz NOT NULL
);
CREATE INDEX optout_event_digest_idx ON messaging.optout_event (index_version, digest);
REVOKE UPDATE, DELETE, TRUNCATE ON messaging.optout_event FROM askhava_app;

CREATE TABLE messaging.suppression (
  index_version smallint NOT NULL CHECK (index_version > 0),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  event_id uuid NOT NULL REFERENCES messaging.optout_event (id),
  since timestamptz NOT NULL,
  PRIMARY KEY (index_version, digest)
);

ALTER TABLE messaging.outbox DROP CONSTRAINT outbox_status_check;
ALTER TABLE messaging.outbox DROP CONSTRAINT outbox_check;
ALTER TABLE messaging.outbox
  ADD CONSTRAINT outbox_status_check CHECK (status IN ('queued', 'sending', 'sent', 'blocked')),
  ADD CONSTRAINT outbox_body_check CHECK (status IN ('sent', 'blocked') OR body IS NOT NULL);
