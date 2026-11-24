-- portals (T804): the single-use email link that confirms a delete or copy request made in the
-- signed-in portal for a number on the suppression list (PRIV-031, SEC-026, SEC-037; D-5281,
-- D-5332, D-5339, D-5340). request_id is the request's received audit row ID (ops.audit), with no
-- foreign key, as no key crosses schemas. salt is the link's 128-bit SEC-037 salt: the token is
-- recomputed from it and the ID secret, never stored. A link lives 15 minutes from sent_at; used_at
-- is set once, by one guarded UPDATE in the transaction that acts on the request. IDs and times
-- only, never a phone, address, name or token. The table is new and empty.

CREATE TABLE portals.data_request_link (
  request_id bigint PRIMARY KEY,
  salt bytea NOT NULL CHECK (octet_length(salt) = 16),
  sent_at timestamptz NOT NULL,
  used_at timestamptz
);

-- The app reads and stores links and sets only the used time; nothing is deleted by the app. The
-- schema's default privileges grant table-wide UPDATE and DELETE, so both are revoked first and
-- UPDATE is granted back on used_at alone.
REVOKE UPDATE, DELETE, TRUNCATE ON portals.data_request_link FROM askhava_app;
GRANT SELECT, INSERT ON portals.data_request_link TO askhava_app;
GRANT UPDATE (used_at) ON portals.data_request_link TO askhava_app;
