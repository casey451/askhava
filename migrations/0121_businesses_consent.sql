-- businesses (T082): the CON-019 E-SIGN consent records, the CON-020 welcome before the first
-- card and the BIZ-048 recruiting-email stop list (D-191, D-312, D-344, D-374, D-491, D-492).
-- businesses.esign_consent is one append-only row per ticked box: the member who ticked it, the
-- channel it covers (email for job and account notices, text for job-card texts to that member's
-- phones) and the wording version and time shown. There is no call channel: no consent to
-- automated calls is asked for or stored, since none are placed (BIZ-039, D-344).
-- businesses.welcome is one row per activated business: when the welcome email was requested and
-- when it went out; no job card goes before sent_at is set (CON-020).
-- businesses.recruit_stop holds the keyed digest (never the address, PRIV-001) of each address
-- that asked to stop recruiting emails; a row is honored at once and never removed (BIZ-048).
-- All three tables are new and empty.

CREATE TABLE businesses.esign_consent (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  business_id uuid NOT NULL,
  member_id uuid NOT NULL,
  channel text NOT NULL CHECK (channel IN ('email', 'text')),
  wording_version text NOT NULL CHECK (wording_version ~ '^con019-v[0-9]+$'),
  consented_at timestamptz NOT NULL,
  FOREIGN KEY (member_id, business_id) REFERENCES businesses.team_member (id, business_id)
);
CREATE INDEX esign_consent_member_idx ON businesses.esign_consent (business_id, member_id);

CREATE TABLE businesses.welcome (
  business_id uuid PRIMARY KEY REFERENCES businesses.business (id),
  requested_at timestamptz NOT NULL,
  sent_at timestamptz
);

CREATE TABLE businesses.recruit_stop (
  address_key text PRIMARY KEY CHECK (address_key ~ '^[0-9a-f]{64}$'),
  stopped_at timestamptz NOT NULL
);

-- Consent and stop rows are append-only (the schema default gives SELECT and INSERT only); the
-- welcome row's sent_at is set once by the sender.
GRANT UPDATE (sent_at) ON businesses.welcome TO askhava_app;
