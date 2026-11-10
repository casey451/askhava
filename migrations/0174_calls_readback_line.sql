-- calls (T311): the line a read-back named, bound to its try (CALL-005, CALL-002; D-026, D-2201).
-- calls.pin_call.readback_line: the ID of the line whose homeowner's first name and street were read
-- back at that try of the call, written by the same INSERT that records the try (in the try's
-- transaction), so no UPDATE is granted. The confirm press connects only this line, and only while
-- it is still open on the call's pool number and still the caller's business's; Twilio's resend of
-- the read-back turn reads this line back again. NULL for every try that read nothing back.
-- A line ID only: no contact value (PRIV-002). The read-back comes only at try 2 (READBACK_AFTER).
-- The table is empty before launch; the foreign key is added NOT VALID then validated (P2). The
-- calls default privileges already give SELECT and INSERT (D-1520); nothing else is granted.

ALTER TABLE calls.pin_call ADD COLUMN readback_line uuid;
ALTER TABLE calls.pin_call ADD CONSTRAINT pin_call_readback_line_fkey
  FOREIGN KEY (readback_line) REFERENCES calls.number_pool (id) NOT VALID;
ALTER TABLE calls.pin_call VALIDATE CONSTRAINT pin_call_readback_line_fkey;
ALTER TABLE calls.pin_call ADD CONSTRAINT pin_call_readback_try
  CHECK (readback_line IS NULL OR try_no = 2);
