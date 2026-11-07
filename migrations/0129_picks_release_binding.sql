-- picks (T253): bind each release to the homeowner contact its proof named and her PICK-018
-- choice of email contact (PICK-018, PICK-022, PICK-024, PRIV-002, PRIV-005; D-1739). contact_id
-- is a vault ID only, never a contact value (PRIV-002); mail_chosen says whether the release
-- opened her email (named without the word the vault's column scan looks for, D-1894). The business portal's released view (picks::api::released_view) reopens
-- exactly this contact and these fields, never one looked up again. Both are NOT NULL with no
-- default: no picks.release row exists before Gate C, so the table is empty when this runs.
-- picks.release stays append-only for the app role (0106's REVOKE is unchanged).

ALTER TABLE picks.release
  ADD COLUMN contact_id uuid NOT NULL,
  ADD COLUMN mail_chosen boolean NOT NULL;
