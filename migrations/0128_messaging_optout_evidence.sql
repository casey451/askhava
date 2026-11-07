-- messaging (T093): the message log's language and the opt-out evidence's message
-- (TXT-019, TXT-020, CON-022, CON-036; D-093, D-1851 to D-1853).
-- messaging.outbox is the outbound message log: the vault reference (`recipient_id`, never the
-- number), the kind (`recipient_kind` with the template ID), the request among `subject_ids`, the
-- sent time, the attempts and, through `provider_sid`, Twilio's status. It gains `language`, the
-- language the stored body was rendered in (`en` or `es`), set by the outbox writer when the row
-- is queued; existing rows and a body that matches neither language's template keep NULL.
-- The row is never deleted by the retention sweep: it keeps its template ID and log fields for the
-- consent-log period (CON-022) once its body is cut (TXT-020).
-- messaging.optout_event gains `inbox_id`, the inbound text that carried a texted opt-out (NULL
-- for every other channel), so the stored detection (`word:<word>`, `phrase:<pattern ID>` or
-- `provider`) stays tied to its message after the body is gone (CON-036). It is an ID, not a
-- foreign key, like `ops.alert`'s subject: the event never depends on the inbox row's lifetime.
-- The table stays append-only for the app role (0044). Both columns are new and nullable; no existing row changes.
ALTER TABLE messaging.outbox
  ADD COLUMN language text CHECK (language IN ('en', 'es'));
ALTER TABLE messaging.optout_event
  ADD COLUMN inbox_id uuid;
