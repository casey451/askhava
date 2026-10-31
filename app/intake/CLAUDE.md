# askhava-intake: requests, conversations and homeowner consent

## Purpose

The layer-7 crate that turns a homeowner's inbound text into a request (`docs/architecture.md` section 3). T038 built the first reply: the consent text first on every path, after the 911 text on a 911-list match, never asking for a reply and never sent twice for one request, and the `intake` tables. T181 added the consent record, the card and text gates and the homeowner terms acceptance at a pick. T152 added the one INT-004 911 text's words, quiet-hours kind and 5-minute check. T039 added the 911-list stem matcher on every message, the SAFE hold, the INT-037 question, "How soon?", the INT-033 night count, the emergency-or-regular choice and the emergency release. Not here yet: the rest of the intake questions and their order (T040), routing (T043), web chat (T103).

## What it owns

- Migration `0053_intake_tables.sql`: the `intake` schema and grants; `intake.homeowner` (one per vault contact), `intake.property` (vault contact ID and town only, PRIV-028), `intake.conversation` (one open per homeowner by a partial unique index; `language`, `first_inbox_id`, `consent_template`, `consent_sent_at`), `intake.consent_record` (append-only for `askhava_app`: channel, inbox ID or web-chat session, consent version, time; CON-005, CON-022), `intake.intake_answer`, `intake.emergency_event`, `intake.recruiting_demand` (empty until T040, T039, T043).
- Migration `0054_intake_terms_acceptance.sql` (T181): `intake.terms_acceptance`, one row per pick (unique `pick_id`), append-only for `askhava_app`: document, version, `text_sha256`, language, channel `quote_page` or `text_pick` (inbox ID required for and only for `text_pick`), assent line, time (TERMS-033, D-934).
- `src/rules.rs`: pure `plan` (with `Inbound`, `Settings`, `Plan`, `Send`, `Then`), `consent_template`, `consent_language`, `wanted_language`, `reads_spanish`, `asks_who`, `language_code`, `language_from_code`; template IDs `CONSENT_TEMPLATE`, `CONSENT_CALL_TEMPLATE`, `SAFETY_TEMPLATE`, `DISCLOSURE_TEMPLATE`.
- `src/service.rs`: `receive_text` and `receive_answer` (the entry points: the screen and first reply plus the consent record, one transaction; `Asked`), `on_text` (`TextIn`, `Handled` with `Screened`, `Queued`; test builds only), `ask_how_soon`, `answer_how_soon` (`Trade`, `Answered`), `offer_emergency_choice`, `answer_choice`, `release_emergency` (`EMERGENCY_RELEASED_KIND`), `safe_hold`, `how_soon`, `emergency_events`, template IDs `BEEP_TEMPLATE`, `HOW_SOON_TEMPLATE`, `NEEDED_BY_TEMPLATE`, `disclosure`, `conversation`, `consent_records`, `card_allowed`, `may_text`, `record_terms_acceptance` (`TermsAcceptance`, `TermsChannel`), `terms_acceptances`.
- `src/safety_text.rs` (T152): `safety_text` (the INT-004 text in English or Spanish), `safety_outgoing` (its `Kind::Safety911` quiet-hours view), `sent_in_time` and `SAFETY_WITHIN` (5 minutes).
- `src/emergency.rs` (T039, pure): `check` (`Check`, `Hazard`), `is_safe_reply`, `urgency` (`Urgency`), `choice` (`Choice`), `beep_is_alarm`; re-exported in `api::rules`.
- `src/emergency_count.rs` (T039): `count` (from `businesses::api::night_count`), `is_night`, `offer` (`Offer`), `CHOICE_TEMPLATE`, `NIGHT_MANY_TEMPLATE`, `NIGHT_ONE_TEMPLATE`, `NIGHT_NONE_TEMPLATE`.
- `src/models.rs`: `IntakeError`, one row type per table, the crate-private queries. `sqlx.toml`: `timestamptz` as jiff timestamps.
- Templates it uses in `app/messaging/templates/`: `consent`, `consent_safety_call` and `who_is_this` (added by T038, D-907), `emergency_911`; `beep_question`, `how_soon`, `needed_by_day`, `emergency_choice`, `night_count_many` (`{count}`, `{trade}`), `night_count_one`, `night_count_none` (`{trade}`) (T039, D-979).
- `src/api.rs`: the public surface; `api::rules` re-exports the pure rules.

## Rules it enforces

- CON-001, D-008, D-131: `rules::plan`: a contact with no open request gets `Send::Safety` first on a 911-list match, then `Send::Consent`; an opt-out or a HELP, INFO or AYUDA text (already answered by `messaging`) gets nothing and opens nothing.
- CON-002, CON-003, CON-004, CON-031: the `consent` template is the exact CON-031 text: every CON-002 line, both links, no fee line, no "Reply OK" or "Reply YES".
- CON-026, TXT-039, D-065: `consent_language`: Spanish only when `wanted_language` says Spanish (`reads_spanish`: two words from its list or ñ, ¿, ¡; or the AI reading) and Spanish conversations are on.
- CON-027: `plan` on an open request gives `Then::IntakeReply` (agreement words record nothing) or `Then::DiscloseThenAsk` when `asks_who`; `service::disclosure` renders the CON-027 line for T040 to put before the next question. The rendered line already starts "Ask Hava: "; T040 joins it to the question without a second "Ask Hava:" prefix (D-907). Consent goes once per request: one open conversation per homeowner (`ON CONFLICT ... DO NOTHING`), the outbox key over template, contact and conversation ID (TXT-011).
- CALL-022, D-384: `consent_template(true)` picks `consent_safety_call` only while the safety voice call is on.
- INT-004, D-436: one fixed 911 text, `emergency_911`, in English or Spanish (`safety_text`; its Spanish template is `spanish_stays_on`, so a Spanish match gets Spanish even while Spanish conversations are off); it is never held by quiet hours (`safety_outgoing`, QH-004) and must go within `SAFETY_WITHIN` of the message (`sent_in_time`); it holds no number but 911 and no cause, symptom, health, heat, water, shutoff, utility or appliance advice. No safety-contacts file exists (D-350 withdrawn by D-436); never add a local or utility number to any text or page.
- INT-002, INT-003, INT-026, D-165, D-436, D-980: `handle` (behind `receive_text`/`receive_answer`) runs `emergency::check` first on every message and replaces the caller's `Inbound::emergency` flag with its result; street-address patterns (at most 3 capitalized name words and a house number before a suffix, never a joining or hazard word, D-982) are removed first and the name-and-address answer (`Asked::NameAndAddress`) is never checked. A match queues the 911 text (before the consent text on a new request, in the message's language) and writes an `intake.emergency_event` row with `text_queued_at` and the `intake.emergency_matched` audit row.
- INT-005, INT-024, INT-025, D-982: `safe_hold` is true while any 911 text of the request waits for SAFE; a match while it waits sends no second 911 text (one per hold) and keeps the hold; a safe reply (safe, seguro, segura, we're safe/ok/fine, plain OK or Yes; never a question or a negated reply: "not safe", "I don't feel safe", "is it safe to go back in?") sets `safe_at` and the request continues; nothing is sent after SAFE. Card tasks call `safe_hold` as well as `card_allowed`.
- INT-037, D-361: a smoke or CO alarm said only to beep gets `beep_question` (after the consent text on a new request) and a `beep_question` event; the next reply is stored once as `emergency_beep` (`alarm` or `chirp`); `alarm` sends the 911 text and holds the cards. A chirp, or a beep with a battery word, is a normal request.
- INT-030, INT-006, INT-033: `answer_how_soon` stores `how_soon` (`today`, `few_days`, `no_rush`, `by_day`); `by_day` always asks `needed_by_day`; `today`, or any other answer after a 911 match answered SAFE, offers the choice (while a 911 text waits for SAFE nothing is offered, `Answered::choice_after_safe` is set and the conversation calls `offer_emergency_choice` after SAFE; negated answers such as "not urgent" never read as Today, D-982) through `emergency_count::offer` (by day `emergency_choice` alone; 8 p.m. to 8 a.m. Arizona the count text first; a count of 0 sends only `night_count_none` and stores `emergency_choice = regular`).
- INT-029, D-164: `answer_choice` stores `emergency_choice`; `release_emergency` enqueues one `intake.emergency_released` task (dedupe key `intake.emergency_released.<request>`) only with consent, no SAFE hold, the emergency choice and an `intake.property` row (the ZIP and area check); the conversation calls it again once the ZIP is in. `quotes` handles the event.
- JOB-005: `on_text` writes the `intake.consent_sent` audit row in the transaction that opens the request and queues its texts.
- CON-005, D-933: `receive_text` writes one `intake.consent_record` (channel `sms`, inbox ID, arrival time, version `<template>:<language>:<12 hex of the SHA256 of the rendered text>`) only when the text queued the consent text; replies, opt-outs and HELP record nothing. `on_text` alone writes no record (T038's `consent_records_are_append_only_for_the_app_role` counts on that), so other crates call `receive_text` only.
- CON-007, CON-021: `card_allowed(conversation)` is true only with a consent record; `may_text(contact)` only for a homeowner with one (tenants and on-site contacts never have one). Card and send tasks call them first. `card_allowed` gates text-delivered cards only; the CON-007 web-chat request with no verified mobile number is T103's to allow. `may_text` covers homeowner request texts only and ignores opt-outs; HELP and opt-out confirmations and the suppression list are the send path's.
- TERMS-001, TERMS-016, TERMS-033, D-934: only a pick calls `record_terms_acceptance`; intake hashes the published text itself; the same pick twice returns `false` and stores nothing more; an unknown request is `IntakeError::UnknownRequest`.

## Never break

- The English consent text equals CON-031, GSM-7 in 2 segments: `tests::consent::td_con_18_english_consent_text_is_con_031_exactly`.
- Spanish consent only while the setting is on: `td_con_18_spanish_consent_text_only_while_spanish_is_on`, `td_txt_24_*`.
- Consent first and at most once per request; STOP and HELP never get it: `tests::prop_consent_first::td_con_01_consent_text_first_and_once_per_request`, `tests::consent::td_con_01_*`.
- The 911 text comes before the consent text in `Handled::texts` (`Queued` rows with their template), the order the caller sends: `td_con_01_911_text_first_then_the_consent_text`.
- No homeowner text asks for "Reply OK", "Reply YES" or "Responda OK": `td_con_24_no_homeowner_text_asks_for_a_consent_reply`.
- The 911 text equals its templates exactly, goes at any hour within 5 minutes, and comes before the consent text on the text and START-return paths: `tests::safety_text::td_int_17_*`, `int_004_*`, `td_con_26_*`.
- No phone number but 911, no 211, utility name or "cooling center" in any template or page, and no safety-contacts file: `tests/docs/test_no_phone_numbers_in_templates.py`.
- The 911 list matches its TD-INT-01 phrases and nothing on the not-list; street names and the address answer never match: `tests::emergency::td_int_01_*`.
- The AI flag never sends the 911 text: `tests::emergency::td_int_10_the_ai_flag_never_stands_in_for_the_stem_match`.
- No card before SAFE, and an emergency is released once, only after consent, SAFE, the choice and the ZIP: `tests/integration/tests/emergency_flow.rs`.
- Consent records cannot be updated, deleted or truncated by the app role: `consent_records_are_append_only_for_the_app_role`.
- Two first texts at once open one request and queue one consent text: `td_con_01_concurrent_first_texts_queue_one_consent_text`.
- One consent record per request, none from a reply; one full terms row per pick, append-only: `tests/integration/tests/consent_records.rs`.

## How to change

- Add a 911-list word: INT-002 first (a spec change with a decision), then the list in `emergency.rs` (folded words; stems match word starts, phrases whole words), plus a case in `MATCHES` or `CLEAR` in `tests/emergency.rs`.

- Edit the consent text: CON-031 first, then `consent` (and `consent_safety_call`) in both `en.json` and `es.json` with their `segments`; run both test commands below and `cargo test -p askhava-messaging tests::template_registry`.
- Add a Spanish marker word or a who-is-this phrase: `SPANISH_WORDS` or `WHO_PHRASES` in `rules.rs` (folded: lower case, no accents or apostrophes), plus a case in `tests/consent.rs`.
- Edit the 911 text: INT-004 first (a spec change with a decision), then `emergency_911` in both `en.json` and `es.json` with `segments`, then `INT_004_EN`/`INT_004_ES` in `tests/safety_text.rs`; run both safety test commands.
- Add a table or column: a new migration (`-- intake (T###)` first line), its row type in `models.rs`, regenerate `.sqlx/` (online build with `SQLX_OFFLINE_DIR=.sqlx`).

## Tests that guard it

- `src/tests/consent.rs` (TD-CON-01, TD-CON-11, TD-CON-14, TD-CON-18, TD-TXT-24): `cargo test -p askhava-intake tests::consent`.
- `src/tests/prop_consent_first.rs` (TD-CON-01, TD-CON-11, TD-CON-24 properties): `cargo test -p askhava-intake tests::prop_consent_first`.
- `src/tests/safety_text.rs` (TD-INT-17, TD-CON-26): `cargo test -p askhava-intake tests::safety_text`.
- `src/tests/emergency.rs` (TD-INT-01, TD-INT-10, TD-INT-13, TD-INT-15, TD-INT-16): `cargo test -p askhava-intake tests::emergency`.
- `tests/integration/tests/emergency_flow.rs` (TD-INT-12, TD-INT-13, TD-INT-15, TD-INT-16): `cargo test -p askhava-integration --test emergency_flow`.
- `tests/docs/test_no_phone_numbers_in_templates.py` (TD-INT-17): `python3 -m pytest tests/docs/test_no_phone_numbers_in_templates.py -q -p no:cacheprovider`.
- `tests/integration/tests/consent_records.rs` (TD-CON-02, TD-CON-03, TD-CON-14, TD-CON-24, TD-TERMS-07): `cargo test -p askhava-integration --test consent_records`.
