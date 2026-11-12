-- businesses (T228): the optional reason a business gives when it leaves or stays paused (BIZ-051,
-- BIZ-054; D-446, D-463, D-491).
--
-- businesses.leave_reason is one row per business: the BIZ-054 question was asked, by leaving
-- (`left`) or by a pause past 14 days (`paused`), and what came back: one of the six choices or
-- none (the question is optional), and the "Something else" box's words only beside that choice.
-- The row's existence is "asked once". The answer feeds the weekly digest only; nothing reads it
-- for routing or fees. Append-only for the app role.

CREATE TABLE businesses.leave_reason (
  business_id uuid PRIMARY KEY REFERENCES businesses.business (id),
  asked_by text NOT NULL CHECK (asked_by IN ('left', 'paused')),
  choice text CHECK (choice IN ('busy', 'few_jobs', 'not_a_fit', 'fee', 'too_many', 'other')),
  other_words text CHECK (other_words IS NULL
    OR (choice = 'other' AND btrim(other_words) <> '' AND char_length(other_words) <= 500)),
  answered_at timestamptz NOT NULL
);
GRANT SELECT, INSERT ON businesses.leave_reason TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON businesses.leave_reason FROM askhava_app;
