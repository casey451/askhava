-- jobs (T315): the homeowner's feedback words are blanked once, by the 3-year text purge and by
-- "Delete my data" (PRIV-009, PRIV-011, JOB-017; D-2205, D-1821 (4), D-1823).
-- jobs.feedback.body is her own unscrubbed text. The row, its share tick, its removal and its
-- author mark stay (JOB-017's audit trail); only the words go: the body becomes '' and
-- body_purged_at records when. The app role may change those two columns only, and the
-- feedback_body_stays trigger lets them change only from unpurged to purged, once, so a purged
-- body never comes back. The body check keeps 1 to 2,000 characters while unpurged.

ALTER TABLE jobs.feedback ADD COLUMN body_purged_at timestamptz;

ALTER TABLE jobs.feedback DROP CONSTRAINT feedback_body_check;
ALTER TABLE jobs.feedback ADD CONSTRAINT feedback_body_check CHECK (
  CASE WHEN body_purged_at IS NULL THEN char_length(btrim(body)) BETWEEN 1 AND 2000
       ELSE body = '' END
) NOT VALID;
ALTER TABLE jobs.feedback VALIDATE CONSTRAINT feedback_body_check;

GRANT UPDATE (body, body_purged_at) ON jobs.feedback TO askhava_app;

-- The words change only by being blanked, once: an unpurged row may become '' with a purge
-- time; a purged row's body and time never change again (PRIV-009, PRIV-011, D-2205).
CREATE FUNCTION jobs.feedback_body_stays() RETURNS trigger
  LANGUAGE plpgsql AS $$
BEGIN
  IF NEW.body IS DISTINCT FROM OLD.body
      OR NEW.body_purged_at IS DISTINCT FROM OLD.body_purged_at THEN
    IF OLD.body_purged_at IS NOT NULL OR NEW.body_purged_at IS NULL OR NEW.body <> '' THEN
      RAISE EXCEPTION 'feedback words are only ever blanked, once';
    END IF;
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER feedback_body_stays BEFORE UPDATE ON jobs.feedback
  FOR EACH ROW EXECUTE FUNCTION jobs.feedback_body_stays();
