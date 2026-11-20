-- recovery (T110): the replay's run record and the items it lists for Casey, and the restore
-- drill's record (OPS-014, OPS-015, OPS-016, OPS-017; D-2764, D-3889).
-- recovery.replay_run: one row per replay after a restore, with how many outbox rows were found
-- in Twilio's sent log and marked sent, how many Twilio had no text for, and how many charges were
-- booked from Stripe's record or already held.
-- recovery.replay_item: what a run lists for Casey: an ambiguous or refused text (its outbox row
-- ID) and an unmatched or refunded Stripe charge (its charge ID). IDs only, never a number or name.
-- recovery.restore_drill: one row per restore drill (T206): when it ran, the backup object
-- restored, the replay run, the seconds the restore took and the outcome.
--
-- All three are append-only: the schema's default privileges give the app role SELECT and INSERT
-- only (migration 0198); UPDATE, DELETE and TRUNCATE are revoked here as well. The tables are new.

CREATE TABLE recovery.replay_run (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  ran_at timestamptz NOT NULL,
  texts_marked integer NOT NULL CHECK (texts_marked >= 0),
  texts_unsent integer NOT NULL CHECK (texts_unsent >= 0),
  charges_booked integer NOT NULL CHECK (charges_booked >= 0),
  charges_already integer NOT NULL CHECK (charges_already >= 0)
);
REVOKE UPDATE, DELETE, TRUNCATE ON recovery.replay_run FROM askhava_app;

CREATE TABLE recovery.replay_item (
  run_id uuid NOT NULL REFERENCES recovery.replay_run (id),
  kind text NOT NULL
    CHECK (kind IN ('text_ambiguous', 'text_refused', 'charge_unmatched', 'charge_refunded')),
  subject text NOT NULL CHECK (subject ~ '^[A-Za-z0-9_-]{1,255}$'),
  PRIMARY KEY (run_id, kind, subject)
);
REVOKE UPDATE, DELETE, TRUNCATE ON recovery.replay_item FROM askhava_app;

CREATE TABLE recovery.restore_drill (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  ran_at timestamptz NOT NULL,
  backup_object text NOT NULL CHECK (backup_object ~ '^[a-z0-9_./-]{1,200}$'),
  replay_run_id uuid REFERENCES recovery.replay_run (id),
  restore_seconds integer CHECK (restore_seconds >= 0),
  outcome text NOT NULL CHECK (outcome IN ('ok', 'failed'))
);
REVOKE UPDATE, DELETE, TRUNCATE ON recovery.restore_drill FROM askhava_app;
