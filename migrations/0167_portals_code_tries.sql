-- portals (T272): the new-device code's one send and its five tries per sign-in link, reserved
-- before Verify is called (SEC-004, SEC-014, SEC-020, SEC-048; D-1776, D-1777, D-1837, D-2349).
-- portals.code_send is a link's one Verify send: a press reserves it in a short committed
-- transaction (a new row, a failed one, or a pending one a crash left more than 75 seconds ago),
-- calls Verify with no transaction open, then records sent or failed; it replaces the born-used
-- `code-sent:<link>` marker row in portals.login_link. A check needs outcome sent.
-- portals.code_try is one numbered code check of a link: the next number is reserved in its own
-- short transaction under the link's advisory lock, the unique key and the 1 to 5 check refuse a
-- sixth, Verify is called with no transaction open, then the outcome is recorded. A try left
-- pending (a crash or a timeout) still counts as used, so a restart gives no try back.
-- Both reference portals.login_link in the same schema. IDs and times only, never a phone,
-- address, token or code. Both tables are new and empty.

CREATE TABLE portals.code_send (
  link_id uuid PRIMARY KEY REFERENCES portals.login_link (id),
  reserved_at timestamptz NOT NULL,
  outcome text NOT NULL CHECK (outcome IN ('pending', 'sent', 'failed'))
);

CREATE TABLE portals.code_try (
  link_id uuid NOT NULL REFERENCES portals.login_link (id),
  try_no integer NOT NULL CHECK (try_no >= 1 AND try_no <= 5),
  reserved_at timestamptz NOT NULL,
  outcome text NOT NULL CHECK (outcome IN ('pending', 'right', 'wrong', 'abandoned')),
  PRIMARY KEY (link_id, try_no)
);

-- The app reads and inserts both; a send's reservation is retaken and its outcome recorded, a
-- try's outcome recorded; nothing is deleted by the app.
GRANT SELECT, INSERT ON portals.code_send TO askhava_app;
GRANT UPDATE (reserved_at, outcome) ON portals.code_send TO askhava_app;
REVOKE DELETE, TRUNCATE ON portals.code_send FROM askhava_app;
GRANT SELECT, INSERT ON portals.code_try TO askhava_app;
GRANT UPDATE (outcome) ON portals.code_try TO askhava_app;
REVOKE DELETE, TRUNCATE ON portals.code_try FROM askhava_app;
