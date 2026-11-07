-- businesses (T083): homeowner complaints about a business, the business's one reply and Casey's
-- digest listing (BIZ-011, BIZ-012, BIZ-029; D-016, D-200, D-222, D-280).
-- businesses.complaint is one row per complaint: the business, the job it is about (an ID only; no
-- foreign key, since jobs is another schema), its kind as a code name, whether the homeowner's
-- words held a safety word (the words themselves are never stored, so no homeowner detail sits
-- here, PRIV-001), when it came in, the business's reply and its time (allowed only before the
-- 5-day box closes, BIZ-029) and when it was listed for Casey's digest after the box closed
-- (OPS-005). The 3-in-90-days count reads (business_id, received_at). The table is new and empty.

CREATE TABLE businesses.complaint (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL REFERENCES businesses.business (id),
  job_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind ~ '^[a-z][a-z_]{0,31}$'),
  safety boolean NOT NULL,
  received_at timestamptz NOT NULL,
  reply text CHECK (char_length(reply) BETWEEN 1 AND 1000),
  replied_at timestamptz,
  listed_at timestamptz,
  CHECK ((reply IS NULL) = (replied_at IS NULL)),
  CHECK (replied_at IS NULL OR replied_at < received_at + interval '120 hours'),
  CHECK (listed_at IS NULL OR listed_at >= received_at + interval '120 hours')
);
CREATE INDEX complaint_business_idx ON businesses.complaint (business_id, received_at);

-- The schema default gives SELECT and INSERT only (0110); the revoke below keeps it so if a later
-- default changes. A complaint is never deleted and only its reply and listing time are set later.
REVOKE UPDATE, DELETE, TRUNCATE ON businesses.complaint FROM askhava_app;
GRANT UPDATE (reply, replied_at, listed_at) ON businesses.complaint TO askhava_app;
