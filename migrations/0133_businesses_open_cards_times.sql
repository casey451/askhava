-- businesses (T085): each person's open-cards email times (CARD-022, D-491, D-492). One row per
-- team member who changed the default: which of 8 a.m., noon and 5 p.m. Arizona time they get the
-- open-cards email, at most two; all three false is "off". No row means the default, 5 p.m. only.
-- No contact value is stored (PRIV-001). A row is replaced in place, never deleted.

CREATE TABLE businesses.open_cards_time (
  member_id uuid PRIMARY KEY REFERENCES businesses.team_member (id),
  at_8am boolean NOT NULL,
  at_noon boolean NOT NULL,
  at_5pm boolean NOT NULL,
  changed_at timestamptz NOT NULL,
  CONSTRAINT open_cards_time_at_most_two CHECK (at_8am::int + at_noon::int + at_5pm::int <= 2)
);

GRANT SELECT, INSERT, UPDATE ON businesses.open_cards_time TO askhava_app;
REVOKE DELETE, TRUNCATE ON businesses.open_cards_time FROM askhava_app;
