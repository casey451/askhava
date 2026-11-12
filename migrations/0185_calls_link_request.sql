-- calls (T390): the pending links a blocked call leaves and the refused LINK replies T391 counts
-- (CALL-011, CALL-003, CALL-025; D-078, D-311, D-1696, D-2247, D-3079, D-3167).
-- calls.link_request: one pending link per line, stored by `calls::api::link_after_dial` in the
-- transaction that queues the line's LINK notice, after an unlinked phone's call connected the
-- line on its PIN. It names the line and its business, when it was asked and when it expires (24
-- hours after); `used_at` is set by T391's `link_reply` when the business's "LINK <PIN>" linked the
-- phone. A pending link is live only while `expires_at` is still in the future.
-- calls.link_request_digest: the blocked phone's keyed digests, one per `sender-dir` index key
-- version (`Vault::blind_index`, as `messaging.sender_directory` stores a linked phone), never the
-- number, its last 4 digits or the PIN (PRIV-002, PRIV-024). T391 moves them into the sender
-- directory and deletes them when the link is used, or when the pending link is met no longer live.
-- calls.link_reply_refusal: refused LINK replies per replying phone (its keyed digest and key
-- version, never the number) and Arizona day, at most 3 (CALL-025's per-call count, T391).
--
-- Grants: the calls default privileges give SELECT and INSERT (D-1520); added here only what
-- `link_after_dial` and T391's `link_reply` use: UPDATE of `used_at` and DELETE on the pending
-- links, DELETE on their digests, UPDATE of `refused` on the counts. All three tables are new.

CREATE TABLE calls.link_request (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  line_id uuid NOT NULL UNIQUE REFERENCES calls.number_pool (id),
  business_id uuid NOT NULL,
  requested_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  used_at timestamptz,
  CHECK (expires_at = requested_at + interval '24 hours'),
  CHECK (used_at IS NULL OR used_at < expires_at)
);
CREATE INDEX link_request_business_idx ON calls.link_request (business_id, expires_at);
GRANT UPDATE (used_at), DELETE ON calls.link_request TO askhava_app;

CREATE TABLE calls.link_request_digest (
  request_id uuid NOT NULL REFERENCES calls.link_request (id) ON DELETE CASCADE,
  index_version smallint NOT NULL CHECK (index_version > 0),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  PRIMARY KEY (request_id, index_version)
);
GRANT DELETE ON calls.link_request_digest TO askhava_app;

CREATE TABLE calls.link_reply_refusal (
  key_version smallint NOT NULL CHECK (key_version > 0),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  az_day date NOT NULL,
  refused smallint NOT NULL CHECK (refused BETWEEN 1 AND 3),
  PRIMARY KEY (key_version, digest, az_day)
);
GRANT UPDATE (refused) ON calls.link_reply_refusal TO askhava_app;
