-- payments (T061): free first wins and their entity keys (FEE-010, FEE-011, FEE-012, FEE-013,
-- D-019, D-069, D-151). payments.entity_key holds each business's free-win entity keys (ROC
-- number, city-list entity, card fingerprint, identity-code contact, city-list owner name) as a
-- keyed HMAC digest only, never the value (PRIV-002, postgres brain P12); keys are never removed.
-- payments.free_win is a used free first win, one per business, with the fee it waived (FEE-012)
-- next to its $0 ledger entry; payments.free_win_claim holds every key of a business whose win
-- was used, so a second business sharing any key finds it claimed (FEE-011). businesses.business
-- and jobs.job are other schemas, so no foreign key there (architecture section 4 rule 3). The
-- tables are new and empty. All three are append-only for the app role (insert and select only).

CREATE TABLE payments.entity_key (
  business_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN (
    'roc', 'city-entity', 'card-fingerprint', 'identity-contact', 'owner-name'
  )),
  digest text NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
  recorded_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (business_id, kind, digest)
);
CREATE INDEX entity_key_digest_idx ON payments.entity_key (kind, digest);

CREATE TABLE payments.free_win (
  business_id uuid PRIMARY KEY,
  job_id uuid NOT NULL,
  ledger_entry_id uuid NOT NULL UNIQUE REFERENCES payments.ledger_entry (id),
  waived_cents bigint NOT NULL CHECK (waived_cents > 0),
  used_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE payments.free_win_claim (
  kind text NOT NULL,
  digest text NOT NULL,
  business_id uuid NOT NULL REFERENCES payments.free_win (business_id),
  PRIMARY KEY (kind, digest)
);

GRANT SELECT, INSERT ON payments.entity_key, payments.free_win, payments.free_win_claim
  TO askhava_app;
REVOKE UPDATE, DELETE, TRUNCATE ON payments.entity_key, payments.free_win, payments.free_win_claim
  FROM askhava_app;
