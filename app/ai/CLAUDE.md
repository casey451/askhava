# askhava-ai: the AI adapter's caller, strict schemas, no tools

## Purpose
The layer-3 crate every AI call goes through (`docs/architecture.md` section 3).

## Never break
- No request ever carries tools: `tests/no_tools.rs::requests_carry_no_tools_and_tool_use_is_discarded`.
- The schema file (minus `$schema` and `$id`), `FIELDS` and `output_config` stay equal: `tests/schemas.rs::rust_types_output_config_and_schema_file_are_equal`, `rust_reading_type_takes_exactly_the_schema_fields`.
- The schema file holds no keyword the Claude API refuses (`minimum`, `maximum`, length and array limits; `docs/brains/ai-intake.md` section 2): ranges and caps live in `parse_reading`; the stand-in answers such a schema with the API's 400 (`tests/contract/test_stand_in_ai_contract.py`, `tests/schemas.rs::confidence_out_of_zero_to_one_is_discarded`, TD-AI-28, D-2760).
- `ai` depends on no crate that sends, charges or releases: `tests/no_tools.rs::output_never_sends_charges_or_releases`.
- No transaction is open during a network call; the call row and its alert commit together after it (R11).
- `ai.call_stat` holds no text, only codes and IDs (PRIV-016).
- `ai.reading` has no text column; trade and job type leave only through the off-database stream (`ai_zero_retention.rs::readings_go_off_database_as_codes_and_the_week_is_counts`).
- The gate reads only the newest `ai.provider_terms` row, its zero-retention date included; a newer incomplete row closes it (`ai_zero_retention.rs::a_newer_incomplete_terms_row_closes_the_gate`).
- Other crates reach only the gated `read_intake`; never export `service::read_intake` (AI-008).
- `IntakeReading::missing_info` is for records and question choice only: never send it to a person or put it into a template (D-814).
- `Transport::post` takes the AI job code; the idempotency key is derived from job and subject (SEC-018, D-814).
- The adapter drops error headers and bodies, so through `AdapterTransport` a 429 counts as having no `retry-after` and any 400 as a spend limit; both fail toward the owner alert (D-810).
- `app/ai/data/word_022_phrases.json` equals `scripts/trace_spec.py`'s `_NOT_A_CONTRACTOR_PATTERNS`: `tests/docs/test_word_022_phrases.py`.
- The openers list holds no brand or market name and every question-bank and card fixed text passes `adds_fact`: `tests/adds_fact_openers.rs` (TD-AI-26).
- A failed check never returns the AI's text, and the phrase file failing to load refuses every AI text (D-988).

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/ai/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/ai/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
