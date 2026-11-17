-- ops (T479): two pilot scorecard inputs (OPS-039, PICK-057; D-3753, D-3883).
-- ops.not_a_fit_answer holds a business's "Not a fit for us" answer on a job card page: the job,
-- the business, the job's trade code, the reason code and the time; one answer per business and
-- job (the primary key), so a second answer adds nothing. The answer changes no routing or fee:
-- nothing outside `ops` reads this table. ops.minutes_entry holds Casey's weekly minutes entry
-- (the Monday of the Arizona week, minutes on recruiting and on everything else, who and when);
-- an entry is never changed, and a later entry for the same week replaces the earlier one when
-- read. IDs, codes, counts and times only; no personal data. Both tables are new and empty, and
-- written and read only by `app/ops/src/scorecard_inputs.rs`. The app role keeps the schema
-- default SELECT and INSERT only (migration 0110).

CREATE TABLE ops.not_a_fit_answer (
  job_id uuid NOT NULL,
  business_id uuid NOT NULL,
  trade text NOT NULL CHECK (trade ~ '^[a-z][a-z0-9_]{0,63}$'),
  reason text NOT NULL CHECK (reason IN ('not_my_trade', 'not_my_town', 'too_big_or_small', 'other')),
  answered_at timestamptz NOT NULL,
  PRIMARY KEY (business_id, job_id)
);
CREATE INDEX not_a_fit_answer_at_idx ON ops.not_a_fit_answer (answered_at);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.not_a_fit_answer FROM askhava_app;

CREATE TABLE ops.minutes_entry (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  week date NOT NULL CHECK (extract(isodow FROM week) = 1),
  recruiting integer NOT NULL CHECK (recruiting BETWEEN 0 AND 10080),
  other integer NOT NULL CHECK (other BETWEEN 0 AND 10080),
  entered_by uuid NOT NULL,
  entered_at timestamptz NOT NULL
);
CREATE INDEX minutes_entry_week_idx ON ops.minutes_entry (week, id);
REVOKE UPDATE, DELETE, TRUNCATE ON ops.minutes_entry FROM askhava_app;
