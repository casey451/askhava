# askhava-messaging: texts, emails and letters

## Purpose

The layer-3 crate that sends and receives messages (`docs/architecture.md` section 3). T026 built the template registry (every text is a fixed English or Spanish template chosen by ID and language); T027 built quiet hours; T028 the outbox and the sender directory; T178 inbox dedupe and delivery statuses; T029 opt-outs and the one suppression list. Not here yet: inbound routing (the scrubbed text and flags), HELP and START replies (T030), retries, email and letters (later messaging tasks).

## What it owns

- `templates/en.json`, `templates/es.json`: one key per template ID with `audience`, `segments` (the count recorded for TXT-044), `text`; Spanish adds `draft: true` and, for the texts that stay on while Spanish conversations are off, `spanish_stays_on: true`. Shared paths: each task adds only its own keys, in both files (D-491).
- `src/templates.rs`: `Registry` (`parse`, `embedded`, `render`, `language_for`, `template`, `ids`), `count_segments`.
- `area_codes.json`: the area-code table, `{zone: [codes]}`, US, territories and Canada only (D-100, D-139).
- `src/rules.rs`: pure quiet-hours logic: `AreaCodes`, `decide`, `retry`, `plan_release`, `Kind`, `Outgoing`, `Decision`, `Held`, `Release`; and the pure opt-out rules: `PROVIDER_OPT_OUT_WORDS`, `SPANISH_OPT_OUT_WORDS`, `APP_OPT_OUT_WORDS`, `RESUBSCRIBE_WORDS`, `OPT_OUT_PHRASES` (with pattern IDs), `detect_opt_out`, `classify`, `ProviderKeyword`, `Detected`, `Classified`, `Listing`, `may_send_to`, `CONFIRM_WITHIN`.
- `src/optout.rs`: the suppression list and opt-out flow: `on_inbound_text`, `suppress`, `send_checked`, `InboundText`, `Outcome`, `RevokeChannel`.
- `src/service.rs`: the outbox (`enqueue`, `send`, `may_text`, `NewText`, `Relation`, `To`, `SEND_ACTION`), `delivery_status` (TXT-013) and the sender directory (`sender_add`, `sender_remove`, `sender_business`).
- `src/handlers.rs`: Twilio's webhooks: `receive_inbound`, `receive_status`, `Callback` (`Recorded`, `Repeat`, `Ignored`).
- `src/status.rs`: `DeliveryStatus` (`from_twilio`, `as_str`, `rank`); every match on it lives here (SEC-031).
- `src/models.rs`: `OutboxError`, `InboxError` and the crate-private queries on `messaging.outbox`, `messaging.sender_directory`, `messaging.webhook_event`, `messaging.inbox`, `messaging.delivery_event`, `messaging.optout_event` and `messaging.suppression`.
- `sqlx.toml`: every `timestamptz` reads as a jiff timestamp (D-488).
- Migration `0042_messaging_outbox.sql`: the `messaging` schema and grants, `messaging.outbox` (unique `idempotency_key`, `subject_ids`, `recipient_kind` homeowner/business, `status` queued/sending/sent, `attempts`, `provider_sid`, `body` NULL once sent), `messaging.sender_directory` (blind-index digests per key version, unique per digest).
- Migration `0043_messaging_inbox_delivery.sql`: `messaging.webhook_event` (source `twilio_inbound` or `twilio_status`, event key, primary key on both), `messaging.inbox` (unique `message_sid`), `messaging.delivery_event` (provider SID, folded status, error code, identity `id` for order). No `received_at` has a default: the caller binds it (D-863).
- Migration `0044_messaging_optout.sql`: `messaging.optout_event` (append-only for `askhava_app`: action `opt_out`/`start`, channel, how detected, the number's active `homeowner-phone` digest, `effective_at`), `messaging.suppression` (one row per index version and digest, the event and `since`), and the outbox's terminal status `blocked` (body NULL).
- `src/api.rs`: the only public surface: `render`, `template_ids`, the quiet-hours functions, the outbox and sender-directory functions and the re-exported types.

## Rules it enforces

- TXT-001, TXT-003, WORD-008: `Registry::parse` refuses a kind missing a language, a Spanish text not marked draft, or a text not beginning "Ask Hava:".
- TXT-002, TXT-039, BIZ-018: `Registry::language_for` gives Spanish only when wanted and Spanish conversations are on or the template stays on; the caller passes the person's own language.
- TXT-004: `render` fills `{name}` placeholders in one pass; a missing value or unknown ID is an error.
- TXT-044: `count_segments` is a GSM-7 counter; the test fails if a text needs more segments than recorded.
- TXT-005 to TXT-008, TXT-010: `tests/docs/test_template_scan.py` (emoji, links, shorteners, ROC link, promotion words, "reply CANCEL").
- QH-001, QH-002, QH-003: `rules::decide` (8:00 to before 20:00 in `America/Phoenix` and the area-code zone; a code not in the table, or a non +1 number, is an error and is never texted, whatever the kind).
- QH-004, QH-005: `rules::night_exception` (the one list of night exceptions; emails and texts, homeowners and businesses alike).
- QH-006, QH-008: `Decision::Hold { until }` is the next instant both zones are open; `rules::retry` is the same rule.
- QH-007, TXT-025: `rules::plan_release` drops held texts for a closed job (the caller's `job_closed` check) except one that announces the close, and merges held `Kind::Update`s per job and recipient.
- TXT-009: `service::may_text`; `enqueue` refuses `Relation::PastCustomer` unless `last_inbound` is strictly after `closed_at`, and `send` checks `To::Homeowner`'s relation again before claiming; the caller supplies both times (D-846).
- PRIV-003, PRIV-004: `send` reads a homeowner's phone from the vault (`read_contact`, `Reader::Messaging`, `Purpose::Send`) for the row's `recipient_id`; only a business phone comes from the caller (D-849). Each row stores `recipient_kind` (from the `Relation` at enqueue) and `send` refuses a `To` of the other kind with `RecipientKind`, so a homeowner's row cannot skip the vault read or TXT-009. `NewText`'s `Debug` omits the body.
- SEC-037: `mark_sent` sets `body` to NULL, so a link token is not kept after the send (D-850).
- TXT-011, SEC-018: `enqueue` derives the key (`sms.send` over template ID, recipient ID, subject IDs) and inserts `ON CONFLICT DO NOTHING`; `send` claims in one statement, calls the carrier outside any transaction with the key re-derived from the stored IDs, then marks `sent` only if still `sending`; a sent row never reaches the carrier (D-847). Subject IDs are sorted before hashing. No lease: the killed-worker resend and concurrent sends both rely on the carrier's key dedupe, verify with vendor before Gate C. No error makes a row terminal: a TXT-009 or kind refusal leaves it `queued`, a vault or carrier failure leaves it `sending`; the retry task decides when to give up.
- TXT-012, SEC-017, SEC-039: `receive_inbound` and `receive_status` take a `VerifiedWebhook` (Twilio only) and `now` from the caller's injected `Clock` (each row's `received_at`, never the database's `now()`, D-863), parse MessageSid, MessageStatus and ErrorCode once (`BadWebhook` otherwise, nothing recorded), and insert the event into `messaging.webhook_event` in the transaction of its effect: inbound on its MessageSid (first: one `inbox` row), status on (MessageSid, MessageStatus) (first: one `delivery_event`). A repeat changes nothing (D-861).
- TXT-013: `delivery_status` is `Queued`/`Sending` from the row until it is sent, then the highest-ranked `delivery_event` for its `provider_sid` (`Sent` if none); a late earlier status never moves it back; a non-outbound status (`received`) is `Callback::Ignored` (D-861).
- D-492, PRIV-002: `sender_add` normalizes the phone and stores only `IndexPurpose::SenderDir` digests, replacing the phone's earlier ones in one transaction (whatever business they named, so re-adding a `phone_ref` under another business moves it); a number another phone holds is `SenderTaken`.

- CON-008, CON-010, CON-011: `rules::detect_opt_out` (folds case, Spanish accents and punctuation; a sole word, or a listed phrase on word boundaries); it runs on the scrubbed words, and T151 runs the same function on help emails, email replies and form messages (CON-035).
- CON-030, D-493: `PROVIDER_OPT_OUT_WORDS`, `SPANISH_OPT_OUT_WORDS` and `RESUBSCRIBE_WORDS` equal `stand_ins/provider_optout.json`; whether the provider answered comes from the webhook's `OptOutType` (`ProviderKeyword::from_opt_out_type`), never from re-matching its rules (D-866).
- CON-009, CON-030: `optout::on_inbound_text` queues the CON-009 confirmation (`opt_out_confirmation`, Spanish for a Spanish phrase) only when the provider did not answer, the number was not listed already (decided by whether its own `ON CONFLICT DO NOTHING` insert added a suppression row, so of two concurrent opt-outs only one confirms, D-870) and texting is not paused (`ops` switches `texting`, `all`); paused gives `Outcome::OptedOut { email: true }` and never a later text.
- CON-013, CON-035: `optout::send_checked` refuses a listed number (`OutboxError::Suppressed`) except `help_reply` and the `opt_out_confirmation` whose subject IDs hold the current listing's `event_id`, within `CONFIRM_WITHIN` (5 minutes) of it (`rules::may_send_to` with `models::listing`), whatever the carrier blocks. Every send must go through it, not `service::send`, which `api` still exports for T028's and T178's tests (D-867, D-870).
- CON-014: a 21610 refusal (watched by a wrapper around the carrier) lists the number (channel `carrier`) and marks the row `blocked`; `models::claim` claims only `queued` or `sending`, so it is never retried.
- CON-015, CON-016, CON-017: only the provider's `OptOutType=START` (its START or UNSTOP match) lifts the listing (`Outcome::Resubscribed`: a new request from the consent text), so the app's list and the provider's block agree; "Start.", a START without `OptOutType`, "yes" and any other message from a listed number give `Outcome::Suppressed` (the caller emails or lists it in the digest) (D-870). An emergency flag raises a critical `messaging.opted_out_emergency` alert on the inbox row, on `Suppressed` and on an opt-out alike (`Outcome::OptedOut { alert }`).
- CON-022, CON-035, CON-036: every opt-out and START is logged in `messaging.optout_event` with channel, `word:<word>` / `phrase:<id>` / `provider` / `carrier:21610`, and time; `optout::suppress` is the entry for the email, form, portal and owner channels (confirmed by email or on screen, never by text).

## Never break

- Every text begins "Ask Hava:" in both languages: `tests::template_registry::every_kind_has_both_languages_and_begins_ask_hava`.
- Spanish never reaches a person while the setting is off, except the `spanish_stays_on` texts: `tests::template_registry::spanish_only_while_the_setting_is_on_except_the_stay_on_texts`.
- Both consent texts stay GSM-7 and 2 segments: `tests::template_registry::recorded_segments_hold_and_the_consent_texts_are_gsm7`.
- No emoji, shortener, off-domain link, promotion wording or "reply CANCEL": `tests/docs/test_template_scan.py`.
- A new any-hour message kind is added only to `rules::night_exception` after QH-004 lists it; tests `tests::quiet_hours::td_qh_02_night_exceptions_sent` and `td_qh_02_cards_and_confirms_at_night`.
- An area code outside the table is refused: `tests::quiet_hours::td_qh_01_unknown_area_code_never_texted`; every table code has a zone: `td_qh_01_every_table_code_has_a_zone`.
- Window edges and daylight saving: `tests::quiet_hours::td_qh_01_*`, `tests::prop_quiet_hours_props`.
- A killed worker's resend carries the same key and the carrier delivers once: `tests/integration/tests/outbox.rs::td_txt_04_a_killed_worker_resends_under_the_same_key_and_delivers_once`.
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

## How to change

- Edit or add a text: change the key in both `en.json` and `es.json`, set `segments` to the new count (the test prints the counted values with `--nocapture`; raising it is a logged decision, TXT-044), run both test commands below.
- Spanish text: keep `draft: true`; a text that must stay on while the setting is off needs `spanish_stays_on: true` and a back-translation entry in `docs/decisions.md` (TXT-039).
- Add an area code: put it under every zone it spans in `area_codes.json` (all must be open to send; the parser rejects a repeat within one zone); only US, territory and Canadian codes (D-100); run the quiet-hours tests.
- Add a night exception: QH-004 first, then `Kind` and `night_exception`, then a case in `td_qh_02_*`.
- Map a new Twilio status: add it to `DeliveryStatus::from_twilio` (and, for a new variant, `as_str`, `rank` and the migration's check in a new migration), then run the inbox test.
- Add an ID to a text's identity: put it in `NewText::subject`; never a phone, name or body word (SEC-018). Changing what the key covers changes every key, so it is a logged decision.
- Add an opt-out word: CON-008 and CON-030 first; a provider word goes in `PROVIDER_OPT_OUT_WORDS` and `stand_ins/provider_optout.json` (a harness task) and the live Advanced Opt-Out together, an app-only word in `APP_OPT_OUT_WORDS`. Add a phrase: CON-010 first, then `OPT_OUT_PHRASES` with a new pattern ID (never one that fits a normal request) and a case in `td_con_04_*`.
- After changing a query, regenerate `.sqlx/` (`cargo sqlx prepare --workspace -- --all-targets --all-features`, or an online build with `SQLX_OFFLINE_DIR=.sqlx`).

## Tests that guard it

- `src/tests/template_registry.rs` (TD-TXT-01, TXT-044): `cargo test -p askhava-messaging tests::template_registry`.
- `tests/docs/test_template_scan.py` (TD-TXT-02): `python3 -m pytest tests/docs/test_template_scan.py -q -p no:cacheprovider`.
- `src/tests/quiet_hours.rs` (TD-QH-01, TD-QH-02): `cargo test -p askhava-messaging tests::quiet_hours`.
- `src/tests/prop_quiet_hours_props.rs` (TD-QH-01 property): `cargo test -p askhava-messaging tests::prop_quiet_hours_props`.
- `tests/integration/tests/outbox.rs` (TD-TXT-03, TD-TXT-04 TXT-011 cases, D-492): `cargo test -p askhava-integration --test outbox`.
- `tests/integration/tests/inbox.rs` (TD-TXT-04 TXT-012 and TXT-013 cases, D-861): `cargo test -p askhava-integration --test inbox`.
- `src/tests/optout_words.rs` (TD-CON-04, TD-CON-20 word lists): `cargo test -p askhava-messaging tests::optout_words`.
- `src/tests/prop_optout_props.rs` (TD-CON-04, TD-CON-05 properties): `cargo test -p askhava-messaging tests::prop_optout_props`.
- `tests/integration/tests/optout_flow.rs` (TD-CON-05, TD-CON-13, TD-CON-20): `cargo test -p askhava-integration --test optout_flow`.
