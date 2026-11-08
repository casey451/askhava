-- server (T254): the rows TD-PICK-49 and TD-PICK-50 start from that no module API can make
-- without a whole homeowner and quote flow (D-2013): one active business, four jobs awaiting
-- confirm with their numbers, a waiting pick on each of the first three and a cancelled one on
-- the fourth, and the first pick's PaymentIntent with one charge attempt sent. Fake IDs only;
-- the homeowner's contact, her request, the owner and the $0 decisions are made through the
-- module APIs in the test.
INSERT INTO businesses.business (id, status, created_at, status_changed_at)
VALUES ('00000000-0000-0000-0000-000000254b01', 'active', '2026-11-02T15:00:00Z', '2026-11-02T15:00:00Z');

INSERT INTO jobs.job (id, status, emergency, created_at, status_at, timer_from) VALUES
  ('00000000-0000-0000-0000-000000254a01', 'awaiting_confirm', false, '2026-11-02T16:00:00Z', '2026-11-02T16:00:00Z', '2026-11-02T16:00:00Z'),
  ('00000000-0000-0000-0000-000000254a02', 'awaiting_confirm', false, '2026-11-02T16:00:00Z', '2026-11-02T16:00:00Z', '2026-11-02T16:00:00Z'),
  ('00000000-0000-0000-0000-000000254a03', 'awaiting_confirm', false, '2026-11-02T16:00:00Z', '2026-11-02T16:00:00Z', '2026-11-02T16:00:00Z'),
  ('00000000-0000-0000-0000-000000254a04', 'quotes_ready', false, '2026-11-02T16:00:00Z', '2026-11-02T16:00:00Z', '2026-11-02T16:00:00Z');

INSERT INTO jobs.job_number (number, job_id, issued_at) VALUES
  (2541, '00000000-0000-0000-0000-000000254a01', '2026-11-02T16:00:00Z'),
  (2542, '00000000-0000-0000-0000-000000254a02', '2026-11-02T16:00:00Z'),
  (2543, '00000000-0000-0000-0000-000000254a03', '2026-11-02T16:00:00Z'),
  (2544, '00000000-0000-0000-0000-000000254a04', '2026-11-02T16:00:00Z');

INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, picked_at, ended_at, end_reason) VALUES
  ('00000000-0000-0000-0000-000000254c01', '00000000-0000-0000-0000-000000254a01', '00000000-0000-0000-0000-000000254b01', 1, 'page', 'awaiting_confirm', '2026-11-02T16:10:00Z', NULL, NULL),
  ('00000000-0000-0000-0000-000000254c02', '00000000-0000-0000-0000-000000254a02', '00000000-0000-0000-0000-000000254b01', 1, 'page', 'awaiting_confirm', '2026-11-02T16:10:00Z', NULL, NULL),
  ('00000000-0000-0000-0000-000000254c03', '00000000-0000-0000-0000-000000254a03', '00000000-0000-0000-0000-000000254b01', 1, 'page', 'awaiting_confirm', '2026-11-02T16:10:00Z', NULL, NULL),
  ('00000000-0000-0000-0000-000000254c04', '00000000-0000-0000-0000-000000254a04', '00000000-0000-0000-0000-000000254b01', 1, 'page', 'cancelled', '2026-11-02T16:10:00Z', '2026-11-02T16:20:00Z', 'replaced');

INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, stripe_intent_id, emergency, created_at)
VALUES ('00000000-0000-0000-0000-000000254c01', '00000000-0000-0000-0000-000000254b01', '00000000-0000-0000-0000-000000254a01', 7500, 'pi_fake254paid', false, '2026-11-02T16:30:00Z');

INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
VALUES ('00000000-0000-0000-0000-000000254c01', 1, '00000000-0000-0000-0000-000000254d01', 'sent', '2026-11-02T16:30:00Z');

INSERT INTO payments.customer (business_id, stripe_customer_id)
VALUES ('00000000-0000-0000-0000-000000254b01', 'cus_fake254');
