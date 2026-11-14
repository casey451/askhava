# askhava-calls: masked numbers and masked calls

## Purpose
The layer-5 crate for masked calls between a homeowner and the business she connected with (`docs/architecture.md` section 3).

## Never break
- Each job gets a pool number neither side's own, once: `tests/integration/tests/number_pool.rs::td_call_01_each_connected_job_gets_a_pool_number_neither_sides_own`.
- No brand word is written in `voice.rs` or `lifecycle.rs`: the greetings and the code call say `brand().legal_name()` (Spanish `brand().es().legal_name()`), the safety script `spoken_legal_name` and `display_name` (WORD-001, D-1932, T292); `tests/docs/test_brand_literals.py` fails on a new literal.
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
- A start is never moved by a repeat or late callback, a lost webhook start is created by the callback, and `call_log` returns the joined start: `tests/integration/tests/call_start.rs` (`td_call_15_*`).
- Stored PINs unique on a number, no ID relation, the pool alert on used-up draws, 12 concurrent wrong tries never past 6 with one alert, repeats not counted, Arizona midnight: `tests/integration/tests/call_pins.rs` (`td_call_13_*`) and `src/pins.rs` `tests` (TD-CALL-20, T349: a failed draw costs one draw, rejected values are drawn again, a failing byte source gives `None`: `td_call_20_*`).
- PIN prompts, code call, safety switch and call, Casey's bridge: `tests/integration/tests/voice_calls.rs` (`td_call_06_*`, `td_call_09_*`, `td_txt_09_*`, `ops_028_*`). Never add a `url` to `dial_twiml`'s `Number` or a `Play` to a prompt.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/calls/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/calls/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
