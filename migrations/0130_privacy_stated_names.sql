-- privacy (T257): the names a homeowner stated in her earlier texts, sealed like a vault field (AI-020, PRIV-001, PRIV-002, PRIV-007, PRIV-024; D-1752).
-- privacy.stated_name keeps each name word the scrubber marked as stated in one of her texts, so a
-- later text has it removed too. The word is sealed with AES-256-GCM under VAULT_KEY (its version
-- in key_version, the associated data naming this table, the contact and the version) and found
-- only by an HMAC-SHA256 digest under a key derived from VAULT_INDEX_KEY (`stated-name|vN`), so a
-- repeat is one row per contact and digest. The contact's deletion takes her names with it. The
-- app role reads, adds and deletes (the purge, "Delete my data", and rotation's delete and
-- re-insert); it never updates. The table is new and empty, so nothing blocks.

CREATE TABLE privacy.stated_name (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  contact_id uuid NOT NULL REFERENCES privacy.contact (id) ON DELETE CASCADE,
  name bytea NOT NULL,
  key_version integer NOT NULL CHECK (key_version BETWEEN 1 AND 65535),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  index_version integer NOT NULL CHECK (index_version BETWEEN 1 AND 65535),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (contact_id, digest)
);

GRANT DELETE ON privacy.stated_name TO askhava_app;
