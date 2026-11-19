-- intake (T563): the CON-022 five-year purge of consent records and the consent sweep's phone wait
-- (CON-022, CON-036; D-079, D-2208, D-3753, D-3967, D-4387, D-4418).
-- intake.purge_expired_consent(contact) deletes, only when the number's last message
-- (messaging.last_message_at, migration 0195) is strictly older than now() - 1827 days on the
-- database clock (5 years, never short by a leap day), that contact's intake.consent_record rows
-- (through intake.homeowner.contact_id) whose consented_at is itself strictly older than that, and
-- returns their IDs for the sweep's ops audit rows. consented_at is app-written, so row age alone
-- never decides: a 6-year-old record of a number texted yesterday stays. consent_record stays
-- append-only for the app role (0053): the delete runs as the migrating role through SECURITY
-- DEFINER, with every name schema-qualified and search_path pinned to intake, pg_temp.
-- intake.consent_phone_wait holds a due contact whose vault phone the sweep kept because a
-- configured index key version had no digest for it yet (a rotation not yet run): the rest of the
-- number's purge ran, so no message row is left to list it again, and the next run finds it here
-- (D-4387, D-4418). intake.consent_phone_due(contact) answers whether the sweep may treat the
-- number as past CON-022's 5 years: its last message strictly older than 1,827 days, or, when it
-- has no message left at all, a wait row for it. Both functions read now() themselves. The only
-- other module named is messaging.last_message_at( (CROSS_SCHEMA_CALLS in
-- tests/integration/tests/architecture_migrations.rs; intake may call messaging, kit/layers.json).
-- Static SQL only.

CREATE TABLE intake.consent_phone_wait (
  contact_id uuid PRIMARY KEY,
  since timestamptz NOT NULL
);
REVOKE ALL ON intake.consent_phone_wait FROM askhava_app;
GRANT SELECT, INSERT, DELETE ON intake.consent_phone_wait TO askhava_app;

CREATE FUNCTION intake.purge_expired_consent(contact uuid) RETURNS SETOF uuid
  LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path = intake, pg_temp AS $$
  DELETE FROM intake.consent_record r
  USING intake.homeowner h
  WHERE h.contact_id = $1
    AND r.homeowner_id = h.id
    AND r.consented_at < now() - interval '1827 days'
    AND messaging.last_message_at($1) < now() - interval '1827 days'
  RETURNING r.id
$$;
REVOKE ALL ON FUNCTION intake.purge_expired_consent(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION intake.purge_expired_consent(uuid) TO askhava_app;

CREATE FUNCTION intake.consent_phone_due(contact uuid) RETURNS boolean
  LANGUAGE sql STABLE SECURITY DEFINER SET search_path = intake, pg_temp AS $$
  SELECT coalesce(
    messaging.last_message_at($1) < now() - interval '1827 days',
    EXISTS (SELECT 1 FROM intake.consent_phone_wait w WHERE w.contact_id = $1)
  )
$$;
REVOKE ALL ON FUNCTION intake.consent_phone_due(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION intake.consent_phone_due(uuid) TO askhava_app;
