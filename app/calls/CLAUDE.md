# askhava-calls: masked numbers and masked calls

## Purpose

The layer-5 crate for masked calls between a homeowner and the business she connected with (`docs/architecture.md` section 3). T076 built the masked-number pool: each connected job gets a number from the fixed pool in settings, never bought by the app, preferring one no other open line of the same homeowner or business uses, otherwise sharing one with PIN routing and one alert to Casey (D-077, D-160, D-231, D-1679 to D-1682). It also holds the suppression-list check every automated call to a homeowner passes first. T077 added PIN routing and the paid-business read-back (D-1684 to D-1688). T078 added the call log and the never-recorded TwiML (D-1689 to D-1691). T079 added closing lines, the staff-phone approval gate, the LINK request and the Ask Hava number's greeting (D-1694 to D-1698). Not here yet: storing the per-day PIN count (D-1685), the who's-calling text (TXT-027, D-1687), the callers of `close_line` and the daily `close_expired` sweep, stored phone approvals (T148, D-1695), the 45-minute LINK follow-up (D-1696). T080 added relay-friendly PIN prompts, the homeowner's code call, the safety voice call behind its switch and Casey's bridge (D-1704 to D-1708). Not here yet: the inbound-voice route that serves the TwiML, the voice adapter's TwiML URL, the stored safety switch and the handler's registration (T096, T108; D-1708).

## What it owns

- Migration `0114_calls_pool.sql`: the `calls` schema (default privileges SELECT, INSERT only, D-1520); `calls.number_pool` (one row per line: job, homeowner, business by ID, `pool_number`, `shared`, `assigned_at`, `closed_at`; `seq` orders assignments; one open line per job; the app role may UPDATE `closed_at` only, for T079); `calls.pool_alert` (append-only: the `ops.alert` ID and the `seq` of the line that raised it).
- `src/service.rs`: `assign_number` (`Connected`, `Line`: `id`, `number`, `shared`), the pure `choose`, `automated_call_gate` (`AutomatedCall`, `CallGate`), `POOL_ALERT` (`calls.pool_exhausted`), `SAFETY_NOT_DIALABLE_ALERT` (`calls.safety_call_not_dialable`), `ASSIGNED_ACTION` (`calls.number_assigned`).
- `src/routing.rs` (T077): the pure `route` (`Party`, `RoutedLine`, `CallTurn`, `Step`, `Attestation`), `job_pin`, `JobPin`, `answer_call` (`IncomingCall`, `Answer`), `TRIES_PER_CALL`, `TRIES_PER_DAY`, `READBACK_AFTER`, `PIN_ALERT` (`calls.pin_tries_exceeded`); its own query `lines_on`.
- Migration `0116_calls_call_log.sql` (T078): `calls.call_log` (append-only: line, call SID unique, `caller` business or homeowner, `answered_by` person, machine, nobody or unknown, `length_seconds` (bigint), `reached_45s`, `ended_at`).
- `src/rules.rs` (T078): the pure `talked`, `business_attempt`, `answered_by`, `reached_report_mark`, `REPORT_MARK` (`Caller`, `AnsweredBy`).
- `src/handlers.rs` (T078): `dial_twiml`, `finished_call` (`FinishedCall`), `log_finished_call`, `call_log` (`LoggedCall`); its own queries.
- `src/lifecycle.rs` (T079): `close_line` (`CloseReason`), `close_expired`, `expired`, `LINE_DAYS`; the pure `phone_works` and `gate` (`PhoneStanding`); `request_link`, `last4`; `greeting_twiml`, `GREETING_EN`, `GREETING_ES`; `BUSINESS_NOTICE_KIND` (`messaging.business_notice`), `CANCELLED_TEMPLATE`, `LINK_TEMPLATE`, `LINK_TEXT_TO` (`main_phone`); its own queries. Templates `line_closed_cancelled` and `link_this_phone` (en, es) in `app/messaging/templates/`.
- `src/routing.rs` (T080): `PinWait` (`PIN_WAIT_KEY`, `ASKHAVA_PIN_WAIT_SECONDS`, `PIN_WAIT_MIN`), `pin_prompt_twiml`, `pin_heard` (`PinHeard`), `PIN_PROMPT`, `PIN_GOODBYE`.
- `src/voice.rs` (T080): `may_code_call`, `code_call_twiml`; `Approval`, `Conditions`, `SafetySwitch`, `safety_script`, `safety_twiml`, `SafetyCall` (`from_payload`), `SafetyDialer`, `SafetyOutcome`, `place_safety_call` (the `SAFETY_CALL_KIND` handler), `safety_call_key`, `safety_call_answered`, `ring_owner`, `owner_bridge_twiml`; `SAFETY_WITHIN`, `SAFETY_SCRIPT_VERSION`, `SAFETY_CALL_ACTION`, `SAFETY_STOP_ACTION`.
- `src/models.rs`: `CallsError` (T080 adds `Voice`) and the crate-private queries.
- `src/api.rs`: the only public surface. `sqlx.toml`: `timestamptz` as jiff timestamps.

## Rules it enforces

- CALL-001, CALL-002: `service::assign_number` gives a connected job one `PoolNumber`; `Line` carries only that number, never either side's phone.
- CALL-017, D-077, D-160: numbers come only from `askhava_adapters::api::MaskedPool` (the settings list, 10 by default); `service::choose` takes the number with fewest open lines of the same homeowner or business, then fewest open lines, then settings order; a number used by the same homeowner or business is `shared` (PIN routing, CALL-004). `Line::shared` and the stored `shared` column mean "shared when assigned" and are never updated: an unshared line becomes shared in fact when a later line takes its number, so PIN routing (T077) must derive sharing from the open lines on the number, never trust the flag (D-1680). A repeat call for a job returns its open line only for the same business and homeowner, else `LineOpen`; an empty pool is `PoolEmpty`.
- CALL-017, D-231, D-1680: a shared assignment raises one High `calls.pool_exhausted` alert unless one is open; an alert is open until a later assignment finds a free number (`models::alert_open`). All under one advisory lock (`models::lock_pool`), with the audit row, in one transaction.
- CALL-021, D-296: no number buying and no Proxy; the voice adapter dials from a `PoolNumber` only.
- CON-035, CALL-014, CALL-022, D-1681: `service::automated_call_gate` checks the one suppression list through `messaging::api::send_window` before a safety call or a code call not asked for on screen; only a code call she just asked for on screen skips it. It reuses the text-send window, so the vault audits the read as a `Send` read, and a number `messaging` would not text (not `+1`, or an area code missing from the quiet-hours table) answers `NotDialable`; for a safety call that raises one High `calls.safety_call_not_dialable` alert about the contact (D-1682).

- CALL-003, CALL-004, D-1684: a party with exactly one open line on the number connects; anyone else, or two or more lines, needs the PIN (`job_pin`: line ID mod 10,000), and only one exact match connects (a collision connects neither).
- CALL-025, D-1685: 3 tries a call, 6 a day per number and pool number (`CallTurn::tries_today` comes from the caller); the last try hangs up with one High `calls.pin_tries_exceeded` alert; a used-up number is `Refuse`d before the prompt.
- CALL-005, D-026, D-1686: after the second wrong try, only a phone linked to a business holding a line on the number, on an A-attested call, gets `ReadBack` of its newest line; the words come from `privacy::api::read_for_readback` (first name and street only); the voice handler connects only on the caller's confirm.

- CALL-006, D-1691: calls are never recorded: `dial_twiml` says `record="do-not-record"` and holds the phone as a `SecretString`; no source names a recording verb or callback.
- CALL-007, CALL-008, D-141, D-1689, D-1690: a person answering, either way and any length, is `talked`; a machine, nobody or an unknown answerer (no `human` detection, D-1690) never is; only the business's calls are `business_attempt`s; the 45-second mark is stored (`reached_45s`) for reports only. Each finished call is logged once per SID with who called (role only), who answered and its length.

- CALL-009, CALL-010, D-1694: `close_line` sets only `closed_at` on the job's open line, with one audit row naming the reason (`calls.line_closed_*`), in one transaction; a repeat returns `None`; only `Cancelled` queues the business notice (owner and "Confirm and billing" members, deduped per line); no close records a strike. `close_expired` closes lines open 30 days or more.
- CALL-012, BIZ-033, D-1695: `gate` makes a business phone that a non-owner linked and the owner has not approved `Party::Unlinked`, so it hears the PIN prompt; the voice handler must call it before `route`.
- CALL-011, D-078, D-1696: `request_link` names only the last 4 digits and the line's PIN, emailed (`to`) to the owner and "Confirm and billing" members and texted only to the main phone when the business chose text (`text_to: LINK_TEXT_TO`); once per line, with an ID-only dedupe key, a repeat returning `false`; never the full number; the template's `{number}` is the last 4 digits and `{job}` the PIN.
- CALL-020, D-227, D-1697: `greeting_twiml` says the greeting in English, then Spanish, then hangs up; no record, dial, gather or voicemail verb.
- CALL-018: `assign_number` never reads the homeowner's contact channel.

- CALL-013, CALL-023, D-1704: every PIN prompt is spoken words (no `Play`) in a `Gather` waiting `PinWait` (at least 10 seconds), said once more, then a goodbye and hang-up; the prompt and read-back answer only the calling leg; `dial_twiml` has no whisper `url`.
- CALL-014, CALL-015, D-366: `may_code_call` only on her tap and only to a homeowner; `code_call_twiml` opens "This is Ask Hava LLC", slow prosody, the code twice, the main number as callback (`None` when it equals the calling number), "Press 1 to hear it again".
- CALL-019, CALL-022, D-384, D-1705: `SafetySwitch` is off unless `turn_on` gets an approval with a reference and every `Conditions` item; `place_safety_call` checks switch, 30 minutes, calling number not main, then the suppression list, dials once (key on the alert) and audits `calls.safety_call_placed` with `safety_script_v1`; "9" lists the number (`safety_call_key`).
- OPS-028, D-1706, D-1707: `safety_call_answered` acknowledges the critical alert through `ops::api::acknowledge_alert` only for a person; `ring_owner` rings Casey from a pool number and `owner_bridge_twiml` reads him the INT-004 words, then dials her with the Ask Hava number shown, never recorded.

## Never break

- Each job gets a pool number neither side's own, once: `tests/integration/tests/number_pool.rs::td_call_01_each_connected_job_gets_a_pool_number_neither_sides_own`.
- Same homeowner or business gets another number while one is free; concurrent jobs too: `td_call_01_same_homeowner_or_business_gets_another_number`, `td_call_01_two_jobs_at_once_get_two_numbers`.
- At most one pool alert open: `td_call_01_an_exhausted_pool_shares_with_pin_routing_and_alerts_once`.
- No buy-number method, no Proxy endpoint: `td_call_01_the_voice_adapter_has_no_buy_number_method`.
- No automated call after an opt-out: `td_con_28_no_automated_call_after_an_opt_out`.
- An untextable number is not dialled, and a safety call to one alerts: `td_con_28_an_untextable_number_is_not_dialled_and_a_safety_call_alerts`.
- A repeat call for a job with another business or homeowner is refused: `td_call_01_each_connected_job_gets_a_pool_number_neither_sides_own`.
- A line's number never changes and alerts are append-only for the app role: `td_call_01_lines_keep_their_number_and_alerts_are_append_only`.

- A wrong PIN never connects; two jobs route only by PIN; the read-back only for the paid business on attestation A; 10 wrong PINs never connect: `tests/integration/tests/pin_routing.rs` (`td_call_02_*`, `td_call_11_*`).

- Never recorded; one log row per call; the log is append-only: `tests/integration/tests/no_recording.rs` (`td_call_03_*`, `td_call_04_each_finished_call_is_logged_once`).
- Talking is a person answering, never a machine, never the length: `app/calls/src/tests/connection.rs` (`td_call_04_*`).

- Lines close on cancel, no-show, complaint and at 30 days, a cancel tells the business once and no strike is written; a blocked phone hears the PIN prompt and LINK names its last 4 digits; an email-preferred homeowner gets a line; a removed member's phones hear the PIN prompt; the greeting never records or forwards; an unapproved member-linked phone does not connect: `tests/integration/tests/line_lifecycle.rs` (`td_call_05_*`, `td_call_07_*`, `td_call_08_*`).

- PIN prompts, code call, safety switch and call, Casey's bridge: `tests/integration/tests/voice_calls.rs` (`td_call_06_*`, `td_call_09_*`, `td_txt_09_*`, `ops_028_*`). Never add a `url` to `dial_twiml`'s `Number` or a `Play` to a prompt.

## How to change

- Pool size: set `ASKHAVA_MASKED_POOL_SIZE` and `ASKHAVA_MASKED_NUMBERS` (Casey buys the numbers); the default is `adapters`' `DEFAULT_POOL_SIZE` (CALL-017, D-160).
- How a number is chosen: CALL-017 first, then `service::choose` and the `td_call_01_*` cases.
- Close a line (T079): `UPDATE calls.number_pool SET closed_at` only; nothing else is granted.
- A new close reason: a `CloseReason` variant with its `action` and, if it tells the business, `tells_business` and a template in both languages.
- The PIN wait: set `ASKHAVA_PIN_WAIT_SECONDS` (10 to 60). The safety script: CALL-022 first, then `safety_script` and a new `SAFETY_SCRIPT_VERSION`.
- A new automated call kind: add an `AutomatedCall` variant; it passes the list check unless the spec names an exception.
- A query: `sqlx migrate run`, then `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `tests/integration/tests/number_pool.rs` (TD-CALL-01, TD-CON-28's T076 case): `cargo test -p askhava-integration --test number_pool`.
- `tests/integration/tests/pin_routing.rs` (TD-CALL-02, TD-CALL-11, TD-CALL-12): `cargo test -p askhava-integration --test pin_routing`.
- `app/calls/src/tests/connection.rs` (TD-CALL-04, TD-FEE-13's CALL-007 cases): `cargo test -p askhava-calls tests::connection`.
- `tests/integration/tests/no_recording.rs` (TD-CALL-03, TD-CALL-04's log): `cargo test -p askhava-integration --test no_recording`.
- `tests/integration/tests/line_lifecycle.rs` (TD-CALL-05, TD-CALL-07, TD-CALL-08's gate, TD-BIZ-34's removal case): `cargo test -p askhava-integration --test line_lifecycle`.
- `tests/integration/tests/schema_grants.rs` (TD-SEC-34): the `calls` default ACL and grants.
- `tests/integration/tests/voice_calls.rs` (TD-CALL-06, TD-CALL-09, TD-TXT-09's T080 cases, OPS-028): `cargo test -p askhava-integration --test voice_calls`.
