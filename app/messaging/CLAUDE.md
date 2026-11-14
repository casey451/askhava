# askhava-messaging: texts, emails and letters

## Purpose
The layer-3 crate that sends and receives messages (`docs/architecture.md` section 3). T026 built the template registry (every text is a fixed English or Spanish template chosen by ID and language); T027 built quiet hours; T028 the outbox and the sender directory; T178 inbox dedupe and delivery statuses; T029 opt-outs and the one suppression list. T030 the HELP, INFO and AYUDA reply.

## Never break
- Every text begins "Ask Hava:" (`<short_name>:`) in both languages: `tests::template_registry::every_kind_has_both_languages_and_begins_ask_hava`. Under the embedded brand every text, subject, footer and sender is today's byte for byte, and a missing prefix or a segment over the recorded count under a longer brand is refused: `tests/security/tests/notice_addresses.rs` (`td_word_35_*`). More: `(never 1)` in NOTES.md.
- Spanish never reaches a person while the setting is off, except the `spanish_stays_on` texts: `tests::template_registry::spanish_only_while_the_setting_is_on_except_the_stay_on_texts`.
- Both consent texts stay GSM-7 and 2 segments: `tests::template_registry::recorded_segments_hold_and_the_consent_texts_are_gsm7`.
- No emoji, shortener, off-domain link, promotion wording or "reply CANCEL": `tests/docs/test_template_scan.py`.
- A new any-hour message kind is added only to `rules::night_exception` after QH-004 lists it; tests `tests::quiet_hours::td_qh_02_night_exceptions_sent` and `td_qh_02_cards_and_confirms_at_night`.
- An area code outside the table is refused: `tests::quiet_hours::td_qh_01_unknown_area_code_never_texted`; every table code has a zone: `td_qh_01_every_table_code_has_a_zone`.
- Window edges and daylight saving: `tests::quiet_hours::td_qh_01_*`, `tests::prop_quiet_hours_props`.
- A killed worker's row is never resent blind: one carrier call, `unknown` after 5 minutes, the email copy: `tests/integration/tests/outbox.rs::td_txt_04_a_killed_worker_never_resends_blind_and_the_row_is_unknown_after_five_minutes`. More: `(never 8)` in NOTES.md.
- No text to a past customer who has not written first: `td_txt_03_no_text_to_a_past_customer_who_has_not_written_first`.
- No plaintext phone in the sender directory: `the_sender_directory_matches_linked_phones_only`.
- A homeowner is re-checked at send, texted at her vault number, and a sent row keeps no body: `td_txt_03_a_homeowner_is_rechecked_and_texted_at_her_vault_number`.
- A row goes only to its recipient kind; subject order does not change the key: `a_row_goes_only_to_its_recipient_kind_and_subject_order_is_one_text`.
- The same inbound MessageSid three times is one inbox row: `tests/integration/tests/inbox.rs::td_txt_04_the_same_inbound_message_id_three_times_is_handled_once`.
- Each status is recorded once and never moves a text back: `td_txt_04_delivery_statuses_are_recorded`.
- The app's word lists equal the provider's settings file: `tests::optout_words::td_con_20_word_lists_equal_the_provider_settings`.
- "can stop by at 3", "stop sign is down", "please cancel my request" are not opt-outs: `tests::optout_words::td_con_04_a_word_in_another_sense_is_not_an_opt_out`.
- Nothing but the confirmation within 5 minutes and HELP reaches a listed number: `tests::prop_optout_props::nothing_but_confirmation_and_help_until_start`, `tests/integration/tests/optout_flow.rs`.
- A 21610 refusal is never retried: `optout_flow.rs::td_con_05_a_21610_refusal_is_never_retried`.
- Two opt-outs at once queue one confirmation, and an earlier listing's confirmation never goes: `optout_flow.rs::td_con_05_two_opt_outs_at_once_queue_one_confirmation`, `td_con_05_only_the_current_listings_confirmation_goes`.
- No outbound text from any number but the audience's configured one; one audience never changes the other: `web_email_mode.rs::td_txt_08_every_outbound_text_uses_the_configured_number`, `td_txt_17_*`. No second emergency call and no repeated alert for one message: `web_email_mode.rs::td_txt_09_suspended_smell_gas_alerts_and_calls_once`. More: `(never 20)` in NOTES.md.
- No number in the message log, no body left at 30 days, log rows kept, opt-out evidence kept: `tests/integration/tests/message_log_retention.rs` (`td_txt_07_*`, `td_con_25_*`). A body is redacted at Twilio once, on or after day 31, never before: `retention_deletes.rs::td_priv_20_bodies_are_deleted_at_the_provider_once_on_day_31`. More: `(never 21)` in NOTES.md.
- The HELP reply holds the help email, askhava.com/help, the LIST line and the settings' times, and only the roaming line differs for Canada: `tests/integration/tests/help_start.rs`.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/messaging/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/messaging/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
- [-] (owns) No caller yet (D-2417). `src/accepted_quote_email.rs` (T388, D-3076): `ACCEPTED_QUOTE_TEMPLATE` (`accepted_quote_email`), `AcceptedQuoteSender` (`new(secret, (postal, kind), vault)`), `AcceptedQuoteFacts`, `render_accepted_quote` (en, draft es; homeowner footer; the "Add Ask Hava to your contacts" line, no link, no reward;
- [CON-016 CON-028 CON-030] (owns) `optout::run_optout_followup` (T357, D-3478): `messaging.optout_followup` (`OPTOUT_FOLLOWUP_KIND`, `FollowUp`, `queue_optout_followup`; payload inbox, contact, language, follow-up; deduped per inbox row and follow-up), queued by `server`'s inbound route; emails dated by the inbox row; no sender: nothing. Test `tests/integration/tests/optout_followups.rs`.
- [CON-009 CON-030] (rules) T357: a non-business sender's stored line off, suspended or restored and waiting (`mode_state::load_lines`, D-3477) also gives `Outcome::OptedOut { email: true }` for every new opt-out, provider word included (D-3479); `optout::listed_by` (D-3480) says whether an inbox row's opt-out is the current listing (no second CON-028 notice).
- [BIZ-040 SEC-024] (owns) T415, D-3552: `api::unsent_business_mailboxes(conn, now)`: `recipient_id` of business `email_outbox` rows not `sent` and queued within 7 days of `now`; the server's contact sweep keeps those mailboxes. Tests: `app/server/tests/business_contacts.rs` (TD-PRIV-29).
- [CALL-011 CALL-012] (owns) T391, D-3564, D-3566: `src/sender_pending.rs` `sender_line` (a linked phone's business and line ID) and `sender_put_pending` (already-keyed digests under a line, in a savepoint; `SenderTaken` rolls back only it). Test `tests/integration/tests/link_reply.rs`.
