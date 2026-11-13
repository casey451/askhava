-- picks (T266): picks.contact_choice, how the homeowner chose at the page pick to be reached
-- (PICK-018, PICK-042, PRIV-001; D-1264, D-1740, D-1761). One row per pick: `call` (a call
-- through the masked number) or `email` (an address she shares) and when she decided. The email
-- itself is sealed only in her vault contact (privacy::api::store_shared_email); this table holds
-- the choice word and IDs only, never a contact value (PRIV-002). A pick with no row (a text pick)
-- is a masked call. The release reads it as `email_chosen`. Append-only for askhava_app; new and
-- empty.

CREATE TABLE picks.contact_choice (
  pick_id uuid PRIMARY KEY REFERENCES picks.pick (id),
  choice text NOT NULL CHECK (choice IN ('call', 'email')),
  decided_at timestamptz NOT NULL
);
GRANT SELECT, INSERT ON picks.contact_choice TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON picks.contact_choice FROM askhava_app;
