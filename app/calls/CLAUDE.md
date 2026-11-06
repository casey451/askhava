# askhava-calls: masked numbers and masked calls

## Purpose

The layer-5 crate for masked calls between a homeowner and the business she connected with (`docs/architecture.md` section 3). T076 built the masked-number pool: each connected job gets a number from the fixed pool in settings, never bought by the app, preferring one no other open line of the same homeowner or business uses, otherwise sharing one with PIN routing and one alert to Casey (D-077, D-160, D-231, D-1679 to D-1682). It also holds the suppression-list check every automated call to a homeowner passes first. T077 added PIN routing and the paid-business read-back (D-1684 to D-1688). T078 added the call log and the never-recorded TwiML (D-1689 to D-1691). Not here yet: storing the per-day PIN count (D-1685), the who's-calling text (TXT-027, D-1687), closing lines, staff phones and LINK (T079), bridging and the identity and code calls (T080).

## What it owns

- Migration `0114_calls_pool.sql`: the `calls` schema (default privileges SELECT, INSERT only, D-1520); `calls.number_pool` (one row per line: job, homeowner, business by ID, `pool_number`, `shared`, `assigned_at`, `closed_at`; `seq` orders assignments; one open line per job; the app role may UPDATE `closed_at` only, for T079); `calls.pool_alert` (append-only: the `ops.alert` ID and the `seq` of the line that raised it).
- `src/service.rs`: `assign_number` (`Connected`, `Line`: `id`, `number`, `shared`), the pure `choose`, `automated_call_gate` (`AutomatedCall`, `CallGate`), `POOL_ALERT` (`calls.pool_exhausted`), `SAFETY_NOT_DIALABLE_ALERT` (`calls.safety_call_not_dialable`), `ASSIGNED_ACTION` (`calls.number_assigned`).
- `src/routing.rs` (T077): the pure `route` (`Party`, `RoutedLine`, `CallTurn`, `Step`, `Attestation`), `job_pin`, `JobPin`, `answer_call` (`IncomingCall`, `Answer`), `TRIES_PER_CALL`, `TRIES_PER_DAY`, `READBACK_AFTER`, `PIN_ALERT` (`calls.pin_tries_exceeded`); its own query `lines_on`.
- Migration `0116_calls_call_log.sql` (T078): `calls.call_log` (append-only: line, call SID unique, `caller` business or homeowner, `answered_by` person, machine or nobody, `length_seconds`, `reached_45s`, `ended_at`).
- `src/rules.rs` (T078): the pure `talked`, `business_attempt`, `answered_by`, `reached_report_mark`, `REPORT_MARK` (`Caller`, `AnsweredBy`).
- `src/handlers.rs` (T078): `dial_twiml`, `finished_call` (`FinishedCall`), `log_finished_call`, `call_log` (`LoggedCall`); its own queries.
- `src/models.rs`: `CallsError` and the crate-private queries.
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
- CALL-007, CALL-008, D-141, D-1689, D-1690: a person answering, either way and any length, is `talked`; a machine or nobody never is; only the business's calls are `business_attempt`s; the 45-second mark is stored (`reached_45s`) for reports only. Each finished call is logged once per SID with who called (role only), who answered and its length.

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

## How to change

- Pool size: set `ASKHAVA_MASKED_POOL_SIZE` and `ASKHAVA_MASKED_NUMBERS` (Casey buys the numbers); the default is `adapters`' `DEFAULT_POOL_SIZE` (CALL-017, D-160).
- How a number is chosen: CALL-017 first, then `service::choose` and the `td_call_01_*` cases.
- Close a line (T079): `UPDATE calls.number_pool SET closed_at` only; nothing else is granted.
- A new automated call kind: add an `AutomatedCall` variant; it passes the list check unless the spec names an exception.
- A query: `sqlx migrate run`, then `cargo sqlx prepare --workspace -- --all-targets --all-features`.

## Tests that guard it

- `tests/integration/tests/number_pool.rs` (TD-CALL-01, TD-CON-28's T076 case): `cargo test -p askhava-integration --test number_pool`.
- `tests/integration/tests/pin_routing.rs` (TD-CALL-02, TD-CALL-11, TD-CALL-12): `cargo test -p askhava-integration --test pin_routing`.
- `app/calls/src/tests/connection.rs` (TD-CALL-04, TD-FEE-13's CALL-007 cases): `cargo test -p askhava-calls tests::connection`.
- `tests/integration/tests/no_recording.rs` (TD-CALL-03, TD-CALL-04's log): `cargo test -p askhava-integration --test no_recording`.
- `tests/integration/tests/schema_grants.rs` (TD-SEC-34): the `calls` default ACL and grants.
