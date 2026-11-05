# askhava-picks: the homeowner's pick and what follows it

## Purpose

The layer-6 crate that takes the homeowner's pick and carries it to the business's Confirm, the charge and the release (`docs/architecture.md` section 3). T054 built the first part: the page pick's 6-digit Twilio Verify code (10 minutes, 5 tries, a 15-minute cooldown, the PICK-055 send caps) and the "<number> YES" text pick, with the Terms line first, the which-request question, a second YES replacing a waiting pick, the status reply and the CANCEL pause's withdrawal (D-1243 to D-1247). T063 added the Confirm window, the license checks at the pick and at Confirm, the recurring fee line, the YES and CONFIRM reply and the pick notice (D-1500 to D-1504). Not here yet: the page pick itself and the review step (T055), the callers of `check_at_pick`, `check_at_confirm` and `notice_sent` (D-1502, D-1503), expiring a pick when `jobs`' backstop fires with no later send, `all_windows_ended` after an expiry and the PICK-046 notice (D-1503), payments hand-off and release.

## What it owns

- Migration `0079_picks_code.sql`: the `picks` schema and grants; `picks.pick` (job, business, quote number, channel `text`/`page`, inbox ID exactly for `text`, status with the eleven architecture codes, `end_reason` `replaced`/`paused`; one `awaiting_confirm` per job by a partial unique index; no DELETE or TRUNCATE for `askhava_app`), `picks.code_attempt` (one row per Verify code sent: job, vault contact ID, `sms`/`call`, `sent_at`, `tries` 0 to 5, `last_try_at`, `verified_at`; never a number), `picks.verify_cap_day` (append-only, one row per Arizona day the global cap was hit), `picks.terms_line` (append-only: job, outbox row, template version, `queued_at`).
- `src/rules.rs` (pure): `parse_code` (`PickCode`, `Debug` hides digits), `code_state` (`CodeRow`, `CodeState`), `send_allowed` (`SendAsk`, `CodeVia`, `SendCounts`, `SendRefused`), `read_text_pick` (`TextPick`), `plan_text_pick` (`JobView`, `Stage`, `Plan`) and the constants `CODE_VALID`, `CODE_TRIES`, `COOLDOWN`, `SENDS_PER_NUMBER`, `CALLS_PER_NUMBER`, `SENDS_PER_JOB`, `CALL_QUIET_FROM`, `CALL_QUIET_UNTIL`.
- `src/status.rs`: `PickStatus` (`AwaitingConfirm`, `Expired`, `Cancelled`; `code`), `account_confirms`, `awaits_confirm`, `pick_lost`, `confirm_backstop_minutes` (read from `jobs`' Awaiting confirm wait), `stage` (a `JobStatus` to its `Stage`), `pick_move` (Quotes ready to Awaiting confirm). Every match on a status lives here (SEC-031).
- `src/service.rs`: `send_code` (`CodeTo`, `CodeSent`), `check_code` (`CodeCheck`), `text_pick` (`TextPickIn`, `TextPickDone`), `withdraw_paused`, `terms_line_sent`, `record_terms_line`, `terms_line`; template IDs `TERMS_TEMPLATE`, `WHICH_TEMPLATE`, `DONE_TEMPLATE`, `NO_QUOTE_TEMPLATE`, `CONFIRMED_TEMPLATE`, `WAITING_TEMPLATE`, `NONE_TEMPLATE`; event kinds `PICK_MADE_KIND`, `PICK_WITHDRAWN_KIND`; alert kind `VERIFY_CAP_ALERT`.
- `src/models.rs`: `PicksError` and the crate-private queries. `sqlx.toml`: `timestamptz` as jiff timestamps.
- Templates in `app/messaging/templates/` (en, es): `text_pick_terms`, `text_pick_which` (both end with the Terms line and carry the share sentence), `text_pick_done`, `text_pick_no_quote`, `text_pick_confirmed`, `text_pick_waiting`, `text_pick_none`.
- `src/money_notices.rs` (T062, D-1428): `run_card_fix_notice` (the handler on `payments::api::CARD_FIX_KIND`), `BUSINESS_NOTICE_KIND` (`messaging.business_notice`), `CARD_FIX_TEMPLATE`, `CARD_APPROVAL_TEMPLATE`. Templates `card_fix_notice` and `card_approval_notice` in `app/messaging/templates/` (en, es).
- `src/confirm.rs` (T063, D-1500, D-1501, D-1503): `ConfirmSettings` (`from_pairs` reads `WINDOW_KEY`, `ASKHAVA_PICK_CONFIRM_WINDOW_MINUTES`), `confirm_by`, `LicenseAtPick`, `license_at_pick`, `check_at_pick`, `Refusal`, `StoredFacts`, `check_at_confirm`, `FeeBasis`, `fee_line`, `BusinessWord`, `read_business_word`, `ConfirmReply`, `business_reply`; template IDs `PICK_NOTICE_TEMPLATE`, `PICK_NOTICE_EMERGENCY_TEMPLATE`, `FEE_REPLY_TEMPLATE`, `EXPIRED_TEMPLATE` (keys `pick_notice`, `pick_notice_emergency`, `pick_fee_reply`, `pick_expired` in en and es).
- `src/confirm.rs` (T190, D-1522 to D-1524): `WindowRules` (HOLD 120 business minutes, Away up to 240 ahead, emergency 60 and 30 clock minutes), `HoldAsk`, `TimeRefusal`, `hold_allowed`, `hold`, `away_until`, `away_for`, `window_start`, `NightLine`, `EmergencyPlan`, `emergency_plan`, `emergency_fix_by`, `TimeWord`, `read_time_word`. Pure; no caller yet.
- `src/handlers.rs` (T063): `run_pick_notice` (on `PICK_MADE_KIND`, with the worker's `ConfirmSettings`), `notice_sent`, `CONFIRM_ENDS_KIND` (`picks.confirm_ends`) and `run_confirm_ends`; both handlers registered in `server::register_handlers`.
- `src/api.rs`: the public surface; `api::rules` re-exports the pure rules.

## Rules it enforces

- PICK-013, D-101, D-015: `code_state`: open strictly before 10 minutes after the send; the fifth try used starts a 15-minute cooldown from that try, during which `check_code` never calls Verify and `send_code` refuses (`SendRefused::CoolingDown`); after it a new code is needed.
- PICK-013: `check_code` counts the try in a locked transaction before calling Verify, so concurrent tries never pass 5; a passed code is used once.
- PICK-014: `parse_code` takes six ASCII digits once spaces and dashes are dropped; a malformed entry counts no try (the `autocomplete="one-time-code"` field is T187's page).
- PICK-013, D-366: a voice code only for `CodeVia::CallTapped`, the homeowner's tap; never by itself.
- PICK-055, D-480, D-1244: per Arizona calendar day, 5 codes per number (counted by vault contact ID), 3 calls per number, 10 per job; no call from 9 p.m. to 8 a.m. Arizona time unless `own_session`; at or above the caller's global threshold nothing goes and one `picks.verify_daily_cap` High alert is raised per day. Verify is called after the commit, with an `IdempotencyKey` over the send's row ID.
- PICK-015, D-290: `read_text_pick` reads "<n> YES/SÍ/SI" with any words after, or "<request> <n> YES".
- PICK-031, PICK-032, PICK-036, D-068, D-147, D-1246: `plan_text_pick`: a job takes a pick in Quotes ready or Awaiting confirm; a job paused by the homeowner is not counted; more than one not paused asks which, listing their numbers in `{job}` with the first as the `{number}` example; none gives the status reply (confirmed line with CLOSE for Payment pending or Connected, waiting line for a job collecting quotes, the none line otherwise); another quote while one waits replaces it (old pick `cancelled`, `replaced`, a `picks.pick_withdrawn` event for its business; if the business confirmed it meanwhile, `PicksError::MovedOn` and nothing is picked); from Quotes ready the job moves to Awaiting confirm; each new pick enqueues `picks.pick_made` at once.
- PICK-016, D-492: a pick only for a job with a `picks.terms_line` row; otherwise `text_pick_terms` goes and is recorded (template `<id>:<lang>:<12 hex of the SHA256 of the words>`); a text pick is stored with channel `text` and its inbound text; `intake::answer_text_pick` stores the terms acceptance.
- CON-029, D-1245: `withdraw_paused` (called by `intake::on_opt_out` in its transaction on CANCEL or CANCELAR) cancels each waiting pick with `paused` and enqueues the business's notice; job status and timers are untouched; `intake` passes each job's pause, so a paused job's YES is `Plan::Paused` with no pick and no text.
- FEE-020, BIZ-019, BIZ-037, D-492: `run_card_fix_notice` sends the card-fix notice only to `businesses::api::notice_addresses(Money)` (the owner and every "Confirm and billing" member), as opaque address IDs in one `messaging.business_notice` task with the job number and the window's current deadline, deduped per pick and attempt; nothing is sent when `payments::api::card_fix_window` says the pick no longer needs a fix (paid or ended); a bank approval uses `card_approval_notice`; `payments` never names a recipient.
- PICK-019, D-345, D-1500, D-1503: the window is `ConfirmSettings::window_minutes` business minutes (the `ASKHAVA_PICK_CONFIRM_WINDOW_MINUTES` setting) from the notice's real send (`notice_sent`, whose returned deadline is the notice's "Accept by"); `jobs`' backstop is restarted to end one business minute later, once per send; a send after `jobs` returned the job expires the pick and returns `MovedOn`; `run_confirm_ends` expires the pick and returns the job to Quotes ready only while the job's `timer_from` equals its task's `from`; an emergency job keeps `jobs`' 60 clock minutes.
- PICK-020, D-056: one live lookup at the pick (`check_at_pick`); a failed lookup uses a stored Active check under 24 hours old, else `Waits`; `check_at_confirm` never looks up and refuses an ended window, a non-Active account, a reached end or bond date, or a latest check not Active and fresh.
- PICK-033, PICK-035, D-365: `business_reply` never confirms or charges; YES and CONFIRM from a linked phone get the fee and link, after the window the expired reply, any other phone nothing.
- PICK-034, D-196, D-1503: `fee_line` shows the 4-month basis, or the cap when the cap set the fee, in English or Spanish; a zero fee has no line.
- PICK-043, D-252, D-284: `hold` adds `hold_minutes` business minutes to the window's end once, only for the owner or a "Confirm and billing" member, never on an emergency job, a second time, a clock delayed by Away or after the end.
- BIZ-034, D-285: `away_until`/`away_for` allow after now and at most `away_max_minutes` business minutes ahead, owner or "Confirm and billing" only; `window_start` delays only a non-emergency pick.
- PICK-044, D-270, D-346: `emergency_plan` gives 60 clock minutes from the send at any hour, a reminder at 30; without the after-hours tick a night pick's notice waits for 8 a.m., and a day pick whose window runs past 8 p.m. warns and holds the reminder to 8 a.m.; `emergency_fix_by` is 60 clock minutes.
- PICK-037, D-491: `run_pick_notice` sends one notice per pick to `notice_addresses(PicksAndRelease)` only.

## Never break

- Refused at exactly 10:00, cooldown after the fifth wrong try, paste reads: `src/tests/pick_code_rules.rs`, `tests/security/tests/pick_code.rs::td_pick_06_*`.
- Voice only on the tap and never at night from another session; every cap holds; one alert a day; no number stored: `pick_code.rs::pick_055_*`.
- No pick before the Terms line; one acceptance per text pick: `tests/integration/tests/text_pick.rs::td_pick_40_*`.
- Another phone gets nothing; two jobs ask which: `text_pick.rs::td_pick_07_*`.
- A second YES replaces and tells the first business, but never after the business confirmed; confirmed and status replies: `text_pick.rs::td_pick_14_*`.
- CANCEL withdraws the waiting pick, keeps the status, blocks new picks: `text_pick.rs::td_con_12_*`.

- The window, its stale-timer fence, a repeated send restarting nothing, a held notice expiring a dead pick and the license checks: `tests/integration/tests/confirm_window.rs::td_pick_09_*`; the notice's recipients: `td_pick_15_*`; pure cases and the spec templates: `src/tests/confirm.rs`.

## How to change

- A cap or a timer: the spec line first (PICK-013, PICK-055), then the constant in `rules.rs` and both test files.
- A reply's words: the `text_pick_*` keys in both `en.json` and `es.json` with `segments`; any text offering a text pick must keep the share sentence and end with the Terms line; run `cargo test -p askhava-messaging tests::template_registry` and the text-pick test.
- A new pick status: a `PickStatus` variant and its code (already allowed by the table's check), arms in `status.rs` only.
- The Confirm window's length: set `ASKHAVA_PICK_CONFIRM_WINDOW_MINUTES` (or pass another `ConfirmSettings` in a test); the backstop follows (D-1500, D-1503).
- A query: `sqlx migrate run`, then `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `src/tests/pick_code_rules.rs` (TD-PICK-06, TD-PICK-07, TD-PICK-14, TD-PICK-40, TD-CON-12 pure cases): `cargo test -p askhava-picks tests::pick_code_rules`.
- `tests/security/tests/pick_code.rs` (TD-PICK-06, PICK-055): `cargo test -p askhava-security --test pick_code`.
- `tests/integration/tests/text_pick.rs` (TD-PICK-07, TD-PICK-14, TD-PICK-40, TD-TERMS-10, TD-CON-12): `cargo test -p askhava-integration --test text_pick`.
- `src/tests/confirm_hold_away.rs` (TD-PICK-20, TD-PICK-21, TD-PICK-23 pure cases): `cargo test -p askhava-picks tests::confirm_hold_away`; `tests/integration/tests/confirm_hold_away.rs` (stored roles, `jobs`' emergency timers): `cargo test -p askhava-integration --test confirm_hold_away`.
- `src/tests/confirm.rs` (TD-PICK-09, TD-PICK-15, TD-JOB-03, TD-PICK-33, TD-PICK-28 pure cases): `cargo test -p askhava-picks tests::confirm`.
- `tests/integration/tests/confirm_window.rs` (TD-PICK-09, TD-PICK-15, TD-JOB-03): `cargo test -p askhava-integration --test confirm_window`.
- `tests/integration/tests/charge_attempts.rs::td_fee_24_an_emergency_decline_at_22_00_has_60_clock_minutes` (FEE-020 notice): `cargo test -p askhava-integration --test charge_attempts`.
