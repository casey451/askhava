-- catalog (T209): the owner-portal record "Stripe written answer on charges without the business
-- present" (PICK-048, FEE-014; D-401, D-491, D-2783): the date of Stripe's answer, its reference and
-- whether it allows the charge, with the time it was filed. The latest filed row is the one in
-- force; catalog::api::stripe_answer_filed reads it, and the owner portal's settings page (T108)
-- files it later. The app role inserts and reads it; a new answer is a new row, never an edit.
-- The table is new.

CREATE TABLE catalog.stripe_written_answer (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  answered_on date NOT NULL,
  reference text NOT NULL CHECK (length(reference) BETWEEN 1 AND 200),
  allows boolean NOT NULL,
  filed_at timestamptz NOT NULL
);

GRANT SELECT, INSERT ON catalog.stripe_written_answer TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON catalog.stripe_written_answer FROM askhava_app;
