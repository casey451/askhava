-- businesses (T396): a staff phone's approval and the facts the owner approves it by (BIZ-033,
-- CALL-012; D-311, D-3081, D-3263).
-- `approved_at` is when the phone began to work: a phone's standing is `approved_at IS NOT NULL`
-- alone. It defaults to now(), so a row inserted without naming it is approved; every link or
-- number change by a member who is not the owner writes NULL explicitly (`team::link_phone`,
-- `team::change_phone_at`), and the phone stays out of `messaging`'s sender directory until the
-- owner approves it. `number_set_at` and `number_set_by` say when and by which member the current
-- number was set, so an approval names the exact number it approves. `number_end` is the number's
-- last 4 digits (D-078), never the whole number (PRIV-002); NULL when no number was given here
-- (the LINK reply's phone, whose number `businesses` never sees).
-- Every existing row was linked under the earlier rule: it is approved at its link time and its
-- number set then by the member who linked it. No row exists outside a test database before
-- Gate C. The table's grants are unchanged.

ALTER TABLE businesses.staff_phone
  ADD COLUMN approved_at timestamptz DEFAULT now(),
  ADD COLUMN number_set_at timestamptz,
  ADD COLUMN number_set_by uuid,
  ADD COLUMN number_end char(4) CHECK (number_end ~ '^[0-9]{4}$');

UPDATE businesses.staff_phone
  SET approved_at = linked_at, number_set_at = linked_at, number_set_by = linked_by;
