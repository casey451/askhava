-- messaging (T396): the sender-directory digests of phones waiting for the owner's approval
-- (BIZ-033, CALL-012; D-3081, D-3263).
-- messaging.sender_held holds a waiting phone's keyed `sender-dir` digests, one per index key
-- version, exactly as `messaging.sender_directory` would, so no lookup finds the phone until
-- `sender_release` moves them back on the owner's approval. Never the number (PRIV-002). Unique on
-- index version and digest, so two waiting phones never hold one number.
-- Grants: SELECT, INSERT and DELETE for the app role and nothing more.

CREATE TABLE messaging.sender_held (
  line_id uuid NOT NULL,
  index_version smallint NOT NULL CHECK (index_version > 0),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  business_id uuid NOT NULL,
  PRIMARY KEY (line_id, index_version),
  UNIQUE (index_version, digest)
);
REVOKE ALL ON messaging.sender_held FROM askhava_app;
GRANT SELECT, INSERT, DELETE ON messaging.sender_held TO askhava_app;
