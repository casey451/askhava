-- businesses (T220): team members with their roles and the staff phones each linked (BIZ-023,
-- CALL-012, D-191, D-491, D-963). businesses.team_member is one row per person on a business
-- account, the owner included: is_owner marks the account owner, who always holds both roles
-- ("Cards and quotes" and "Confirm and billing") and is never removed; every other member holds
-- at least one. mailbox_id is the opaque ID of the person's sign-in email, which is never stored
-- here (PRIV-001); notice_addresses returns it (D-492). A removed member keeps its row with
-- removed_at set, so the audit trail and the end-sessions event still name it. businesses.staff_phone
-- is one row per linked main or staff phone: its id is the line ID messaging's sender directory
-- knows it by, linked_by the member who linked it (CALL-012), of the same business by a composite
-- foreign key, and at most one main phone per business. The number itself is never stored here;
-- messaging keeps only its blind-index digests.
-- Both tables are new and empty.

CREATE TABLE businesses.team_member (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  mailbox_id uuid NOT NULL,
  is_owner boolean NOT NULL DEFAULT false,
  cards_and_quotes boolean NOT NULL,
  confirm_and_billing boolean NOT NULL,
  added_at timestamptz NOT NULL,
  removed_at timestamptz,
  CHECK (cards_and_quotes OR confirm_and_billing),
  CHECK (NOT is_owner OR (cards_and_quotes AND confirm_and_billing AND removed_at IS NULL)),
  UNIQUE (id, business_id)
);
CREATE UNIQUE INDEX team_member_one_owner_idx ON businesses.team_member (business_id)
  WHERE is_owner;
CREATE UNIQUE INDEX team_member_mailbox_idx ON businesses.team_member (business_id, mailbox_id)
  WHERE removed_at IS NULL;

CREATE TABLE businesses.staff_phone (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  linked_by uuid NOT NULL,
  is_main boolean NOT NULL DEFAULT false,
  linked_at timestamptz NOT NULL,
  FOREIGN KEY (linked_by, business_id) REFERENCES businesses.team_member (id, business_id)
);
CREATE UNIQUE INDEX staff_phone_one_main_idx ON businesses.staff_phone (business_id) WHERE is_main;
CREATE INDEX staff_phone_linked_by_idx ON businesses.staff_phone (linked_by);

-- A member is removed by removed_at, never by deleting the row; an unlinked phone is deleted.
GRANT SELECT, INSERT, UPDATE ON businesses.team_member TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.team_member FROM askhava_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON businesses.staff_phone TO askhava_app;
REVOKE TRUNCATE ON businesses.staff_phone FROM askhava_app;
