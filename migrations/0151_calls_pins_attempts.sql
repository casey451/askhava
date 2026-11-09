-- calls (T262): a random stored PIN per line, the per-day PIN-try count, the per-call try record
-- and each answered call's start (CALL-002, CALL-003, CALL-004, CALL-005, CALL-025, SEC-017,
-- FEE-022; D-1684, D-1685, D-1755, D-1836, D-2091 to D-2095).
-- calls.number_pool.pin: four digits drawn from the OS random source when the line is assigned
-- (`calls::pins`), unique among the open lines of one pool number. A line's ID never gives its
-- PIN. Lines assigned before this migration keep no PIN (the table is empty before launch) and
-- connect only when the caller holds exactly one line on the number.
-- calls.pin_try: wrong PIN tries per calling number, pool number and Arizona day. The caller is a
-- digest only: HMAC-SHA-256 under ID_SECRET (`key_version`) with the label `pin-try|v1` over the
-- E.164 number, never the number (PRIV-002, PRIV-024). `tries` never passes 6 (CALL-025).
-- calls.pin_call: one row per (CallSid, try number of that call), so a repeated Twilio callback
-- is not another try; `today_before` is the day's count the try was judged on (SEC-017).
-- calls.call_start: one row per parent CallSid, written when the voice webhook answers with the
-- dial; `dialed_sid` is filled later by the dialed leg's callback (T271, D-1836).
--
-- Grants: the calls default privileges give SELECT and INSERT (D-1520); the only UPDATEs are
-- `pin_try.tries` and `call_start.dialed_sid`. Nothing grants DELETE. All three tables are new.

ALTER TABLE calls.number_pool ADD COLUMN pin text CHECK (pin ~ '^[0-9]{4}$');
CREATE UNIQUE INDEX number_pool_open_pin_idx ON calls.number_pool (pool_number, pin)
  WHERE closed_at IS NULL AND pin IS NOT NULL;

CREATE TABLE calls.pin_try (
  caller_digest bytea NOT NULL CHECK (octet_length(caller_digest) = 32),
  key_version smallint NOT NULL CHECK (key_version > 0),
  pool_number text NOT NULL CHECK (pool_number ~ '^\+[0-9]{8,15}$'),
  az_day date NOT NULL,
  tries smallint NOT NULL CHECK (tries BETWEEN 1 AND 6),
  PRIMARY KEY (caller_digest, pool_number, az_day)
);
GRANT UPDATE (tries) ON calls.pin_try TO askhava_app;

CREATE TABLE calls.pin_call (
  call_sid text NOT NULL CHECK (call_sid ~ '^CA[0-9a-zA-Z]{32}$'),
  try_no smallint NOT NULL CHECK (try_no BETWEEN 1 AND 3),
  today_before smallint NOT NULL CHECK (today_before BETWEEN 0 AND 6),
  PRIMARY KEY (call_sid, try_no)
);

CREATE TABLE calls.call_start (
  parent_sid text PRIMARY KEY CHECK (parent_sid ~ '^CA[0-9a-zA-Z]{32}$'),
  line_id uuid NOT NULL REFERENCES calls.number_pool (id),
  started_at timestamptz NOT NULL,
  dialed_sid text UNIQUE CHECK (dialed_sid ~ '^CA[0-9a-zA-Z]{32}$')
);
CREATE INDEX call_start_line_idx ON calls.call_start (line_id, started_at);
GRANT UPDATE (dialed_sid) ON calls.call_start TO askhava_app;
