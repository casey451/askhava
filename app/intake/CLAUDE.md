# askhava-intake: requests, conversations and homeowner consent

## Purpose

The layer-7 crate that turns a homeowner's inbound text into a request (`docs/architecture.md` section 3). T038 built the first reply: the consent text first on every path, after the 911 text on a 911-list match, never asking for a reply and never sent twice for one request, and the `intake` tables. Not here yet: consent records (T181), the 911 flow and SAFE hold (T039), the intake questions (T040), routing (T043), web chat (T103).

## What it owns

- Migration `0053_intake_tables.sql`: the `intake` schema and grants; `intake.homeowner` (one per vault contact), `intake.property` (vault contact ID and town only, PRIV-028), `intake.conversation` (one open per homeowner by a partial unique index; `language`, `first_inbox_id`, `consent_template`, `consent_sent_at`), `intake.consent_record` (append-only for `askhava_app`: channel, inbox ID or web-chat session, consent version, time; CON-005, CON-022), `intake.intake_answer`, `intake.emergency_event`, `intake.recruiting_demand` (empty until T040, T039, T043).
- `src/rules.rs`: pure `plan` (with `Inbound`, `Settings`, `Plan`, `Send`, `Then`), `consent_template`, `consent_language`, `wanted_language`, `reads_spanish`, `asks_who`, `language_code`, `language_from_code`; template IDs `CONSENT_TEMPLATE`, `CONSENT_CALL_TEMPLATE`, `SAFETY_TEMPLATE`, `DISCLOSURE_TEMPLATE`.
- `src/service.rs`: `on_text` (`TextIn`, `Handled`, `Queued`), `disclosure`, `conversation`.
- `src/models.rs`: `IntakeError`, one row type per table, the crate-private queries. `sqlx.toml`: `timestamptz` as jiff timestamps.
- Templates it uses in `app/messaging/templates/`: `consent`, `consent_safety_call` and `who_is_this` (added by T038, D-907), `emergency_911`.
- `src/api.rs`: the public surface; `api::rules` re-exports the pure rules.

## Rules it enforces

- CON-001, D-008, D-131: `rules::plan`: a contact with no open request gets `Send::Safety` first on a 911-list match, then `Send::Consent`; an opt-out or a HELP, INFO or AYUDA text (already answered by `messaging`) gets nothing and opens nothing.
- CON-002, CON-003, CON-004, CON-031: the `consent` template is the exact CON-031 text: every CON-002 line, both links, no fee line, no "Reply OK" or "Reply YES".
- CON-026, TXT-039, D-065: `consent_language`: Spanish only when `wanted_language` says Spanish (`reads_spanish`: two words from its list or ñ, ¿, ¡; or the AI reading) and Spanish conversations are on.
- CON-027: `plan` on an open request gives `Then::IntakeReply` (agreement words record nothing) or `Then::DiscloseThenAsk` when `asks_who`; `service::disclosure` renders the CON-027 line for T040 to put before the next question. Consent goes once per request: one open conversation per homeowner (`ON CONFLICT ... DO NOTHING`), the outbox key over template, contact and conversation ID (TXT-011).
- CALL-022, D-384: `consent_template(true)` picks `consent_safety_call` only while the safety voice call is on.
- JOB-005: `on_text` writes the `intake.consent_sent` audit row in the transaction that opens the request and queues its texts.

## Never break

- The English consent text equals CON-031, GSM-7 in 2 segments: `tests::consent::td_con_18_english_consent_text_is_con_031_exactly`.
- Spanish consent only while the setting is on: `td_con_18_spanish_consent_text_only_while_spanish_is_on`, `td_txt_24_*`.
- Consent first and at most once per request; STOP and HELP never get it: `tests::prop_consent_first::td_con_01_consent_text_first_and_once_per_request`, `tests::consent::td_con_01_*`.
- The 911 text comes before the consent text in `Handled::texts` (`Queued` rows with their template), the order the caller sends: `td_con_01_911_text_first_then_the_consent_text`.
- No homeowner text asks for "Reply OK", "Reply YES" or "Responda OK": `td_con_24_no_homeowner_text_asks_for_a_consent_reply`.
- Consent records cannot be changed by the app role: `consent_records_are_append_only_for_the_app_role`.

## How to change

- Edit the consent text: CON-031 first, then `consent` (and `consent_safety_call`) in both `en.json` and `es.json` with their `segments`; run both test commands below and `cargo test -p askhava-messaging tests::template_registry`.
- Add a Spanish marker word or a who-is-this phrase: `SPANISH_WORDS` or `WHO_PHRASES` in `rules.rs` (folded: lower case, no accents or apostrophes), plus a case in `tests/consent.rs`.
- Add a table or column: a new migration (`-- intake (T###)` first line), its row type in `models.rs`, regenerate `.sqlx/` (online build with `SQLX_OFFLINE_DIR=.sqlx`).

## Tests that guard it

- `src/tests/consent.rs` (TD-CON-01, TD-CON-11, TD-CON-14, TD-CON-18, TD-TXT-24): `cargo test -p askhava-intake tests::consent`.
- `src/tests/prop_consent_first.rs` (TD-CON-01, TD-CON-11, TD-CON-24 properties): `cargo test -p askhava-intake tests::prop_consent_first`.
