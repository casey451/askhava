-- payments (T210)
-- payments.expense holds Casey's manual expenses list for the owner money view (FEE-046, D-444,
-- D-491): the Arizona date spent, the vendor, the amount in whole cents (above 0), a short
-- category, an optional note and when it was entered. It is never part of the ledger
-- (payments.ledger_entry, FEE-028): no ledger row points at it and nothing here writes one.
-- Business expenses only; no homeowner data. Written only by `add_expense` and read only by
-- `app/payments/src/money_view.rs`. The app role keeps the schema default SELECT and INSERT only
-- (D-1495, TD-SEC-33): the app never changes or deletes an entered expense, so no GRANT is added
-- here (D-4728).

CREATE TABLE payments.expense (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  spent_on date NOT NULL,
  vendor text NOT NULL CHECK (char_length(vendor) BETWEEN 1 AND 120),
  amount_cents bigint NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000),
  category text NOT NULL CHECK (char_length(category) BETWEEN 1 AND 60),
  note text NOT NULL DEFAULT '' CHECK (char_length(note) <= 500),
  entered_at timestamptz NOT NULL
);
CREATE INDEX expense_spent_on_idx ON payments.expense (spent_on, id);
