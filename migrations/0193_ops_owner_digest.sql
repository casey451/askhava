-- ops (T204): the owner's away date, the daytime push items and slots, and the weekly digest row
-- (OPS-026, OPS-029, OPS-032, OPS-035; D-3880, D-3883, D-4044). ops.alert_push and ops.alert_ack
-- already exist (0192) and are not re-created. ops.owner_away is one row: the date Casey is away
-- until, null when he is not away (OPS-026); T483 shows it to businesses and in the HELP reply.
-- ops.push_item holds the OPS-032 items that are not open alerts (help requests due within 24
-- hours or held back by the OPS-036 daily limit): kind, subject kind and ID, the time it may first
-- be listed, and Casey's done mark; at most one open row per kind and subject. ops.push_slot is
-- the record of each 9 a.m. and 3 p.m. Arizona push slot handled, so each slot pushes at most once
-- and a slot with nothing open sends nothing. ops.owner_digest is one row per weekly digest, its
-- sent, opened and reminder times (T528 writes them). Code names, IDs and times only: no text and
-- no contact detail. All tables are new and empty except the one ops.owner_away row. The app role
-- keeps the schema default SELECT and INSERT (0110), gains UPDATE on the rows that move, and cannot
-- insert a second ops.owner_away row.

CREATE TABLE ops.owner_away (
  id boolean PRIMARY KEY DEFAULT true CHECK (id),
  away_until date,
  set_at timestamptz
);
INSERT INTO ops.owner_away (id) VALUES (true);
REVOKE INSERT ON ops.owner_away FROM askhava_app;
GRANT UPDATE ON ops.owner_away TO askhava_app;

CREATE TABLE ops.push_item (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  kind text NOT NULL CHECK (kind ~ '^[a-z][a-z0-9_.]{0,63}$'),
  subject_kind text NOT NULL CHECK (subject_kind ~ '^[a-z][a-z0-9_.]{0,63}$'),
  subject_id uuid NOT NULL,
  show_from timestamptz NOT NULL,
  done_at timestamptz
);
CREATE UNIQUE INDEX push_item_open_idx ON ops.push_item (kind, subject_kind, subject_id)
  WHERE done_at IS NULL;
GRANT UPDATE ON ops.push_item TO askhava_app;

CREATE TABLE ops.push_slot (
  slot timestamptz PRIMARY KEY,
  items integer NOT NULL CHECK (items >= 0),
  handled_at timestamptz NOT NULL
);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.push_slot FROM askhava_app;

CREATE TABLE ops.owner_digest (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  sent_at timestamptz NOT NULL,
  opened_at timestamptz,
  reminded_at timestamptz
);
GRANT UPDATE ON ops.owner_digest TO askhava_app;
