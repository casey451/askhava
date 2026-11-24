# askhava-intake: requests, conversations and homeowner consent

## Purpose
The layer-7 crate that turns a homeowner's inbound text into a request (`docs/architecture.md` section 3).

## Never break
- The English consent text equals CON-031, GSM-7 in 2 segments: `tests::consent::td_con_18_english_consent_text_is_con_031_exactly`.
- Spanish consent only while the setting is on: `td_con_18_spanish_consent_text_only_while_spanish_is_on`, `td_txt_24_*`.
- Consent first and at most once per request; STOP and HELP never get it: `tests::prop_consent_first::td_con_01_consent_text_first_and_once_per_request`. More: `(never 23)` in NOTES.md.
- No homeowner text asks for a consent reply: `td_con_24_no_homeowner_text_asks_for_a_consent_reply`; the 911 text equals its templates: `tests::safety_text::td_int_17_*`. More: `(never 24)` in NOTES.md.
- No phone number but 911 in any template or page: `tests/docs/test_no_phone_numbers_in_templates.py`. More: `(never 25)` in NOTES.md.
- A trade answer keeps the keyword job type, never `<trade>_other` (the twin's six warnings): `tests::twin_warnings_regression::td_int_03_*`. More: `(never 6)` in NOTES.md.
- The AI flag never sends the 911 text: `tests::emergency::td_int_10_the_ai_flag_never_stands_in_for_the_stem_match`.
- No card before SAFE, and an emergency is released once: `tests/integration/tests/emergency_flow.rs`, `emergency_choice_flag.rs`. More: `(never 26)` in NOTES.md.
- Consent records cannot be updated, deleted or truncated by the app role: `consent_records_are_append_only_for_the_app_role`. Guarded by `td_con_01_concurrent_first_texts_queue_one_consent_text`. More: `(never 9)` in NOTES.md.
- One consent record per request, none from a reply: `tests/integration/tests/consent_records.rs`. More: `(never 27)` in NOTES.md.
- With the skip threshold unset no question is skipped: `tests::question_bank::td_int_21_*`. More: `(never 28)` in NOTES.md.
- The INT-019 closing text equals the spec: `tests::intake_timeouts::td_pick_33_*`, `tests/integration/tests/intake_timeouts.rs`. More: `(never 29)` in NOTES.md.
- "cancel" alone never closes; two open jobs get "Close which?": `tests::close_cancel::td_job_05_*`, `tests/integration/tests/close_which.rs`; a 911-list reply never reaches `answer_close`: `close_by_text.rs`. More: `(never 30)` in NOTES.md.
- A check-in PROBLEM's scrubbed words are stored before the job closes: `tests/integration/tests/check_in_replies.rs` (TD-JOB-12). More: `(never 31)` in NOTES.md.
- After HELP (CON-034, D-4293): a 911-list match gets the safety text first: `tests/integration/tests/help_answers.rs`. More: `(never 32)` in NOTES.md.
- Web chat texts nothing and calls no AI before the Verify code is checked: `tests/integration/tests/web_chat.rs` (TD-CON-10, TD-AI-10). More: `(never 33)` in NOTES.md.
- PICK-046's notice goes by email only when its text cannot go, once per pick: `tests/integration/tests/pick_lapsed_email.rs` (T446). More: `(never 34)` in NOTES.md.
- PICK-047's `pick_moved` text goes once per ended pick: `tests/integration/tests/pick_moved_text.rs` (T471). More: `(never 35)` in NOTES.md.
- The intake order on texts (D-4360): consent then the ZIP question: `tests/integration/tests/intake_order_area_trade.rs` (TD-INT-45). More: `(never 36)` in NOTES.md.
- Walker corrections (D-4441, D-4602): SAFE and an INT-037 answer are never a ZIP answer (`ask_open_again`, no `zip` row, the re-ask kept); nothing for a contact `may_text` refuses; Guarded by `tests/integration/tests/intake_order_corrections.rs`. More: `(never 20)` in NOTES.md.
- INT-032's two jobs (D-4486 to D-4488): `two_jobs` only after a two-trade first text; Guarded by `tests/integration/tests/intake_order_two_jobs.rs`. More: `(never 21)` in NOTES.md.
- INT-032 corrections (D-4441, D-4778): `two_jobs` right after the first ZIP question only (after SAFE or the INT-037 answer on those paths); Guarded by `tests/integration/tests/intake_order_two_jobs_rules.rs`. More: `(never 22)` in NOTES.md.
- CON-022: the consent sweep clears a vault phone only after `still_unlisted` answered true: `tests/integration/tests/consent_purge.rs` (`td_con_30_*`, T563). More: `(never 37)` in NOTES.md.
- Photos (D-4360, D-5241): no Twilio fetch or delete in the webhook's transaction: `tests/integration/tests/intake_order_photos.rs` (TD-INT-49). More: `(never 38)` in NOTES.md.
- Step 12 (D-5039): no card before SEND or the timer: `tests/integration/tests/intake_order_flow.rs` (TD-INT-47). More: `(never 39)` in NOTES.md.
- `job_answers` never returns a contact answer: `tests/integration/tests/job_answers_reader.rs` (T745). More: `(never 40)` in NOTES.md.
- Web chat after the code (T645): an opt-out lists the number and texts nothing; INT-038 stops the chat: `app/server/tests/web_chat_stops.rs` (TD-CON-36). More: `(never 41)` in NOTES.md.
- Trade questions: "Which fits best?" and `trade_ask` at most twice each: `tests/integration/tests/intake_order_trade_questions.rs`, `intake_boundary_count.rs`. More: `(never 42)` in NOTES.md.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/intake/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/intake/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
