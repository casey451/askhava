-- privacy (T022): the contact vault, its blind index and the hourly plaintext-read counts (PRIV-001, PRIV-002, PRIV-003, PRIV-024, SEC-047, D-749).
-- Every homeowner contact field lives only here (PRIV-001), each column one sealed value: a 2-byte key
-- version, a random 96-bit nonce and the AES-256-GCM ciphertext under VAULT_KEY, whose associated data
-- names the table, column, row and key version, so a value copied to another row or column fails to
-- open (PRIV-002, PRIV-024). The database never holds a key or a plaintext value, and no index or WHERE
-- ever sees one (postgres brain P12): phone lookup is by an HMAC-SHA-256 digest under a per-purpose key
-- derived from VAULT_INDEX_KEY. oldest_key_version finds the rows a key rotation still has to re-seal.
-- read_count holds one row per hour and reader-purpose action, the input of the SEC-047 rate check;
-- the audit row of each read is ops.audit_log's (PRIV-003). All tables are new and empty.

CREATE SCHEMA IF NOT EXISTS privacy;
GRANT USAGE ON SCHEMA privacy TO askhava_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA privacy
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO askhava_app;

-- The key version a sealed value starts with (its first two bytes, big-endian).
CREATE FUNCTION privacy.sealed_version(sealed bytea) RETURNS integer
  LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
  RETURN get_byte(sealed, 0) * 256 + get_byte(sealed, 1);

CREATE TABLE privacy.contact (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  created_at timestamptz NOT NULL DEFAULT now(),
  name bytea CHECK (octet_length(name) >= 30),
  phone bytea CHECK (octet_length(phone) >= 30),
  email bytea CHECK (octet_length(email) >= 30),
  address bytea CHECK (octet_length(address) >= 30),
  onsite_contact bytea CHECK (octet_length(onsite_contact) >= 30),
  oldest_key_version integer GENERATED ALWAYS AS (LEAST(
    privacy.sealed_version(name), privacy.sealed_version(phone), privacy.sealed_version(email),
    privacy.sealed_version(address), privacy.sealed_version(onsite_contact))) STORED
);
CREATE INDEX contact_key_version_idx ON privacy.contact (oldest_key_version);

-- One digest per contact, purpose and index key version; both versions coexist during a rotation.
CREATE TABLE privacy.contact_index (
  purpose text NOT NULL CHECK (purpose IN ('homeowner-phone', 'sender-dir', 'email')),
  key_version integer NOT NULL CHECK (key_version BETWEEN 1 AND 65535),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  contact_id uuid NOT NULL REFERENCES privacy.contact (id) ON DELETE CASCADE,
  PRIMARY KEY (purpose, key_version, digest)
);
CREATE INDEX contact_index_contact_idx ON privacy.contact_index (contact_id);

CREATE TABLE privacy.read_count (
  id uuid NOT NULL UNIQUE DEFAULT gen_random_uuid(),
  hour timestamptz NOT NULL CHECK (date_trunc('hour', hour, 'UTC') = hour),
  action text NOT NULL CHECK (action ~ '^vault_read\.[a-z_]+\.[a-z_]+$'),
  reads integer NOT NULL CHECK (reads > 0),
  alerted boolean NOT NULL DEFAULT false,
  PRIMARY KEY (action, hour)
);
