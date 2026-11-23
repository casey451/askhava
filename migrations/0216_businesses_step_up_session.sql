-- businesses (T812): the session a member's step-up was passed in (SEC-038; D-5314).
-- team_member.step_up_session is the portals.session row ID (never the cookie or its hash) in which
-- the member last passed a Verify code, written with step_up_at in one statement by
-- record_session_step_up. record_step_up writes null: a step-up bound to no session. Nothing reads
-- it yet (T813 makes the step-up check require it). No foreign key: businesses sits below portals,
-- and a swept session leaves a stale ID that matches no live session. A nullable column with no
-- default is a catalog change only, so nothing blocks.

ALTER TABLE businesses.team_member ADD COLUMN step_up_session uuid;
