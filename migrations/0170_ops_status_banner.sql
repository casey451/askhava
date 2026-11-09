-- ops (T279): the WEB-007 site status banner, hidden by default, with who or what turned it on
-- (TXT-024, WEB-007; D-144, D-1843). ops.status_banner is append-only history: each change inserts a
-- row and the banner's state is the latest row (none: hidden). The system turns it on for an
-- emergency message while homeowner texting is off or suspended, naming that `messaging.inbox`
-- row; only Casey (actor `owner`) turns it off. The app role keeps SELECT and INSERT only
-- (postgres brain P5). IDs and code names only (PRIV-028). The table is new and empty.

CREATE TABLE ops.status_banner (
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  shown boolean NOT NULL,
  actor_kind text NOT NULL CHECK (actor_kind IN ('system', 'owner')),
  actor_id uuid,
  cause_kind text CHECK (cause_kind ~ '^[a-z][a-z0-9_.]*$'),
  cause_id uuid,
  changed_at timestamptz NOT NULL,
  CHECK ((actor_kind = 'system') = (actor_id IS NULL)),
  CHECK (shown OR actor_kind = 'owner'),
  CHECK ((cause_kind IS NULL) = (cause_id IS NULL))
);
GRANT SELECT, INSERT ON ops.status_banner TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON ops.status_banner FROM askhava_app;
