-- quotes (T148): the optional "minutes this quote took" on quotes.quote (PICK-040, OPS-047, D-492).
-- The business's own estimate of how long the quote took, from the quote form's optional field; it
-- feeds only the pilot scorecard (T158) and never shows on a quote card. NULL when left empty;
-- 1 to 600 when given (the form refuses anything else). Existing rows stay NULL.

ALTER TABLE quotes.quote
  ADD COLUMN quote_minutes integer CHECK (quote_minutes BETWEEN 1 AND 600);
