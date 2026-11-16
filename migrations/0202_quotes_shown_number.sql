-- quotes (T361): a quote's shown number, kept once shown (PICK-008, PRIV-005; D-084, D-2632,
-- D-3999). quotes.quote.shown_number is assigned once, under the job's row lock, when the quote is
-- first confirmed (shown to the homeowner): one more than the job's highest; it is never
-- reassigned or cleared, so a later confirm, an AI-read edit waiting for its confirm or a
-- withdrawal keeps it. The Rust paths (quotes::intake::submit_quote, confirm_quote_at) set it in
-- the same statement that confirms; the trigger below only fills a confirmed row they or plain SQL
-- (done tests) left without one, and keeps a number once set. Existing confirmed rows are numbered
-- per job in (received_at, business_id) order, the arrival order the page shows. quotes.quote is
-- small, so the column, backfill and index run in this migration's one transaction (P2). The app
-- role already updates quotes.quote (migration 0070); grants are unchanged. No homeowner detail.

ALTER TABLE quotes.quote ADD COLUMN shown_number integer CHECK (shown_number > 0);

UPDATE quotes.quote q SET shown_number = n.place
  FROM (SELECT job_id, business_id,
               row_number() OVER (PARTITION BY job_id ORDER BY received_at, business_id) AS place
          FROM quotes.quote WHERE confirmed) n
 WHERE q.job_id = n.job_id AND q.business_id = n.business_id;

CREATE UNIQUE INDEX quote_job_shown_number ON quotes.quote (job_id, shown_number);

-- A number once set stays (an UPDATE never moves or clears it); a confirmed row with none gets the
-- job's highest plus one.
CREATE FUNCTION quotes.quote_shown_number() RETURNS trigger
  LANGUAGE plpgsql SET search_path = quotes, pg_temp AS $$
BEGIN
  IF TG_OP = 'UPDATE' AND OLD.shown_number IS NOT NULL THEN
    NEW.shown_number := OLD.shown_number;
  ELSIF NEW.confirmed AND NEW.shown_number IS NULL THEN
    NEW.shown_number := (SELECT coalesce(max(shown_number), 0) + 1
                           FROM quotes.quote WHERE job_id = NEW.job_id);
  END IF;
  RETURN NEW;
END
$$;

CREATE TRIGGER quote_shown_number BEFORE INSERT OR UPDATE OF confirmed, shown_number
  ON quotes.quote FOR EACH ROW EXECUTE FUNCTION quotes.quote_shown_number();
