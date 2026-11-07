-- jobs (T230): the homeowner's private feedback after Done, its share tick and any removal with its
-- reason (JOB-017, D-083, D-466, D-492, D-493).
-- jobs.feedback is one row per job: what the homeowner wrote to Ask Hava on the feedback page, the
-- one share box ("Ask Hava may show what I wrote on askhava.com with my first name and town"),
-- which starts unticked and is stored as sent, and when it came in. There is no rating or tone
-- column, so nothing can leave feedback out for being negative. Ticked feedback is shown only while
-- it has no removal and no author mark: a removal is one of the four JOB-017 reasons, with its time,
-- logged in ops.audit_log by the code that sets it; an author mark says the writer is the owner, a
-- relative of the owner or a business, none of whose feedback is shown. Unticked feedback is never
-- shown, so it takes no removal. No name, phone or address is stored here: the first name and town
-- are read from the vault when a page shows the feedback (PRIV-001). The table is new and empty.

CREATE TABLE jobs.feedback (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  job_id uuid NOT NULL UNIQUE REFERENCES jobs.job (id),
  body text NOT NULL CHECK (char_length(btrim(body)) BETWEEN 1 AND 2000),
  share boolean NOT NULL DEFAULT false,
  received_at timestamptz NOT NULL,
  removed_reason text CHECK (removed_reason IN ('personal_details', 'abusive', 'fake', 'unrelated')),
  removed_at timestamptz,
  author_kind text CHECK (author_kind IN ('owner', 'owner_relative', 'business')),
  CHECK ((removed_reason IS NULL) = (removed_at IS NULL)),
  CHECK (removed_reason IS NULL OR share),
  CHECK (removed_at IS NULL OR removed_at >= received_at)
);

-- The schema default gives SELECT and INSERT only (0110). Feedback is never deleted and its words
-- and tick never change; only a removal (set once) and an author mark are written later.
REVOKE UPDATE, DELETE, TRUNCATE ON jobs.feedback FROM askhava_app;
GRANT UPDATE (removed_reason, removed_at, author_kind) ON jobs.feedback TO askhava_app;
