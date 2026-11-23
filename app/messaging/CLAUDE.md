# askhava-messaging: texts, emails and letters

## Purpose
The layer-3 crate that sends and receives messages (`docs/architecture.md` section 3).

## Never break
- Every text begins `<short_name>:` from the brand file in both languages: `tests::template_registry::every_kind_has_both_languages_and_begins_ask_hava`. More: `(never 1)` in NOTES.md.
- Spanish never reaches a person while the setting is off, except `spanish_stays_on` texts: `tests::template_registry::spanish_only_*`. More: `(never 2)` in NOTES.md.
- Both consent texts stay GSM-7 and 2 segments: `tests::template_registry::recorded_segments_hold_and_the_consent_texts_are_gsm7`.
- No emoji, shortener, off-domain link, promotion wording or "reply CANCEL": `tests/docs/test_template_scan.py`.
- A new any-hour message kind goes only in `rules::night_exception` after QH-004 lists it: `tests::quiet_hours::td_qh_02_*`. More: `(never 5)` in NOTES.md.
- An area code outside the table is refused: `tests::quiet_hours::td_qh_01_unknown_area_code_never_texted`. More: `(never 6)` in NOTES.md.
- Window edges and daylight saving: `tests::quiet_hours::td_qh_01_*`, `tests::prop_quiet_hours_props`.
- A killed worker's row is never resent blind: `tests/integration/tests/outbox.rs::td_txt_04_*`. More: `(never 8)` in NOTES.md.
- No text to a past customer who has not written first: `td_txt_03_no_text_to_a_past_customer_who_has_not_written_first`.
- No plaintext phone in the sender directory: `the_sender_directory_matches_linked_phones_only`.
- A homeowner is re-checked at send, texted at her vault number, and a sent row keeps no body: `td_txt_03_a_homeowner_is_rechecked_and_texted_at_her_vault_number`.
- A row goes only to its recipient kind; subject order does not change the key: `a_row_goes_only_to_its_recipient_kind_and_subject_order_is_one_text`.
- One inbound MessageSid three times is one inbox row: `tests/integration/tests/inbox.rs::td_txt_04_*`. More: `(never 13)` in NOTES.md.
- Each status is recorded once and never moves a text back: `td_txt_04_delivery_statuses_are_recorded`.
- The app's word lists equal the provider's settings file: `tests::optout_words::td_con_20_*`. More: `(never 15)` in NOTES.md.
- An opt-out word in another sense is not an opt-out: `tests::optout_words::td_con_04_*`. More: `(never 16)` in NOTES.md.
- Nothing but the confirmation within 5 minutes reaches a listed number: `tests/integration/tests/optout_flow.rs`. More: `(never 17)` in NOTES.md.
- A 21610 refusal is never retried: `optout_flow.rs::td_con_05_a_21610_refusal_is_never_retried`.
- Two opt-outs at once queue one confirmation, and only the current listing's goes: `optout_flow.rs::td_con_05_*`. More: `(never 19)` in NOTES.md.
- No outbound text from any number but the audience's configured one: `web_email_mode.rs::td_txt_08_*`, `td_txt_17_*`. More: `(never 20)` in NOTES.md.
- No number in the message log and no body left at 30 days: `tests/integration/tests/message_log_retention.rs`. More: `(never 21)` in NOTES.md.
- The HELP reply's contents: `tests/integration/tests/help_start.rs`. More: `(never 22)` in NOTES.md.
- A mailbox code email's stored row never holds the code: `tests/integration/tests/signup_mail_paths.rs` (T468). More: `(never 23)` in NOTES.md.
- A repeat `check_webhook_silence` raises no second alert: `tests/integration/tests/alert_dedupe.rs` (T320). More: `(never 24)` in NOTES.md.
- `mark_replayed_sent` sends nothing and marks a row once: `tests/integration/tests/replay_helpers.rs` (T481). More: `(never 25)` in NOTES.md.
- CON-024's opt-out counts and alert: `tests/integration/tests/consent_records_alerts.rs` (T107). More: `(never 26)` in NOTES.md.
- `Notice::PickLapsed` names only the lapsed business and her quote page (PICK-046): `tests/integration/tests/pick_lapsed_email.rs` (T446). More: `(never 27)` in NOTES.md.
- CON-022: no referenced or newest opt-out event is purged: `tests/integration/tests/consent_purge.rs` (T321). More: `(never 28)` in NOTES.md.
- No index key version is retired while a suppression row is held under it: `tests/integration/tests/suppression_key_versions.rs`. More: `(never 29)` in NOTES.md.
- An opt-out and the consent sweep's re-check wait for each other: `tests/integration/tests/optout_number_lock.rs` (T701). More: `(never 30)` in NOTES.md.
- Rekeying removes only re-indexed rows and logs no phone or digest: `tests/integration/tests/suppression_rekey.rs` (T743). More: `(never 31)` in NOTES.md.
- A queued email gets one provider call per lease: `tests/integration/tests/email_due.rs` (T470). More: `(never 32)` in NOTES.md.
- A business row is sent only from the business number after `may_text`: `tests/integration/tests/outbox_business_rows.rs` (T573). More: `(never 33)` in NOTES.md.
- `suppress_indexed` lists only by the digests given, in the caller's transaction: `app/server/tests/web_chat_stops.rs`. More: `(never 34)` in NOTES.md.
- Only `UPDATE_TEMPLATES` rows are emailed as updates, only to the update address: `tests/integration/tests/update_email_sends.rs` (T730). More: `(never 35)` in NOTES.md.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/messaging/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/messaging/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
