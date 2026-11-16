-- ops (T105): the delivery and coverage alert facts, alert acknowledgements and pushes, and safe
-- mode's state (OPS-001, OPS-004, OPS-006, OPS-023, OPS-025; D-163, D-3144, D-3878).
-- ops.alert_ack and ops.alert_push are append-only: one row per alert, by whom and when
-- it was acknowledged, and when it was pushed to Casey's phone (T487 reads the push time for the
-- 24-hour safe-mode clock). ops.safe_mode is one row: `on` and `since` (the time it last turned
-- on, null while off), read and written by T487 only. ops.delivery_alert names the audience line
-- of each text-delivery alert; ops.delivery_line holds the last time each line was back at 97% of
-- its last 200 texts (OPS-025). ops.coverage holds the latest active-business count of each town
-- and trade (OPS-006), ops.trade_request each trade's requests per Arizona calendar month. Codes,
-- counts and record IDs only; no personal data. All tables are new and empty except the one
-- ops.safe_mode row. The app role keeps the schema default SELECT and INSERT (migration 0110), gains
-- UPDATE on the four tables whose rows move, and cannot insert a second ops.safe_mode row.

CREATE TABLE ops.alert_ack (
  alert_id uuid PRIMARY KEY REFERENCES ops.alert (id),
  acknowledged_by text NOT NULL CHECK (acknowledged_by IN ('casey', 'system')),
  at timestamptz NOT NULL
);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.alert_ack FROM askhava_app;

CREATE TABLE ops.alert_push (
  alert_id uuid PRIMARY KEY REFERENCES ops.alert (id),
  pushed_at timestamptz NOT NULL
);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.alert_push FROM askhava_app;

CREATE TABLE ops.safe_mode (
  id boolean PRIMARY KEY DEFAULT true CHECK (id),
  "on" boolean NOT NULL DEFAULT false,
  since timestamptz,
  CHECK ("on" = (since IS NOT NULL))
);
INSERT INTO ops.safe_mode (id) VALUES (true);
REVOKE INSERT ON ops.safe_mode FROM askhava_app;
GRANT UPDATE ON ops.safe_mode TO askhava_app;

CREATE TABLE ops.delivery_alert (
  alert_id uuid PRIMARY KEY REFERENCES ops.alert (id),
  audience text NOT NULL CHECK (audience IN ('homeowner', 'business'))
);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.delivery_alert FROM askhava_app;

CREATE TABLE ops.delivery_line (
  audience text PRIMARY KEY CHECK (audience IN ('homeowner', 'business')),
  recovered_at timestamptz NOT NULL
);
GRANT UPDATE ON ops.delivery_line TO askhava_app;

CREATE TABLE ops.coverage (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  scope text NOT NULL CHECK (scope IN ('town', 'trade')),
  code text NOT NULL CHECK (code ~ '^[a-z][a-z0-9_]{0,63}$'),
  active integer NOT NULL CHECK (active >= 0),
  counted_at timestamptz NOT NULL,
  UNIQUE (scope, code)
);
GRANT UPDATE ON ops.coverage TO askhava_app;

CREATE TABLE ops.trade_request (
  trade text NOT NULL CHECK (trade ~ '^[a-z][a-z0-9_]{0,63}$'),
  month date NOT NULL CHECK (extract(day FROM month) = 1),
  requests integer NOT NULL CHECK (requests >= 0),
  PRIMARY KEY (trade, month)
);
GRANT UPDATE ON ops.trade_request TO askhava_app;
