# askhava-messaging: texts, emails and letters

## Purpose

The layer-3 crate that sends and receives messages (`docs/architecture.md` section 3). T026 built the template registry (every text is a fixed English or Spanish template chosen by ID and language); T027 built quiet hours. Not here yet: the outbox and inbox, opt-outs, email and letters (later messaging tasks).

## What it owns

- `templates/en.json`, `templates/es.json`: one key per template ID with `audience`, `segments` (the count recorded for TXT-044), `text`; Spanish adds `draft: true` and, for the texts that stay on while Spanish conversations are off, `spanish_stays_on: true`. Shared paths: each task adds only its own keys, in both files (D-491).
- `src/templates.rs`: `Registry` (`parse`, `embedded`, `render`, `language_for`, `template`, `ids`), `count_segments`.
- `area_codes.json`: the area-code table, `{zone: [codes]}`, US, territories and Canada only (D-100, D-139).
- `src/rules.rs`: pure quiet-hours logic: `AreaCodes`, `decide`, `retry`, `plan_release`, `Kind`, `Outgoing`, `Decision`, `Held`, `Release`.
- `src/api.rs`: the only public surface: `render`, `template_ids`, the quiet-hours functions and the re-exported types.

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

## Never break

- Every text begins "Ask Hava:" in both languages: `tests::template_registry::every_kind_has_both_languages_and_begins_ask_hava`.
- Spanish never reaches a person while the setting is off, except the `spanish_stays_on` texts: `tests::template_registry::spanish_only_while_the_setting_is_on_except_the_stay_on_texts`.
- Both consent texts stay GSM-7 and 2 segments: `tests::template_registry::recorded_segments_hold_and_the_consent_texts_are_gsm7`.
- No emoji, shortener, off-domain link, promotion wording or "reply CANCEL": `tests/docs/test_template_scan.py`.
- A new any-hour message kind is added only to `rules::night_exception` after QH-004 lists it; tests `tests::quiet_hours::td_qh_02_night_exceptions_sent` and `td_qh_02_cards_and_confirms_at_night`.
- An area code outside the table is refused: `tests::quiet_hours::td_qh_01_unknown_area_code_never_texted`; every table code has a zone: `td_qh_01_every_table_code_has_a_zone`.
- Window edges and daylight saving: `tests::quiet_hours::td_qh_01_*`, `tests::prop_quiet_hours_props`.

## How to change

- Edit or add a text: change the key in both `en.json` and `es.json`, set `segments` to the new count (the test prints the counted values with `--nocapture`; raising it is a logged decision, TXT-044), run both test commands below.
- Spanish text: keep `draft: true`; a text that must stay on while the setting is off needs `spanish_stays_on: true` and a back-translation entry in `docs/decisions.md` (TXT-039).
- Add an area code: put it under every zone it spans in `area_codes.json` (all must be open to send; the parser rejects a repeat within one zone); only US, territory and Canadian codes (D-100); run the quiet-hours tests.
- Add a night exception: QH-004 first, then `Kind` and `night_exception`, then a case in `td_qh_02_*`.

## Tests that guard it

- `src/tests/template_registry.rs` (TD-TXT-01, TXT-044): `cargo test -p askhava-messaging tests::template_registry`.
- `tests/docs/test_template_scan.py` (TD-TXT-02): `python3 -m pytest tests/docs/test_template_scan.py -q -p no:cacheprovider`.
- `src/tests/quiet_hours.rs` (TD-QH-01, TD-QH-02): `cargo test -p askhava-messaging tests::quiet_hours`.
- `src/tests/prop_quiet_hours_props.rs` (TD-QH-01 property): `cargo test -p askhava-messaging tests::prop_quiet_hours_props`.
