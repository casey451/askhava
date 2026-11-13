-- privacy (T412): business members' sign-in addresses and texting phones, sealed apart from the homeowner vault (SEC-004, BIZ-037, PRIV-002, PRIV-003, PRIV-024, PRIV-026; D-3262).
-- privacy.business_mailbox holds one sign-in address per mailbox ID (the opaque ID every businesses
-- table stores), sealed with AES-256-GCM under VAULT_KEY with associated data
-- `privacy.business_mailbox|address|<id>|v<version>`; touched_at moves each time the address is
-- looked up or minted, so the sweep forgets only mailboxes nothing uses. The address is found only by
-- its business email blind index (`email|vN`, the digest read_inbound_email matches business senders
-- with), one mailbox per digest. privacy.staff_phone holds the texting number of one
-- businesses.staff_phone line (that table's ID, also the sender directory's line ID), sealed with
-- associated data `privacy.staff_phone|number|<line>|v<version>`. No plaintext column, and no foreign
-- key into businesses (a lower layer never names a higher one's tables; the sweep keeps them in step).
-- key_version finds the rows a key rotation still has to re-seal. All tables are new and empty.

CREATE TABLE privacy.business_mailbox (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  address bytea NOT NULL CHECK (octet_length(address) >= 30),
  key_version integer GENERATED ALWAYS AS (privacy.sealed_version(address)) STORED,
  created_at timestamptz NOT NULL DEFAULT now(),
  touched_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX business_mailbox_key_version_idx ON privacy.business_mailbox (key_version);
CREATE INDEX business_mailbox_touched_idx ON privacy.business_mailbox (touched_at);

-- One digest per mailbox, purpose and index key version; both versions coexist during a rotation.
CREATE TABLE privacy.business_mailbox_index (
  purpose text NOT NULL CHECK (purpose = 'email'),
  key_version integer NOT NULL CHECK (key_version BETWEEN 1 AND 65535),
  digest bytea NOT NULL CHECK (octet_length(digest) = 32),
  mailbox_id uuid NOT NULL REFERENCES privacy.business_mailbox (id) ON DELETE CASCADE,
  PRIMARY KEY (purpose, key_version, digest)
);
CREATE INDEX business_mailbox_index_mailbox_idx ON privacy.business_mailbox_index (mailbox_id);

CREATE TABLE privacy.staff_phone (
  line_id uuid PRIMARY KEY,
  number bytea NOT NULL CHECK (octet_length(number) >= 30),
  key_version integer GENERATED ALWAYS AS (privacy.sealed_version(number)) STORED,
  set_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX staff_phone_key_version_idx ON privacy.staff_phone (key_version);

GRANT SELECT, INSERT, UPDATE, DELETE ON privacy.business_mailbox TO askhava_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON privacy.business_mailbox_index TO askhava_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON privacy.staff_phone TO askhava_app;
