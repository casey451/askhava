# askhava-messaging: texts, emails and letters

## Purpose

The layer-3 crate that sends and receives messages (`docs/architecture.md` section 3). T026 built only the template registry: every text is a fixed English or Spanish template chosen by ID and language. Not here yet: the outbox and inbox, opt-outs, quiet hours, email and letters (later messaging tasks).

## What it owns

- `templates/en.json`, `templates/es.json`: one key per template ID with `audience`, `segments` (the count recorded for TXT-044), `text`; Spanish adds `draft: true` and, for the texts that stay on while Spanish conversations are off, `spanish_stays_on: true`. Shared paths: each task adds only its own keys, in both files (D-491).
- `src/templates.rs`: `Registry` (`parse`, `embedded`, `render`, `language_for`, `template`, `ids`), `count_segments`.
- `src/api.rs`: the only public surface: `render`, `template_ids` and the re-exported types.

## Rules it enforces

- TXT-001, TXT-003, WORD-008: `Registry::parse` refuses a kind missing a language, a Spanish text not marked draft, or a text not beginning "Ask Hava:".
- TXT-002, TXT-039, BIZ-018: `Registry::language_for` gives Spanish only when wanted and Spanish conversations are on or the template stays on; the caller passes the person's own language.
- TXT-004: `render` fills `{name}` placeholders in one pass; a missing value or unknown ID is an error.
- TXT-044: `count_segments` is a GSM-7 counter; the test fails if a text needs more segments than recorded.
- TXT-005 to TXT-008, TXT-010: `tests/docs/test_template_scan.py` (emoji, links, shorteners, ROC link, promotion words, "reply CANCEL").

## Never break

- Every text begins "Ask Hava:" in both languages: `tests::template_registry::every_kind_has_both_languages_and_begins_ask_hava`.
- Spanish never reaches a person while the setting is off, except the `spanish_stays_on` texts: `tests::template_registry::spanish_only_while_the_setting_is_on_except_the_stay_on_texts`.
- Both consent texts stay GSM-7 and 2 segments: `tests::template_registry::recorded_segments_hold_and_the_consent_texts_are_gsm7`.
- No emoji, shortener, off-domain link, promotion wording or "reply CANCEL": `tests/docs/test_template_scan.py`.

## How to change

- Edit or add a text: change the key in both `en.json` and `es.json`, set `segments` to the new count (the test prints the counted values with `--nocapture`; raising it is a logged decision, TXT-044), run both test commands below.
- Spanish text: keep `draft: true`; a text that must stay on while the setting is off needs `spanish_stays_on: true` and a back-translation entry in `docs/decisions.md` (TXT-039).

## Tests that guard it

- `src/tests/template_registry.rs` (TD-TXT-01, TXT-044): `cargo test -p askhava-messaging tests::template_registry`.
- `tests/docs/test_template_scan.py` (TD-TXT-02): `python3 -m pytest tests/docs/test_template_scan.py -q -p no:cacheprovider`.
