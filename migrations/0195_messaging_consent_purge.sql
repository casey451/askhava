-- messaging (T321): the CON-022 five-year clock and the opt-out events' purge (CON-022, CON-036;
-- D-2208, D-3155, D-3753, D-3967).
-- messaging.inbox_contact records which vault contact each inbound text came from, written once per
-- text by messaging::api::on_inbound_text (ON CONFLICT DO NOTHING): the clock's inbound half, since
-- messaging.inbox holds only the MessageSid and its time. `inbox_id` has no REFERENCES (like
-- messaging.optout_event.inbox_id) and `contact_id` none either (no cross-schema foreign key). The
-- app role may SELECT, INSERT and DELETE (the sweep deletes a number's rows with its inbox rows),
-- never UPDATE. Texts received before this table have no row and count from the outbox only.
-- messaging.last_message_at(contact) is the later of the number's newest inbox_contact.received_at
-- and its newest outbox time (sent, else queued), NULL when it has neither. Only the definer
-- functions call it: no EXECUTE for the app role.
-- messaging.purge_expired_optout_events(contact, digests) deletes, only when that last message is
-- strictly older than now() - 1827 days on the database clock (5 years, never short by a leap
-- day), the number's opt-out events (found by their blind-index digests under every index version)
-- that are themselves strictly older than that, never one a messaging.suppression row references
-- and never the number's newest one (its current STOP or START evidence), and returns their IDs
-- for the caller's audit rows. optout_event stays append-only for the app role (0044): the delete
-- runs as the migrating role through SECURITY DEFINER, with every name schema-qualified and
-- search_path pinned to messaging, pg_temp, so no temporary table can stand in for suppression or
-- outbox. Static SQL only. The new outbox index serves the per-number lookups; no live data exists
-- before Gate C.

CREATE TABLE messaging.inbox_contact (
  inbox_id uuid PRIMARY KEY,
  contact_id uuid NOT NULL,
  received_at timestamptz NOT NULL
);
CREATE INDEX inbox_contact_contact_idx ON messaging.inbox_contact (contact_id, received_at);
REVOKE ALL ON messaging.inbox_contact FROM askhava_app;
GRANT SELECT, INSERT, DELETE ON messaging.inbox_contact TO askhava_app;

CREATE INDEX outbox_recipient_idx ON messaging.outbox (recipient_id, created_at);

CREATE FUNCTION messaging.last_message_at(contact uuid) RETURNS timestamptz
  LANGUAGE sql STABLE SECURITY DEFINER SET search_path = messaging, pg_temp AS $$
  SELECT max(m.at) FROM (
    SELECT max(c.received_at) AS at FROM messaging.inbox_contact c WHERE c.contact_id = $1
    UNION ALL
    SELECT max(coalesce(o.sent_at, o.created_at)) FROM messaging.outbox o WHERE o.recipient_id = $1
  ) AS m
$$;
REVOKE ALL ON FUNCTION messaging.last_message_at(uuid) FROM PUBLIC;

CREATE FUNCTION messaging.purge_expired_optout_events(contact uuid, digests bytea[])
  RETURNS SETOF uuid
  LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path = messaging, pg_temp AS $$
  WITH due AS (
    SELECT now() - interval '1827 days' AS cutoff
    WHERE messaging.last_message_at($1) < now() - interval '1827 days'
  ), newest AS (
    SELECT e.id FROM messaging.optout_event e
    WHERE e.digest = ANY ($2)
    ORDER BY e.effective_at DESC, e.id DESC
    LIMIT 1
  )
  DELETE FROM messaging.optout_event e
  USING due d
  WHERE e.digest = ANY ($2)
    AND e.effective_at < d.cutoff
    AND NOT EXISTS (SELECT 1 FROM newest n WHERE n.id = e.id)
    AND NOT EXISTS (SELECT 1 FROM messaging.suppression s WHERE s.event_id = e.id)
  RETURNING e.id
$$;
REVOKE ALL ON FUNCTION messaging.purge_expired_optout_events(uuid, bytea[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION messaging.purge_expired_optout_events(uuid, bytea[]) TO askhava_app;
