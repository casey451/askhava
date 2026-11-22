-- privacy (T729): the homeowner's quote-page update address, sealed like a vault field and kept apart from her contact's email (TXT-018, PICK-029, PRIV-001, PRIV-002; D-4727, D-5340, D-5442).
-- privacy.update_email holds at most one address per contact, typed on her quote page (source
-- `quote_page`) for quote updates only. It is never her contact's conversation email
-- (privacy.contact.email), so no pick-code or release read can reach it (PICK-029, D-477). The
-- address is sealed with AES-256-GCM under VAULT_KEY, its associated data naming the row ID, so a
-- ciphertext copied to another row fails to open. A new address replaces the old row with a new
-- ID and a new 16-byte confirm_salt and clears confirmed_at and bounced_at, so a confirm link or a
-- late bounce for the earlier address never touches the new one. confirmed_at is set once, by the
-- first update email's single-use link (D-5340); bounced_at by a hard bounce of the address
-- (D-5442). The contact purge deletes the row (it nulls the contact's fields, so a cascade never
-- fires then); the cascade covers a contact row deleted outright. The table is new and empty, so
-- nothing blocks.

CREATE TABLE privacy.update_email (
  id uuid PRIMARY KEY,
  contact_id uuid NOT NULL UNIQUE REFERENCES privacy.contact (id) ON DELETE CASCADE,
  email bytea NOT NULL,
  source text NOT NULL CHECK (source = 'quote_page'),
  confirm_salt bytea NOT NULL CHECK (octet_length(confirm_salt) = 16),
  stored_at timestamptz NOT NULL,
  confirmed_at timestamptz,
  bounced_at timestamptz
);

GRANT SELECT, INSERT, UPDATE, DELETE ON privacy.update_email TO askhava_app;
